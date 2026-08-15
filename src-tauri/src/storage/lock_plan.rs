use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::core::lossless_lock::{
    convert_legacy_project_document, LockEntrySnapshot, LockRootSnapshot, LockSchema,
    LosslessLockDocument,
};
use crate::environment::types::ResourceLocator;
use crate::error::AppError;
use crate::storage::atomic_document::AtomicDocumentIo;

#[derive(Debug, Clone)]
pub struct LockCommitReceipt {
    pub entry_snapshots: BTreeMap<String, LockEntrySnapshot>,
    pub root_snapshots: BTreeMap<String, LockRootSnapshot>,
}

#[derive(Debug, Clone)]
pub struct LockExpectedState {
    pub entry_snapshots: BTreeMap<String, LockEntrySnapshot>,
    pub root_snapshots: BTreeMap<String, LockRootSnapshot>,
}

impl LockExpectedState {
    pub fn capture<E, R, ES, RS>(
        document: &LosslessLockDocument,
        entries: E,
        root_fields: R,
    ) -> Self
    where
        E: IntoIterator<Item = ES>,
        R: IntoIterator<Item = RS>,
        ES: AsRef<str>,
        RS: AsRef<str>,
    {
        Self {
            entry_snapshots: entries
                .into_iter()
                .map(|name| {
                    let name = name.as_ref().to_string();
                    let snapshot = document.entry_snapshot(&name);
                    (name, snapshot)
                })
                .collect(),
            root_snapshots: root_fields
                .into_iter()
                .map(|field| {
                    let field = field.as_ref().to_string();
                    let snapshot = document.root_snapshot(&field);
                    (field, snapshot)
                })
                .collect(),
        }
    }

    pub fn advance(&mut self, receipt: &LockCommitReceipt) {
        self.entry_snapshots.extend(receipt.entry_snapshots.clone());
        self.root_snapshots.extend(receipt.root_snapshots.clone());
    }
}

#[derive(Debug, Clone)]
pub enum LockEntryMutation {
    Replace {
        key: String,
        replacement: Value,
    },
    Remove {
        key: String,
    },
    MoveAndReplace {
        from: String,
        to: String,
        replacement: Value,
    },
}

impl LockEntryMutation {
    fn affected_keys(&self) -> Vec<&str> {
        match self {
            Self::Replace { key, .. } | Self::Remove { key } => vec![key],
            Self::MoveAndReplace { from, to, .. } => vec![from, to],
        }
    }

    #[cfg(test)]
    pub fn target_key(&self) -> &str {
        match self {
            Self::Replace { key, .. } | Self::Remove { key } => key,
            Self::MoveAndReplace { to, .. } => to,
        }
    }

