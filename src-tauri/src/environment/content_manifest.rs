use std::future::Future;
use std::pin::Pin;

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::environment::runtime::PhysicalTargetKey;
use crate::environment::types::ResourceLocator;
use crate::error::AppError;

const CONTENT_MANIFEST_FORMAT_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContentManifestRecordKind {
    Directory,
    File,
    Symlink,
}

impl ContentManifestRecordKind {
    fn hash_tag(self) -> u8 {
        match self {
            Self::Directory => b'd',
            Self::File => b'f',
            Self::Symlink => b'l',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentManifestRecord {
    pub relative_path: String,
    pub kind: ContentManifestRecordKind,
    pub content_digest: Option<String>,
    pub executable: bool,
    pub symlink_target: Option<String>,
}

impl ContentManifestRecord {
    pub fn directory(relative_path: impl Into<String>) -> Result<Self, AppError> {
        Self::new(
            relative_path,
            ContentManifestRecordKind::Directory,
            None,
            false,
            None,
        )
    }

    pub fn file(
        relative_path: impl Into<String>,
        content_digest: impl Into<String>,
        executable: bool,
    ) -> Result<Self, AppError> {
        Self::new(
            relative_path,
            ContentManifestRecordKind::File,
            Some(content_digest.into()),
            executable,
            None,
        )
    }

    pub fn symlink(
        relative_path: impl Into<String>,
        target: impl Into<String>,
    ) -> Result<Self, AppError> {
        Self::new(
            relative_path,
            ContentManifestRecordKind::Symlink,
            None,
            false,
            Some(target.into()),
        )
    }

    pub fn new(
        relative_path: impl Into<String>,
        kind: ContentManifestRecordKind,
        content_digest: Option<String>,
        executable: bool,
        symlink_target: Option<String>,
    ) -> Result<Self, AppError> {
        let relative_path = normalize_relative_path(&relative_path.into())?;
        let content_digest = content_digest.map(|digest| digest.to_ascii_lowercase());
        let valid = match kind {
            ContentManifestRecordKind::Directory => {
                content_digest.is_none() && !executable && symlink_target.is_none()
            }
            ContentManifestRecordKind::File => {
                content_digest.as_deref().is_some_and(is_sha256) && symlink_target.is_none()
            }
            ContentManifestRecordKind::Symlink => {
                content_digest.is_none()
                    && !executable
                    && symlink_target
                        .as_deref()
                        .is_some_and(|target| !target.contains('\0'))
            }
        };
        if !valid {
            return Err(manifest_error("invalid content manifest record shape"));
        }
        Ok(Self {
            relative_path,
            kind,
            content_digest,
            executable,
            symlink_target: symlink_target.map(|target| target.nfc().collect()),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentManifestHash(String);

impl ContentManifestHash {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentManifest {
    records: Vec<ContentManifestRecord>,
    hash: ContentManifestHash,
}

impl ContentManifest {
    pub fn from_records(mut records: Vec<ContentManifestRecord>) -> Result<Self, AppError> {
        records.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        if records
            .windows(2)
            .any(|pair| pair[0].relative_path == pair[1].relative_path)
        {
            return Err(manifest_error("duplicate content manifest path"));
        }
        let hash = aggregate_hash(&records);
        Ok(Self { records, hash })
    }

    #[cfg(test)]
    pub fn records(&self) -> &[ContentManifestRecord] {
        &self.records
    }

    pub fn hash(&self) -> &ContentManifestHash {
        &self.hash
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentManifestTarget {
    pub key: PhysicalTargetKey,
    pub location: ResourceLocator,
}

pub trait ContentManifestReader: Send + Sync {
    fn read<'a>(
        &'a self,
        target: &'a ContentManifestTarget,
    ) -> Pin<Box<dyn Future<Output = Result<ContentManifest, AppError>> + Send + 'a>>;
}

fn normalize_relative_path(value: &str) -> Result<String, AppError> {
    if value.is_empty()
        || value.starts_with(['/', '\\'])
        || value.contains(['\\', '\0'])
        || value
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(AppError::UnsafePath {
            path: value.to_string(),
            reason: "invalid content manifest relative path".to_string(),
        });
    }
    Ok(value.nfc().collect())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn aggregate_hash(records: &[ContentManifestRecord]) -> ContentManifestHash {
    let mut hasher = Sha256::new();
    hasher.update(b"skill-deck-content-manifest");
    hasher.update([CONTENT_MANIFEST_FORMAT_VERSION]);
    for record in records {
        hasher.update([record.kind.hash_tag(), u8::from(record.executable)]);
        hash_field(&mut hasher, record.relative_path.as_bytes());
        hash_field(
            &mut hasher,
            record.content_digest.as_deref().unwrap_or("").as_bytes(),
        );
        hash_field(
            &mut hasher,
            record.symlink_target.as_deref().unwrap_or("").as_bytes(),
        );
    }
    ContentManifestHash(format!("{:x}", hasher.finalize()))
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn manifest_error(message: &str) -> AppError {
    AppError::ConfigurationCorrupted {
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ContentManifest, ContentManifestRecord, ContentManifestRecordKind};

    fn file(path: &str, digest: &str) -> ContentManifestRecord {
        ContentManifestRecord::file(path, digest, false).unwrap()
    }

    #[test]
    fn aggregate_hash_is_independent_of_record_arrival_order() {
        let left = ContentManifest::from_records(vec![
            file("b", &"2".repeat(64)),
            file("a", &"1".repeat(64)),
        ])
        .unwrap();
        let right = ContentManifest::from_records(vec![
            file("a", &"1".repeat(64)),
            file("b", &"2".repeat(64)),
        ])
        .unwrap();

        assert_eq!(left.hash(), right.hash());
        assert_eq!(
            left.records()
                .iter()
                .map(|record| record.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn manifest_hash_covers_entry_kind_executable_empty_directories_and_symlinks() {
        let base = ContentManifest::from_records(vec![
            ContentManifestRecord::directory("empty").unwrap(),
            ContentManifestRecord::file("run.sh", "a".repeat(64), true).unwrap(),
            ContentManifestRecord::symlink("current", "versions/v1").unwrap(),
        ])
        .unwrap();

        let executable_changed = ContentManifest::from_records(vec![
            ContentManifestRecord::directory("empty").unwrap(),
            ContentManifestRecord::file("run.sh", "a".repeat(64), false).unwrap(),
            ContentManifestRecord::symlink("current", "versions/v1").unwrap(),
        ])
        .unwrap();
        let kind_changed = ContentManifest::from_records(vec![
            ContentManifestRecord::directory("empty").unwrap(),
            ContentManifestRecord::file("run.sh", "a".repeat(64), true).unwrap(),
            ContentManifestRecord::directory("current").unwrap(),
        ])
        .unwrap();
        let target_changed = ContentManifest::from_records(vec![
            ContentManifestRecord::directory("empty").unwrap(),
            ContentManifestRecord::file("run.sh", "a".repeat(64), true).unwrap(),
            ContentManifestRecord::symlink("current", "versions/v2").unwrap(),
        ])
        .unwrap();

        assert_ne!(base.hash(), executable_changed.hash());
        assert_ne!(base.hash(), kind_changed.hash());
        assert_ne!(base.hash(), target_changed.hash());
        assert_eq!(base.records()[0].kind, ContentManifestRecordKind::Symlink);
    }

    #[test]
    fn manifest_rejects_unsafe_paths_duplicate_paths_and_invalid_record_shapes() {
        for path in ["", "/absolute", "../escape", "a/../../escape", "a\\b"] {
            assert!(ContentManifestRecord::directory(path).is_err(), "{path}");
        }
        assert!(ContentManifestRecord::file("a", "not-a-sha256", false).is_err());
        assert!(ContentManifest::from_records(vec![
            ContentManifestRecord::directory("same").unwrap(),
            ContentManifestRecord::directory("same").unwrap(),
        ])
        .is_err());
    }
}
