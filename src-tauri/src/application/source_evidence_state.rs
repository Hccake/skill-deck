use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Clone)]
pub(crate) struct SourceEvidenceStateFile {
    path: PathBuf,
}

impl SourceEvidenceStateFile {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn read_optional(&self) -> Result<Option<Vec<u8>>, AppError> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn write_atomic(&self, bytes: &[u8]) -> Result<(), AppError> {
        let parent = self.parent()?;
        fs::create_dir_all(parent)?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        temporary.write_all(bytes)?;
        temporary.write_all(b"\n")?;
        temporary.as_file_mut().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        sync_parent(parent)
    }

    pub(crate) fn quarantine(&self, now_epoch_ms: u64) -> Result<PathBuf, AppError> {
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AppError::UnsafePath {
                path: self.path.to_string_lossy().into_owned(),
                reason: "update-check state path has no UTF-8 file name".to_string(),
            })?;
        let quarantine = self.path.with_file_name(format!(
            "{file_name}.corrupt-{now_epoch_ms}-{}",
            Uuid::new_v4().simple()
        ));
        fs::rename(&self.path, &quarantine)?;
        sync_parent(self.parent()?)?;
        Ok(quarantine)
    }

    fn parent(&self) -> Result<&Path, AppError> {
        self.path.parent().ok_or_else(|| AppError::UnsafePath {
            path: self.path.to_string_lossy().into_owned(),
            reason: "update-check state path has no parent".to_string(),
        })
    }
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), AppError> {
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<(), AppError> {
    Ok(())
}
