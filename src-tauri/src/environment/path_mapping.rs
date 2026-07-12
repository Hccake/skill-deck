use crate::error::AppError;

pub fn wsl_unc_to_linux_path(path: &str, distro_name: &str) -> Result<String, AppError> {
    let normalized = path.replace('/', "\\");
    let without_prefix = normalized
        .strip_prefix("\\\\wsl.localhost\\")
        .or_else(|| normalized.strip_prefix("\\\\wsl$\\"))
        .ok_or_else(|| AppError::Path {
            message: format!("not a WSL UNC path: {path}"),
        })?;
    let (distro, remainder) = without_prefix
        .split_once('\\')
        .unwrap_or((without_prefix, ""));
    if !distro.eq_ignore_ascii_case(distro_name) {
        return Err(AppError::Path {
            message: format!("path belongs to WSL distro '{distro}', expected '{distro_name}'"),
        });
    }
    if remainder.is_empty() {
        return Ok("/".to_string());
    }
    Ok(format!("/{}", remainder.replace('\\', "/")))
}

pub fn linux_path_to_host_path(path: &str) -> Option<String> {
    let remainder = path.strip_prefix("/mnt/")?;
    let (drive, tail) = remainder.split_once('/').unwrap_or((remainder, ""));
    if drive.len() != 1 || !drive.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let drive = drive.to_ascii_uppercase();
    if tail.is_empty() {
        Some(format!("{drive}:\\"))
    } else {
        Some(format!("{drive}:\\{}", tail.replace('/', "\\")))
    }
}

pub fn host_path_to_linux_path(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    if bytes.len() > 2 && !matches!(bytes[2], b'\\' | b'/') {
        return None;
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    let tail = path[2..].trim_start_matches(['\\', '/']).replace('\\', "/");
    if tail.is_empty() {
        Some(format!("/mnt/{drive}"))
    } else {
        Some(format!("/mnt/{drive}/{tail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{host_path_to_linux_path, linux_path_to_host_path, wsl_unc_to_linux_path};

    #[test]
    fn maps_current_distro_unc_to_linux_path() {
        assert_eq!(
            wsl_unc_to_linux_path(
                r"\\wsl.localhost\Ubuntu-24.04\home\alice\项目",
                "Ubuntu-24.04"
            )
            .expect("map UNC"),
            "/home/alice/项目"
        );
        assert_eq!(
            wsl_unc_to_linux_path(r"\\wsl$\Ubuntu-24.04\home\alice", "Ubuntu-24.04")
                .expect("map legacy UNC"),
            "/home/alice"
        );
    }

    #[test]
    fn rejects_unc_for_another_distro() {
        assert!(wsl_unc_to_linux_path(r"\\wsl.localhost\Debian\home\alice", "Ubuntu").is_err());
    }

    #[test]
    fn maps_standard_drvfs_path_to_windows_drive() {
        assert_eq!(
            linux_path_to_host_path("/mnt/c/Code/demo").expect("map drvfs"),
            r"C:\Code\demo"
        );
        assert!(linux_path_to_host_path("/srv/demo").is_none());
    }

    #[test]
    fn maps_windows_drive_path_to_standard_drvfs_path() {
        assert_eq!(
            host_path_to_linux_path(r"C:\Users\alice\AppData\Local\Temp\skill deck")
                .expect("map host temp"),
            "/mnt/c/Users/alice/AppData/Local/Temp/skill deck"
        );
        assert_eq!(
            host_path_to_linux_path(r"d:/Code/demo").expect("map slash path"),
            "/mnt/d/Code/demo"
        );
        assert!(host_path_to_linux_path(r"\\server\share\demo").is_none());
    }
}
