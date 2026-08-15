use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use tempfile::TempDir;

use crate::core::skill::parse_skill_md_content;
use crate::error::{AppError, DirectDownloadFailureReason};

const MAX_UNPACKED_BYTES: u64 = 25 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 1000;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ArchiveLimits {
    pub(crate) max_unpacked_bytes: u64,
    pub(crate) max_entries: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveEntryKind {
    File,
    Directory,
}

struct ExtractionState {
    entries: usize,
    unpacked_bytes: u64,
    paths: HashMap<String, RegisteredPath>,
    limits: ArchiveLimits,
}

struct RegisteredPath {
    original: String,
    kind: ArchiveEntryKind,
}

impl ExtractionState {
    fn new(limits: ArchiveLimits) -> Self {
        Self {
            entries: 0,
            unpacked_bytes: 0,
            paths: HashMap::new(),
            limits,
        }
    }

    fn register(&mut self, path: &Path, kind: ArchiveEntryKind) -> Result<(), AppError> {
        self.entries = self.entries.checked_add(1).ok_or_else(too_many_entries)?;
        if self.entries > self.limits.max_entries {
            return Err(too_many_entries());
        }

        let components = path
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        for index in 1..components.len() {
            let original = components[..index].join("/");
            let parent = original.to_lowercase();
            match self.paths.get(&parent) {
                Some(RegisteredPath {
                    kind: ArchiveEntryKind::File,
                    ..
                }) => return Err(path_conflict()),
                Some(RegisteredPath {
                    original: registered,
                    kind: ArchiveEntryKind::Directory,
                }) if registered != &original => {
                    return Err(path_conflict());
                }
                Some(RegisteredPath {
                    kind: ArchiveEntryKind::Directory,
                    ..
                }) => {}
                None => {
                    self.paths.insert(
                        parent,
                        RegisteredPath {
                            original,
                            kind: ArchiveEntryKind::Directory,
                        },
                    );
                }
            }
        }

        let original = components.join("/");
        let folded = original.to_lowercase();
        if self
            .paths
            .insert(folded, RegisteredPath { original, kind })
            .is_some()
        {
            return Err(path_conflict());
        }
        Ok(())
    }

    fn ensure_unpacked_bytes_fit(&self, size: u64) -> Result<(), AppError> {
        let unpacked_bytes = self
            .unpacked_bytes
            .checked_add(size)
            .ok_or_else(archive_too_large)?;
        if unpacked_bytes > self.limits.max_unpacked_bytes {
            return Err(archive_too_large());
        }
        Ok(())
    }

    fn record_unpacked_bytes(&mut self, size: u64) -> Result<(), AppError> {
        self.ensure_unpacked_bytes_fit(size)?;
        self.unpacked_bytes += size;
        Ok(())
    }
}

pub(crate) fn materialize_download(bytes: &[u8]) -> Result<TempDir, AppError> {
    if bytes.is_empty() {
        return Err(download_error(DirectDownloadFailureReason::EmptyContent));
    }

    let temp = tempfile::TempDir::new()?;
    if std::str::from_utf8(bytes)
        .ok()
        .is_some_and(|content| parse_skill_md_content(content).is_ok())
    {
        fs::write(temp.path().join("SKILL.md"), bytes)?;
        return Ok(temp);
    }

    if bytes.starts_with(&[0x50, 0x4b]) {
        extract_archive(
            bytes,
            ArchiveFormat::Zip,
            temp.path(),
            ArchiveLimits {
                max_unpacked_bytes: MAX_UNPACKED_BYTES,
                max_entries: MAX_ARCHIVE_ENTRIES,
            },
        )?;
    } else if bytes.starts_with(&[0x1f, 0x8b]) {
        extract_archive(
            bytes,
            ArchiveFormat::TarGz,
            temp.path(),
            ArchiveLimits {
                max_unpacked_bytes: MAX_UNPACKED_BYTES,
                max_entries: MAX_ARCHIVE_ENTRIES,
            },
        )?;
    } else {
        extract_archive(
            bytes,
            ArchiveFormat::Tar,
            temp.path(),
            ArchiveLimits {
                max_unpacked_bytes: MAX_UNPACKED_BYTES,
                max_entries: MAX_ARCHIVE_ENTRIES,
            },
        )?;
    }
    Ok(temp)
}

pub(crate) fn extract_archive(
    bytes: &[u8],
    format: ArchiveFormat,
    target: &Path,
    limits: ArchiveLimits,
) -> Result<(), AppError> {
    match format {
        ArchiveFormat::Zip => extract_zip(bytes, target, limits),
        ArchiveFormat::Tar => extract_tar(Cursor::new(bytes), target, "tar", limits),
        ArchiveFormat::TarGz => extract_tar(
            flate2::read::GzDecoder::new(Cursor::new(bytes)),
            target,
            "tar.gz",
            limits,
        ),
    }
}

fn extract_zip(bytes: &[u8], target: &Path, limits: ArchiveLimits) -> Result<(), AppError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| invalid_content("zip", &error))?;
    let mut state = ExtractionState::new(limits);
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| invalid_content("zip entry", &error))?;
        let path = normalize_archive_path(entry.name())?;
        let kind = if entry.is_dir() {
            ArchiveEntryKind::Directory
        } else {
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(unsafe_archive());
            }
            ArchiveEntryKind::File
        };
        state.register(&path, kind)?;
        if kind == ArchiveEntryKind::Directory {
            state.record_unpacked_bytes(entry.size())?;
            fs::create_dir_all(target.join(path))?;
            continue;
        }
        state.ensure_unpacked_bytes_fit(entry.size())?;
        let content = read_bounded_entry(&mut entry, &mut state, "zip entry")?;
        write_file(target, &path, &content)?;
    }
    if state.entries == 0 {
        return Err(download_error(DirectDownloadFailureReason::EmptyContent));
    }
    Ok(())
}