    #[cfg(test)]
    pub fn replacement(&self) -> Option<&Value> {
        match self {
            Self::Replace { replacement, .. } | Self::MoveAndReplace { replacement, .. } => {
                Some(replacement)
            }
            Self::Remove { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreparedLockMutation {
    pub target: ResourceLocator,
    pub legacy_target: Option<ResourceLocator>,
    pub schema: LockSchema,
    pub entry: LockEntryMutation,
    pub root_replacements: BTreeMap<String, Value>,
    pub expected: LockExpectedState,
}

impl PreparedLockMutation {
    #[cfg(test)]
    pub fn skill_name(&self) -> &str {
        self.entry.target_key()
    }

    #[cfg(test)]
    pub fn replacement(&self) -> Option<&Value> {
        self.entry.replacement()
    }
}

pub struct LockPlanCommitter<I> {
    io: Arc<I>,
}

impl<I> LockPlanCommitter<I>
where
    I: AtomicDocumentIo,
{
    pub fn new(io: Arc<I>) -> Self {
        Self { io }
    }

    pub async fn commit(
        &self,
        prepared: PreparedLockMutation,
    ) -> Result<LockCommitReceipt, AppError> {
        let mut latest = self.load_latest(&prepared).await?;
        for key in prepared.entry.affected_keys() {
            let expected = prepared.expected.entry_snapshots.get(key).ok_or_else(|| {
                AppError::InvalidSource {
                    value: format!("lock plan did not capture Skill '{key}'"),
                }
            })?;
            latest.validate_entry_snapshot(key, expected)?;
        }
        match prepared.entry {
            LockEntryMutation::Replace { key, replacement } => latest.replace_entry(
                prepared.schema,
                &key,
                prepared
                    .expected
                    .entry_snapshots
                    .get(&key)
                    .expect("validated snapshot"),
                replacement,
            )?,
            LockEntryMutation::Remove { key } => latest.remove_entry(
                &key,
                prepared
                    .expected
                    .entry_snapshots
                    .get(&key)
                    .expect("validated snapshot"),
            )?,
            LockEntryMutation::MoveAndReplace {
                from,
                to,
                replacement,
            } => latest.move_and_replace_entry(
                prepared.schema,
                &from,
                &to,
                prepared
                    .expected
                    .entry_snapshots
                    .get(&from)
                    .expect("validated snapshot"),
                prepared
                    .expected
                    .entry_snapshots
                    .get(&to)
                    .expect("validated snapshot"),
                replacement,
            )?,
        }
        for (field, replacement) in &prepared.root_replacements {
            let expected = prepared.expected.root_snapshots.get(field).ok_or_else(|| {
                AppError::InvalidSource {
                    value: format!("lock plan did not capture root field '{field}'"),
                }
            })?;
            latest.replace_root(field, expected, replacement.clone())?;
        }

        let receipt = LockCommitReceipt {
            entry_snapshots: prepared
                .expected
                .entry_snapshots
                .keys()
                .map(|name| (name.clone(), latest.entry_snapshot(name)))
                .collect(),
            root_snapshots: prepared
                .expected
                .root_snapshots
                .keys()
                .map(|field| (field.clone(), latest.root_snapshot(field)))
                .collect(),
        };
        self.io
            .write_atomic(&prepared.target, latest.to_pretty_bytes()?)
            .await?;
        Ok(receipt)
    }

    async fn load_latest(
        &self,
        prepared: &PreparedLockMutation,
    ) -> Result<LosslessLockDocument, AppError> {
        load_lock_document(
            self.io.as_ref(),
            &prepared.target,
            prepared.legacy_target.as_ref(),
            prepared.schema,
        )
        .await
    }
}

pub async fn load_lock_document<I>(
    io: &I,
    target: &ResourceLocator,
    legacy_target: Option<&ResourceLocator>,
    schema: LockSchema,
) -> Result<LosslessLockDocument, AppError>
where
    I: AtomicDocumentIo + ?Sized,
{
    if let Some(bytes) = io.read_optional(target).await? {
        ensure_supported_schema(&bytes, schema)?;
        return LosslessLockDocument::parse(&bytes);
    }
    let Some(legacy) = legacy_target else {
        return Ok(LosslessLockDocument::empty(schema));
    };
    let Some(bytes) = io.read_optional(legacy).await? else {
        return Ok(LosslessLockDocument::empty(schema));
    };
    let document = LosslessLockDocument::parse(&bytes)?;
    match schema {
        LockSchema::Global => Ok(document),
        LockSchema::Project => convert_legacy_project_document(document),
    }
}

fn ensure_supported_schema(bytes: &[u8], schema: LockSchema) -> Result<(), AppError> {
    let value: Value = serde_json::from_slice(bytes)?;
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| AppError::ConfigurationCorrupted {
            message: "lock version is missing".to_string(),
        })?;
    let supported = match schema {
        LockSchema::Global => 3,
        LockSchema::Project => 1,
    };
    if version > supported {
        return Err(AppError::ConfigurationCorrupted {
            message: format!("lock schema version {version} is newer than {supported}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};

    use super::*;
    use crate::environment::types::EnvironmentRef;
    use crate::storage::atomic_document::{AtomicDocumentIo, IoFuture};

    #[derive(Default)]
    struct FakeIo {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl AtomicDocumentIo for FakeIo {
        fn read_optional<'a>(
            &'a self,
            target: &'a ResourceLocator,
        ) -> IoFuture<'a, Result<Option<Vec<u8>>, AppError>> {
            Box::pin(
                async move { Ok(self.files.lock().unwrap().get(&target.native_path).cloned()) },
            )
        }

        fn write_atomic<'a>(
            &'a self,
            target: &'a ResourceLocator,
            bytes: Vec<u8>,
        ) -> IoFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.files
                    .lock()
                    .unwrap()
                    .insert(target.native_path.clone(), bytes);
                Ok(())
            })
        }
    }

    fn locator() -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: "/project/skills-lock.json".to_string(),
        }
    }

