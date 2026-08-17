use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::core::lossless_lock::{
    convert_legacy_project_document, LockRootSnapshot, LockSchema, LosslessLockDocument,
};
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::types::ResourceLocator;
use crate::error::AppError;

pub struct LockTarget {
    pub primary: ResourceLocator,
    pub legacy: Option<ResourceLocator>,
    pub schema: LockSchema,
}

pub struct LockMutationTargets {
    pub root_fields: Vec<String>,
}

pub struct LockRepository {
    io: EnvironmentLockIo,
}

pub struct LockTransaction<'a> {
    repository: &'a LockRepository,
    target: LockTarget,
    root_snapshots: HashMap<String, LockRootSnapshot>,
    pending_roots: BTreeMap<String, Value>,
}

impl LockRepository {
    pub fn new(io: EnvironmentLockIo) -> Self {
        Self { io }
    }

    pub async fn read_document(
        &self,
        target: &LockTarget,
    ) -> Result<LosslessLockDocument, AppError> {
        if let Some(bytes) = self.io.read_optional(&target.primary).await? {
            return LosslessLockDocument::parse(&bytes);
        }
        let Some(legacy) = target.legacy.as_ref() else {
            return Ok(LosslessLockDocument::empty(target.schema));
        };
        let Some(bytes) = self.io.read_optional(legacy).await? else {
            return Ok(LosslessLockDocument::empty(target.schema));
        };
        let document = LosslessLockDocument::parse(&bytes)?;
        match target.schema {
            LockSchema::Global => Ok(document),
            LockSchema::Project => convert_legacy_project_document(document),
        }
    }

    pub async fn begin(
        &self,
        target: LockTarget,
        targets: LockMutationTargets,
    ) -> Result<LockTransaction<'_>, AppError> {
        let document = self.read_document(&target).await?;
        Ok(self.transaction_from_document(target, targets, document))
    }

    fn transaction_from_document(
        &self,
        target: LockTarget,
        targets: LockMutationTargets,
        document: LosslessLockDocument,
    ) -> LockTransaction<'_> {
        let mut root_snapshots = HashMap::new();
        for field in targets.root_fields {
            root_snapshots.insert(field.clone(), document.root_snapshot(&field));
        }
        LockTransaction {
            repository: self,
            target,
            root_snapshots,
            pending_roots: BTreeMap::new(),
        }
    }
}