fn extract_tar<R: Read>(
    reader: R,
    target: &Path,
    label: &str,
    limits: ArchiveLimits,
) -> Result<(), AppError> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|error| invalid_content(label, &error))?;
    let mut state = ExtractionState::new(limits);
    for entry in entries {
        let mut entry =
            entry.map_err(|error| invalid_content(&format!("{label} entry"), &error))?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(unsafe_archive());
        }
        let kind = if entry_type.is_dir() {
            ArchiveEntryKind::Directory
        } else if entry_type.is_file() {
            ArchiveEntryKind::File
        } else {
            return Err(unsafe_archive());
        };
        let raw_path = entry
            .path()
            .map_err(|error| invalid_content(&format!("{label} path"), &error))?;
        let path = normalize_archive_path(&raw_path.to_string_lossy())?;
        state.register(&path, kind)?;
        if kind == ArchiveEntryKind::Directory {
            state.record_unpacked_bytes(entry.size())?;
            fs::create_dir_all(target.join(path))?;
            continue;
        }
        state.ensure_unpacked_bytes_fit(entry.size())?;
        let content = read_bounded_entry(&mut entry, &mut state, &format!("{label} entry"))?;
        write_file(target, &path, &content)?;
    }
    if state.entries == 0 {
        return Err(download_error(DirectDownloadFailureReason::EmptyContent));
    }
    Ok(())
}

fn read_bounded_entry<R: Read>(
    reader: &mut R,
    state: &mut ExtractionState,
    context: &str,
) -> Result<Vec<u8>, AppError> {
    let remaining = state
        .limits
        .max_unpacked_bytes
        .saturating_sub(state.unpacked_bytes);
    let mut content = Vec::new();
    reader
        .take(remaining.saturating_add(1))
        .read_to_end(&mut content)
        .map_err(|error| invalid_content(context, &error))?;
    let actual_size = u64::try_from(content.len()).map_err(|_| archive_too_large())?;
    state.record_unpacked_bytes(actual_size)?;
    Ok(content)
}

fn normalize_archive_path(raw: &str) -> Result<PathBuf, AppError> {
    let windows_absolute = raw.as_bytes().get(1) == Some(&b':')
        && raw.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && raw.as_bytes().get(2) == Some(&b'/');
    if raw.is_empty() || raw.contains('\0') || raw.contains('\\') || windows_absolute {
        return Err(unsafe_archive());
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(unsafe_archive());
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            _ => return Err(unsafe_archive()),
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(unsafe_archive());
    }
    Ok(normalized)
}

