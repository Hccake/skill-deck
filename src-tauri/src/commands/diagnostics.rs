use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::environment::opener::open_authorized_resource;
use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::error::AppError;

const RECENT_DIAGNOSTICS_MAX_BYTES: usize = 32 * 1024;

#[tauri::command]
#[specta::specta]
pub async fn open_diagnostics_directory(app: AppHandle) -> Result<(), AppError> {
    let path = app.path().app_log_dir().map_err(|error| AppError::Path {
        message: error.to_string(),
    })?;
    open_diagnostics_path(path, open_authorized_resource)
}

#[tauri::command]
#[specta::specta]
pub async fn read_recent_diagnostics(app: AppHandle) -> Result<String, AppError> {
    let path = app.path().app_log_dir().map_err(|error| AppError::Path {
        message: error.to_string(),
    })?;
    read_recent_diagnostics_from(&path)
}

fn open_diagnostics_path(
    path: PathBuf,
    open: impl FnOnce(&ResourceLocator) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let native_path = path
        .clone()
        .into_os_string()
        .into_string()
        .map_err(|_| AppError::Path {
            message: "diagnostics directory is not valid UTF-8".to_string(),
        })?;
    std::fs::create_dir_all(&path)?;

    open(&ResourceLocator {
        environment: EnvironmentRef::Host,
        native_path,
    })
}

fn read_recent_diagnostics_from(path: &std::path::Path) -> Result<String, AppError> {
    if !path.exists() {
        return Ok(String::new());
    }

    let mut remaining = RECENT_DIAGNOSTICS_MAX_BYTES;
    let mut newest_first = Vec::new();
    for file_path in crate::diagnostics::diagnostic_log_paths(path) {
        if remaining == 0 {
            break;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&file_path) else {
            continue;
        };
        if !metadata.file_type().is_file() {
            continue;
        }

        let separator_bytes = usize::from(!newest_first.is_empty());
        if remaining <= separator_bytes {
            break;
        }
        let mut file = open_diagnostics_log_no_follow(&file_path)?;
        let file_length = file.metadata()?.len();
        let read_length = remaining
            .saturating_sub(separator_bytes)
            .min(file_length.try_into().unwrap_or(usize::MAX));
        if read_length == 0 {
            continue;
        }
        file.seek(SeekFrom::End(-(read_length as i64)))?;
        let mut chunk = vec![0; read_length];
        file.read_exact(&mut chunk)?;
        remaining = remaining.saturating_sub(read_length + separator_bytes);
        newest_first.push(chunk);
    }

    newest_first.reverse();
    let mut bytes = Vec::with_capacity(RECENT_DIAGNOSTICS_MAX_BYTES - remaining);
    for chunk in newest_first {
        if !bytes.is_empty() {
            bytes.push(b'\n');
        }
        bytes.extend(chunk);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn open_diagnostics_log_no_follow(path: &std::path::Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }

    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || diagnostics_metadata_is_link_like(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "diagnostics log path must be a regular file",
        ));
    }
    Ok(file)
}

#[cfg(not(windows))]
fn diagnostics_metadata_is_link_like(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn diagnostics_metadata_is_link_like(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn diagnostics_directory_is_created_and_opened_as_a_host_resource() {
        let temp = tempfile::tempdir().expect("temporary app log root");
        let path = temp.path().join("nested").join("logs");
        let opened = Mutex::new(None);

        open_diagnostics_path(path.clone(), |target| {
            *opened.lock().expect("opened target lock") = Some(target.clone());
            Ok(())
        })
        .expect("open diagnostics path");

        assert!(path.is_dir());
        assert_eq!(
            opened.lock().expect("opened target lock").as_ref(),
            Some(&ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: path.to_string_lossy().into_owned(),
            })
        );
    }

    #[test]
    fn recent_diagnostics_reads_only_bounded_diagnostics_logs() {
        let temp = tempfile::tempdir().expect("temporary app log root");
        std::fs::write(temp.path().join("ordinary.log"), "must-not-be-copied")
            .expect("ordinary log");
        std::fs::write(
            temp.path().join("skill-deck-diagnostics.log"),
            "x".repeat(RECENT_DIAGNOSTICS_MAX_BYTES + 64),
        )
        .expect("diagnostics log");

        let recent = read_recent_diagnostics_from(temp.path()).expect("recent diagnostics");

        assert_eq!(recent.len(), RECENT_DIAGNOSTICS_MAX_BYTES);
        assert!(!recent.contains("must-not-be-copied"));
    }

    #[test]
    fn recent_diagnostics_ignores_unrecognized_prefixed_files() {
        let temp = tempfile::tempdir().expect("temporary app log root");
        std::fs::write(
            temp.path().join("skill-deck-diagnostics-private.log"),
            "must-not-be-copied",
        )
        .expect("unrecognized prefixed log");

        let recent = read_recent_diagnostics_from(temp.path()).expect("recent diagnostics");

        assert_eq!(recent, "");
    }

    #[cfg(unix)]
    #[test]
    fn recent_diagnostics_does_not_follow_log_symlinks() {
        let temp = tempfile::tempdir().expect("temporary app log root");
        let private = temp.path().join("private.log");
        std::fs::write(&private, "must-not-be-copied").expect("private log");
        std::os::unix::fs::symlink(&private, temp.path().join("skill-deck-diagnostics.log"))
            .expect("diagnostics symlink");

        let recent = read_recent_diagnostics_from(temp.path()).expect("recent diagnostics");

        assert_eq!(recent, "");
    }

    #[cfg(unix)]
    #[test]
    fn diagnostics_log_open_does_not_follow_a_symlink() {
        let temp = tempfile::tempdir().expect("temporary app log root");
        let private = temp.path().join("private.log");
        let link = temp.path().join("skill-deck-diagnostics.log");
        std::fs::write(&private, "must-not-be-copied").expect("private log");
        std::os::unix::fs::symlink(&private, &link).expect("diagnostics symlink");

        assert!(open_diagnostics_log_no_follow(&link).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn diagnostics_log_open_rejects_a_reparse_point() {
        let temp = tempfile::tempdir().expect("temporary app log root");
        let private = temp.path().join("private");
        let link = temp.path().join("skill-deck-diagnostics.log");
        std::fs::create_dir_all(&private).expect("private target directory");
        junction::create(&private, &link).expect("diagnostics junction");

        assert!(open_diagnostics_log_no_follow(&link).is_err());
    }

    #[test]
    fn recent_diagnostics_is_empty_before_the_first_record() {
        let temp = tempfile::tempdir().expect("temporary app log root");

        assert_eq!(
            read_recent_diagnostics_from(temp.path()).expect("empty diagnostics"),
            ""
        );
    }

    #[cfg(unix)]
    #[test]
    fn diagnostics_directory_rejects_non_utf8_paths_before_opening() {
        use std::os::unix::ffi::OsStringExt;

        let temp = tempfile::tempdir().expect("temporary app log root");
        let path = temp.path().join(std::ffi::OsString::from_vec(vec![0xff]));
        let error =
            open_diagnostics_path(path.clone(), |_| Ok(())).expect_err("invalid UTF-8 path");

        assert!(matches!(error, AppError::Path { .. }));
        assert!(!path.exists());
    }
}