    fn initial() -> Value {
        json!({
            "version": 1,
            "skills": {
                "demo": { "source": "old", "computedHash": "old", "futureEntry": 7 },
                "other": { "source": "keep", "computedHash": "keep" }
            },
            "defaultTargetAgents": { "global": ["old"], "project": [] },
            "lastSelectedAgents": ["old"],
            "futureRoot": { "keep": true }
        })
    }

    fn setup() -> (Arc<FakeIo>, LockPlanCommitter<FakeIo>, LosslessLockDocument) {
        let io = Arc::new(FakeIo::default());
        io.files.lock().unwrap().insert(
            locator().native_path,
            serde_json::to_vec_pretty(&initial()).unwrap(),
        );
        let document =
            LosslessLockDocument::parse(&serde_json::to_vec(&initial()).unwrap()).unwrap();
        (io.clone(), LockPlanCommitter::new(io), document)
    }

    fn mutation(expected: LockExpectedState) -> PreparedLockMutation {
        PreparedLockMutation {
            target: locator(),
            legacy_target: None,
            schema: LockSchema::Project,
            entry: LockEntryMutation::Replace {
                key: "demo".to_string(),
                replacement: json!({
                "source": "new",
                "sourceType": "local",
                "computedHash": "new",
                "remoteHash": "remote"
                }),
            },
            root_replacements: Default::default(),
            expected,
        }
    }

    #[tokio::test]
    async fn target_entry_change_conflicts_but_unrelated_changes_merge() {
        let (io, committer, document) = setup();
        let expected = LockExpectedState::capture(&document, ["demo"], std::iter::empty::<&str>());
        let mut external = initial();
        external["skills"]["other"]["computedHash"] = json!("external");
        external["futureRoot"]["revision"] = json!(2);
        io.files.lock().unwrap().insert(
            locator().native_path,
            serde_json::to_vec(&external).unwrap(),
        );

        committer
            .commit(mutation(expected.clone()))
            .await
            .expect("merge");
        let merged: Value =
            serde_json::from_slice(&io.files.lock().unwrap()[&locator().native_path]).unwrap();
        assert_eq!(merged["skills"]["demo"]["computedHash"], "new");
        assert_eq!(merged["skills"]["demo"]["futureEntry"], 7);
        assert_eq!(merged["skills"]["other"]["computedHash"], "external");
        assert_eq!(merged["futureRoot"]["revision"], 2);

        let mut changed_target = merged;
        changed_target["skills"]["demo"]["computedHash"] = json!("another-writer");
        io.files.lock().unwrap().insert(
            locator().native_path,
            serde_json::to_vec(&changed_target).unwrap(),
        );
        assert!(matches!(
            committer.commit(mutation(expected)).await,
            Err(AppError::LockConflict { .. })
        ));
    }

