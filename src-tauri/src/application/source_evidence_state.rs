use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use uuid::Uuid;

use crate::error::AppError;

const MAX_SOURCE_EVIDENCE_BYTES: usize = environment_protocol::MAX_DOCUMENT_BYTES as usize;

#[derive(Clone)]
pub(crate) struct SourceEvidenceStateFile {
    path: PathBuf,
    #[cfg(test)]
    fail_writes: Arc<AtomicBool>,
    #[cfg(test)]
    fail_after_publish: Arc<AtomicBool>,
}

impl SourceEvidenceStateFile {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            #[cfg(test)]
            fail_writes: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            fail_after_publish: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn read_optional(&self) -> Result<Option<Vec<u8>>, AppError> {
        environment_engine::atomic_document::read_optional_bounded(
            &self.path,
            MAX_SOURCE_EVIDENCE_BYTES,
        )
        .map_err(Into::into)
    }

    pub(crate) fn write_atomic(&self, bytes: &[u8]) -> Result<(), AppError> {
        #[cfg(test)]
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("forced update-check state write failure").into());
        }

        let parent = self.parent()?;
        fs::create_dir_all(parent)?;
        let mut document = Vec::with_capacity(bytes.len().saturating_add(1));
        document.extend_from_slice(bytes);
        document.push(b'\n');
        let result = environment_engine::atomic_document::replace(&self.path, &document)
            .map_err(crate::storage::atomic_document::DocumentWriteFailure::from_engine)
            .map(|_| ());
        #[cfg(test)]
        let result = match result {
            Ok(()) if self.fail_after_publish.load(Ordering::SeqCst) => {
                Err(crate::storage::atomic_document::DocumentWriteFailure {
                    error: AppError::Io {
                        message: "forced update-check state confirmation failure".to_string(),
                    },
                    phase: crate::storage::atomic_document::WritePhase::Confirming,
                    publication:
                        crate::storage::atomic_document::PublicationState::PublishedUnconfirmed,
                })
            }
            result => result,
        };
        match result {
            Ok(()) => Ok(()),
            Err(failure)
                if failure.publication
                    == crate::storage::atomic_document::PublicationState::PublishedUnconfirmed =>
            {
                log::warn!(
                    "来源证据状态已经发布，但持久化确认失败；当前进程继续使用新状态: {}",
                    failure.error
                );
                Ok(())
            }
            Err(failure)
                if failure.publication
                    == crate::storage::atomic_document::PublicationState::OutcomeUnknown =>
            {
                match environment_engine::atomic_document::read_optional_bounded(
                    &self.path,
                    document.len(),
                ) {
                    Ok(Some(current)) if current == document => {
                        log::warn!("来源证据状态发布结果未知，重新读取后确认新状态已经生效");
                        Ok(())
                    }
                    _ => Err(failure.error),
                }
            }
            Err(failure) => Err(failure.error),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_write_failure(&self, fail: bool) {
        self.fail_writes.store(fail, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn set_post_publish_failure(&self, fail: bool) {
        self.fail_after_publish.store(fail, Ordering::SeqCst);
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

fn sync_parent(parent: &Path) -> Result<(), AppError> {
    environment_engine::atomic_document::sync_directory(parent).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_source_evidence_state_is_rejected_before_deserialization() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("source-evidence.json");
        let file = fs::File::create(&path).unwrap();
        file.set_len(u64::from(environment_protocol::MAX_DOCUMENT_BYTES) + 1)
            .unwrap();
        let state = SourceEvidenceStateFile::new(path.clone());

        let error = match state.read_optional() {
            Err(error) => error,
            Ok(_) => panic!("oversized source evidence must be rejected"),
        };

        assert!(matches!(
            error,
            AppError::Io { ref message } if message.contains("exceeds its read limit")
        ));
        assert_eq!(
            fs::metadata(path).unwrap().len(),
            u64::from(environment_protocol::MAX_DOCUMENT_BYTES) + 1
        );
    }
}