fn write_file(target: &Path, relative: &Path, content: &[u8]) -> Result<(), AppError> {
    let path = target.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn download_error(reason: DirectDownloadFailureReason) -> AppError {
    AppError::DirectDownloadFailed { reason }
}

fn invalid_content(context: &str, error: &impl std::fmt::Display) -> AppError {
    log::warn!("Direct download contains invalid {context}: {error}");
    download_error(DirectDownloadFailureReason::InvalidContent)
}

fn unsafe_archive() -> AppError {
    download_error(DirectDownloadFailureReason::UnsafeArchive)
}

fn too_many_entries() -> AppError {
    download_error(DirectDownloadFailureReason::TooManyEntries)
}

fn archive_too_large() -> AppError {
    download_error(DirectDownloadFailureReason::ArchiveTooLarge)
}

fn path_conflict() -> AppError {
    unsafe_archive()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::error::{AppError, DirectDownloadFailureReason};

    use super::{materialize_download, MAX_ARCHIVE_ENTRIES, MAX_UNPACKED_BYTES};

    fn invalid_reason(result: Result<tempfile::TempDir, AppError>) -> DirectDownloadFailureReason {
        match result.expect_err("download should be rejected") {
            AppError::DirectDownloadFailed { reason } => reason,
            error => panic!("unexpected error: {error}"),
        }
    }

    #[test]
    fn rejects_empty_archives_and_entry_count_overflow() {
        let mut empty = Vec::new();
        tar::Builder::new(&mut empty).finish().unwrap();
        assert_eq!(
            invalid_reason(materialize_download(&empty)),
            DirectDownloadFailureReason::EmptyContent
        );

        let mut archive = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut archive);
            for index in 0..=MAX_ARCHIVE_ENTRIES {
                let mut header = tar::Header::new_gnu();
                header.set_size(0);
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append_data(&mut header, format!("entry-{index}"), std::io::empty())
                    .unwrap();
            }
            builder.finish().unwrap();
        }
        assert_eq!(
            invalid_reason(materialize_download(&archive)),
            DirectDownloadFailureReason::TooManyEntries
        );
    }

    #[test]
    fn rejects_unknown_content_and_absolute_paths() {
        assert_eq!(
            invalid_reason(materialize_download(b"not a skill or archive")),
            DirectDownloadFailureReason::InvalidContent
        );
        assert_eq!(
            invalid_reason(
                super::normalize_archive_path("/SKILL.md")
                    .map(|_| { tempfile::tempdir().unwrap() })
            ),
            DirectDownloadFailureReason::UnsafeArchive
        );
        assert_eq!(
            invalid_reason(
                super::normalize_archive_path("C:/outside/SKILL.md")
                    .map(|_| tempfile::tempdir().unwrap())
            ),
            DirectDownloadFailureReason::UnsafeArchive
        );
    }

    #[test]
    fn rejects_unpacked_content_over_the_limit() {
        let mut archive = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut archive);
            let content = vec![b'x'; MAX_UNPACKED_BYTES as usize + 1];
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "large.bin", content.as_slice())
                .unwrap();
            builder.finish().unwrap();
        }
        assert_eq!(
            invalid_reason(materialize_download(&archive)),
            DirectDownloadFailureReason::ArchiveTooLarge
        );
    }

    #[test]
    fn rejects_zip_content_over_the_limit_before_reading_it() {
        let mut bytes = Vec::new();
        {
            let mut archive = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
            archive
                .start_file("large.bin", zip::write::SimpleFileOptions::default())
                .unwrap();
            archive
                .write_all(&vec![b'x'; MAX_UNPACKED_BYTES as usize + 1])
                .unwrap();
            archive.finish().unwrap();
        }
        assert_eq!(
            invalid_reason(materialize_download(&bytes)),
            DirectDownloadFailureReason::ArchiveTooLarge
        );
    }

    #[test]
    fn rejects_zip_content_when_declared_size_is_smaller_than_actual_content() {
        let mut bytes = Vec::new();
        {
            let mut archive = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
            archive
                .start_file(
                    "large.bin",
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            archive
                .write_all(&vec![b'x'; MAX_UNPACKED_BYTES as usize + 1])
                .unwrap();
            archive.finish().unwrap();
        }

        let central_directory = bytes
            .windows(4)
            .rposition(|window| window == [0x50, 0x4b, 0x01, 0x02])
            .expect("central directory entry");
        bytes[central_directory + 24..central_directory + 28].copy_from_slice(&1_u32.to_le_bytes());

        assert_eq!(
            invalid_reason(materialize_download(&bytes)),
            DirectDownloadFailureReason::ArchiveTooLarge
        );
    }

    #[test]
    fn rejects_parent_directories_that_only_differ_by_case() {
        let mut state = super::ExtractionState::new(super::ArchiveLimits {
            max_unpacked_bytes: 1024,
            max_entries: 10,
        });
        state
            .register(std::path::Path::new("Foo/a"), super::ArchiveEntryKind::File)
            .unwrap();
        assert!(matches!(
            state.register(std::path::Path::new("foo/b"), super::ArchiveEntryKind::File,),
            Err(AppError::DirectDownloadFailed {
                reason: DirectDownloadFailureReason::UnsafeArchive
            })
        ));
    }
}