    #[tokio::test]
    async fn owned_root_change_conflicts() {
        let (io, committer, document) = setup();
        let expected = LockExpectedState::capture(
            &document,
            ["demo"],
            ["defaultTargetAgents", "lastSelectedAgents"],
        );
        let mut external = initial();
        external["defaultTargetAgents"]["global"] = json!(["external"]);
        io.files.lock().unwrap().insert(
            locator().native_path,
            serde_json::to_vec(&external).unwrap(),
        );
        let mut prepared = mutation(expected);
        prepared.root_replacements.insert(
            "defaultTargetAgents".to_string(),
            json!({ "global": ["new"], "project": [] }),
        );
        prepared
            .root_replacements
            .insert("lastSelectedAgents".to_string(), json!(["new"]));

        assert!(matches!(
            committer.commit(prepared).await,
            Err(AppError::LockConflict { .. })
        ));
    }

    #[tokio::test]
    async fn receipt_advances_only_owned_expected_state_for_the_next_unit() {
        let (_io, committer, document) = setup();
        let mut expected = LockExpectedState::capture(
            &document,
            ["demo"],
            ["defaultTargetAgents", "lastSelectedAgents"],
        );
        let mut first = mutation(expected.clone());
        first.root_replacements.insert(
            "defaultTargetAgents".to_string(),
            json!({ "global": ["new"], "project": [] }),
        );
        first
            .root_replacements
            .insert("lastSelectedAgents".to_string(), json!(["new"]));
        let receipt = committer.commit(first).await.expect("first commit");
        expected.advance(&receipt);

        let mut second = mutation(expected);
        second.entry = LockEntryMutation::Replace {
            key: "demo".to_string(),
            replacement: json!({
                "source": "second",
                "sourceType": "local",
                "computedHash": "second"
            }),
        };
        committer.commit(second).await.expect("second commit");
    }

    #[tokio::test]
    async fn move_and_replace_validates_both_keys_and_preserves_old_unknown_fields() {
        let (io, committer, document) = setup();
        let expected = LockExpectedState::capture(
            &document,
            ["demo", "ce:review"],
            std::iter::empty::<&str>(),
        );
        let prepared = PreparedLockMutation {
            target: locator(),
            legacy_target: None,
            schema: LockSchema::Project,
            entry: LockEntryMutation::MoveAndReplace {
                from: "demo".to_string(),
                to: "ce:review".to_string(),
                replacement: json!({
                    "source": "new",
                    "sourceType": "well-known",
                    "computedHash": "new"
                }),
            },
            root_replacements: Default::default(),
            expected,
        };

        committer.commit(prepared).await.expect("move entry");
        let moved: Value =
            serde_json::from_slice(&io.files.lock().unwrap()[&locator().native_path]).unwrap();
        assert!(moved["skills"].get("demo").is_none());
        assert_eq!(moved["skills"]["ce:review"]["source"], "new");
        assert_eq!(moved["skills"]["ce:review"]["futureEntry"], 7);
    }

    #[tokio::test]
    async fn move_and_replace_rejects_a_change_to_either_key_without_writing() {
        for changed_key in ["demo", "ce:review"] {
            let (io, committer, document) = setup();
            let expected = LockExpectedState::capture(
                &document,
                ["demo", "ce:review"],
                std::iter::empty::<&str>(),
            );
            let mut external = initial();
            external["skills"][changed_key] = json!({ "source": "external" });
            let external_bytes = serde_json::to_vec(&external).unwrap();
            io.files
                .lock()
                .unwrap()
                .insert(locator().native_path, external_bytes.clone());

            let result = committer
                .commit(PreparedLockMutation {
                    target: locator(),
                    legacy_target: None,
                    schema: LockSchema::Project,
                    entry: LockEntryMutation::MoveAndReplace {
                        from: "demo".to_string(),
                        to: "ce:review".to_string(),
                        replacement: json!({ "source": "new" }),
                    },
                    root_replacements: Default::default(),
                    expected,
                })
                .await;

            assert!(matches!(result, Err(AppError::LockConflict { .. })));
            assert_eq!(
                io.files.lock().unwrap()[&locator().native_path],
                external_bytes
            );
        }
    }
}