impl LockTransaction<'_> {
    pub fn replace_root(&mut self, field: &str, replacement: Value) -> Result<(), AppError> {
        self.require_root_snapshot(field)?;
        self.pending_roots.insert(field.to_string(), replacement);
        Ok(())
    }

    pub async fn commit(self) -> Result<(), AppError> {
        let Self {
            repository,
            target,
            root_snapshots,
            pending_roots,
        } = self;
        let mut latest = repository.read_document(&target).await?;
        for (field, replacement) in pending_roots {
            latest.replace_root(
                &field,
                root_snapshots
                    .get(&field)
                    .expect("pending root field was captured"),
                replacement,
            )?;
        }
        repository
            .io
            .write_atomic(&target.primary, latest.to_pretty_bytes()?)
            .await
    }

    fn require_root_snapshot(&self, field: &str) -> Result<&LockRootSnapshot, AppError> {
        self.root_snapshots
            .get(field)
            .ok_or_else(|| AppError::InvalidSource {
                value: format!("lock transaction did not capture root field '{field}'"),
            })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::{LockMutationTargets, LockRepository, LockTarget};
    use crate::core::lossless_lock::LockSchema;
    use crate::environment::lock_io::EnvironmentLockIo;
    use crate::environment::types::{EnvironmentRef, ResourceLocator};
    use crate::error::{AppError, LockConflictTarget};

    fn locator(path: &Path) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string_lossy().to_string(),
        }
    }

    fn project_target(primary: &Path, legacy: Option<&Path>) -> LockTarget {
        LockTarget {
            primary: locator(primary),
            legacy: legacy.map(locator),
            schema: LockSchema::Project,
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().expect("lock parent")).expect("create lock parent");
        fs::write(path, bytes).expect("write lock");
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).expect("read lock")).expect("parse lock")
    }

    #[tokio::test]
    async fn canonical_document_wins_over_legacy_and_preserves_unknown_data() {
        let temp = tempdir().expect("tempdir");
        let primary = temp.path().join("skills-lock.json");
        let legacy = temp.path().join(".agents/.skill-lock.json");
        write(
            &primary,
            include_bytes!("../../tests/fixtures/locks/cli-project-v1-future.json"),
        );
        write(
            &legacy,
            include_bytes!("../../tests/fixtures/locks/cli-legacy-project-v3-future.json"),
        );
        let repository = LockRepository::new(EnvironmentLockIo::Native);

        let document = repository
            .read_document(&project_target(&primary, Some(&legacy)))
            .await
            .expect("read canonical document")
            .into_value();

        assert_eq!(document["version"], 1);
        assert_eq!(document["futureRoot"]["schema"], 8);
        assert_eq!(
            document["skills"]["toolkit"]["computedHash"],
            "old-computed-hash"
        );
    }

    #[tokio::test]
    async fn last_selected_agents_update_preserves_legacy_defaults_and_unknown_roots() {
        let temp = tempdir().expect("tempdir");
        let primary = temp.path().join("skill-lock.json");
        write(
            &primary,
            br#"{"version":3,"defaultTargetAgents":{"global":["legacy"],"project":["legacy"]},"lastSelectedAgents":["codex"],"futureRoot":{"keep":true},"skills":{}}"#,
        );
        let repository = LockRepository::new(EnvironmentLockIo::Native);
        let mut transaction = repository
            .begin(
                LockTarget {
                    primary: locator(&primary),
                    legacy: None,
                    schema: LockSchema::Global,
                },
                LockMutationTargets {
                    root_fields: vec!["lastSelectedAgents".to_string()],
                },
            )
            .await
            .expect("begin history transaction");
        transaction
            .replace_root("lastSelectedAgents", json!(["claude-code"]))
            .expect("queue history update");

        transaction.commit().await.expect("commit history");

        let committed = read_json(&primary);
        assert_eq!(
            committed["defaultTargetAgents"],
            json!({"global":["legacy"],"project":["legacy"]})
        );
        assert_eq!(committed["lastSelectedAgents"], json!(["claude-code"]));
        assert_eq!(committed["futureRoot"], json!({"keep":true}));
    }

    #[tokio::test]
    async fn last_selected_agents_update_rejects_an_external_history_change() {
        let temp = tempdir().expect("tempdir");
        let primary = temp.path().join("skill-lock.json");
        write(
            &primary,
            br#"{"version":3,"lastSelectedAgents":["codex"],"skills":{}}"#,
        );
        let repository = LockRepository::new(EnvironmentLockIo::Native);
        let mut transaction = repository
            .begin(
                LockTarget {
                    primary: locator(&primary),
                    legacy: None,
                    schema: LockSchema::Global,
                },
                LockMutationTargets {
                    root_fields: vec!["lastSelectedAgents".to_string()],
                },
            )
            .await
            .expect("begin history transaction");
        write(
            &primary,
            br#"{"version":3,"lastSelectedAgents":["external"],"skills":{}}"#,
        );
        transaction
            .replace_root("lastSelectedAgents", json!(["claude-code"]))
            .expect("queue history update");

        assert!(matches!(
            transaction.commit().await,
            Err(AppError::LockConflict {
                target: LockConflictTarget::RootField { field }
            }) if field == "lastSelectedAgents"
        ));
        assert_eq!(
            read_json(&primary)["lastSelectedAgents"],
            json!(["external"])
        );
    }
}
