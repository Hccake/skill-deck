use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::core::lossless_lock::{
    convert_legacy_project_document, LockEntrySnapshot, LockRootSnapshot, LockSchema,
    LosslessLockDocument,
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
    pub entries: Vec<String>,
    pub default_target_agents: bool,
}

pub struct LockRepository {
    io: EnvironmentLockIo,
}

enum PendingEntry {
    Replace(Value),
    Remove,
}

pub struct LockTransaction<'a> {
    repository: &'a LockRepository,
    target: LockTarget,
    entry_snapshots: HashMap<String, LockEntrySnapshot>,
    root_snapshots: HashMap<String, LockRootSnapshot>,
    pending_entries: BTreeMap<String, PendingEntry>,
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
        let entry_snapshots = targets
            .entries
            .into_iter()
            .map(|name| {
                let snapshot = document.entry_snapshot(&name);
                (name, snapshot)
            })
            .collect();
        let mut root_snapshots = HashMap::new();
        if targets.default_target_agents {
            for field in ["defaultTargetAgents", "lastSelectedAgents"] {
                root_snapshots.insert(field.to_string(), document.root_snapshot(field));
            }
        }
        Ok(LockTransaction {
            repository: self,
            target,
            entry_snapshots,
            root_snapshots,
            pending_entries: BTreeMap::new(),
            pending_roots: BTreeMap::new(),
        })
    }
}

impl LockTransaction<'_> {
    pub fn initial_entry(&self, skill_name: &str) -> Option<&Value> {
        self.entry_snapshots
            .get(skill_name)
            .and_then(LockEntrySnapshot::value)
    }

    pub fn replace_entry(&mut self, skill_name: &str, replacement: Value) -> Result<(), AppError> {
        self.require_entry_snapshot(skill_name)?;
        self.pending_entries
            .insert(skill_name.to_string(), PendingEntry::Replace(replacement));
        Ok(())
    }

    pub fn remove_entry(&mut self, skill_name: &str) -> Result<(), AppError> {
        self.require_entry_snapshot(skill_name)?;
        self.pending_entries
            .insert(skill_name.to_string(), PendingEntry::Remove);
        Ok(())
    }

    pub fn set_default_target_agents(
        &mut self,
        defaults: Value,
        last_selected_agents: Value,
    ) -> Result<(), AppError> {
        self.require_root_snapshot("defaultTargetAgents")?;
        self.require_root_snapshot("lastSelectedAgents")?;
        self.pending_roots
            .insert("defaultTargetAgents".to_string(), defaults);
        self.pending_roots
            .insert("lastSelectedAgents".to_string(), last_selected_agents);
        Ok(())
    }

    pub async fn commit(self) -> Result<(), AppError> {
        let Self {
            repository,
            target,
            entry_snapshots,
            root_snapshots,
            pending_entries,
            pending_roots,
        } = self;
        let mut latest = repository.read_document(&target).await?;
        for (skill_name, pending) in pending_entries {
            let snapshot = entry_snapshots
                .get(&skill_name)
                .expect("pending entry was captured");
            match pending {
                PendingEntry::Replace(replacement) => {
                    latest.replace_entry(target.schema, &skill_name, snapshot, replacement)?
                }
                PendingEntry::Remove => latest.remove_entry(&skill_name, snapshot)?,
            }
        }
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

    fn require_entry_snapshot(&self, skill_name: &str) -> Result<&LockEntrySnapshot, AppError> {
        self.entry_snapshots
            .get(skill_name)
            .ok_or_else(|| AppError::InvalidSource {
                value: format!("lock transaction did not capture '{skill_name}'"),
            })
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
            environment: EnvironmentRef::Host,
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

    fn entry_targets(names: &[&str]) -> LockMutationTargets {
        LockMutationTargets {
            entries: names.iter().map(|name| (*name).to_string()).collect(),
            default_target_agents: false,
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
        let repository = LockRepository::new(EnvironmentLockIo::Host);

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
    async fn begin_reads_legacy_without_writing_canonical() {
        let temp = tempdir().expect("tempdir");
        let primary = temp.path().join("skills-lock.json");
        let legacy = temp.path().join(".agents/.skill-lock.json");
        let legacy_bytes =
            include_bytes!("../../tests/fixtures/locks/cli-legacy-project-v3-future.json");
        write(&legacy, legacy_bytes);
        let repository = LockRepository::new(EnvironmentLockIo::Host);

        let transaction = repository
            .begin(
                project_target(&primary, Some(&legacy)),
                entry_targets(&["toolkit"]),
            )
            .await
            .expect("begin transaction");

        assert_eq!(
            transaction.initial_entry("toolkit").expect("initial entry")["remoteHash"],
            "old-remote-hash"
        );
        assert!(!primary.exists());
        assert_eq!(fs::read(&legacy).expect("read legacy"), legacy_bytes);
    }

    #[tokio::test]
    async fn first_commit_writes_canonical_and_leaves_legacy_unchanged() {
        let temp = tempdir().expect("tempdir");
        let primary = temp.path().join("skills-lock.json");
        let legacy = temp.path().join(".agents/.skill-lock.json");
        let legacy_bytes =
            include_bytes!("../../tests/fixtures/locks/cli-legacy-project-v3-future.json");
        write(&legacy, legacy_bytes);
        let repository = LockRepository::new(EnvironmentLockIo::Host);
        let mut transaction = repository
            .begin(
                project_target(&primary, Some(&legacy)),
                entry_targets(&["toolkit"]),
            )
            .await
            .expect("begin transaction");
        transaction
            .replace_entry(
                "toolkit",
                json!({"source":"owner/new", "computedHash":"new"}),
            )
            .expect("queue replacement");

        transaction.commit().await.expect("commit transaction");

        assert_eq!(read_json(&primary)["version"], 1);
        assert_eq!(
            read_json(&primary)["skills"]["toolkit"]["source"],
            "owner/new"
        );
        assert_eq!(fs::read(&legacy).expect("read legacy"), legacy_bytes);
    }

    #[tokio::test]
    async fn canonical_appearing_after_begin_merges_when_selected_entry_matches() {
        let temp = tempdir().expect("tempdir");
        let primary = temp.path().join("skills-lock.json");
        let legacy = temp.path().join(".agents/.skill-lock.json");
        write(
            &legacy,
            include_bytes!("../../tests/fixtures/locks/cli-legacy-project-v3-future.json"),
        );
        let repository = LockRepository::new(EnvironmentLockIo::Host);
        let target = project_target(&primary, Some(&legacy));
        let mut transaction = repository
            .begin(target, entry_targets(&["toolkit"]))
            .await
            .expect("begin transaction");
        let mut canonical = repository
            .read_document(&project_target(&primary, Some(&legacy)))
            .await
            .expect("normalize legacy")
            .into_value();
        canonical["externalRoot"] = json!({"keep": true});
        write(
            &primary,
            &serde_json::to_vec_pretty(&canonical).expect("serialize canonical"),
        );
        transaction
            .replace_entry(
                "toolkit",
                json!({"source":"owner/new", "computedHash":"new"}),
            )
            .expect("queue replacement");

        transaction.commit().await.expect("merge canonical");

        let committed = read_json(&primary);
        assert_eq!(committed["externalRoot"]["keep"], true);
        assert_eq!(committed["skills"]["toolkit"]["source"], "owner/new");
    }

    #[tokio::test]
    async fn selected_entry_change_after_begin_returns_structured_conflict() {
        let temp = tempdir().expect("tempdir");
        let primary = temp.path().join("skills-lock.json");
        write(
            &primary,
            br#"{"version":1,"skills":{"toolkit":{"source":"old"}}}"#,
        );
        let repository = LockRepository::new(EnvironmentLockIo::Host);
        let mut transaction = repository
            .begin(project_target(&primary, None), entry_targets(&["toolkit"]))
            .await
            .expect("begin transaction");
        write(
            &primary,
            br#"{"version":1,"skills":{"toolkit":{"source":"external"}}}"#,
        );
        transaction
            .replace_entry("toolkit", json!({"source":"gui"}))
            .expect("queue replacement");

        assert!(matches!(
            transaction.commit().await,
            Err(AppError::LockConflict {
                target: LockConflictTarget::Skill { skill_name }
            }) if skill_name == "toolkit"
        ));
    }

    #[tokio::test]
    async fn unrelated_entry_and_root_changes_survive_commit() {
        let temp = tempdir().expect("tempdir");
        let primary = temp.path().join("skills-lock.json");
        write(
            &primary,
            br#"{"version":1,"skills":{"toolkit":{"source":"old"},"review":{"source":"before"}}}"#,
        );
        let repository = LockRepository::new(EnvironmentLockIo::Host);
        let mut transaction = repository
            .begin(project_target(&primary, None), entry_targets(&["toolkit"]))
            .await
            .expect("begin transaction");
        write(
            &primary,
            br#"{"version":1,"futureRoot":{"keep":true},"skills":{"toolkit":{"source":"old"},"review":{"source":"external"}}}"#,
        );
        transaction
            .replace_entry("toolkit", json!({"source":"gui"}))
            .expect("queue replacement");

        transaction.commit().await.expect("commit transaction");

        let committed = read_json(&primary);
        assert_eq!(committed["futureRoot"]["keep"], true);
        assert_eq!(committed["skills"]["review"]["source"], "external");
    }

    #[tokio::test]
    async fn agent_default_root_change_returns_structured_conflict() {
        let temp = tempdir().expect("tempdir");
        let primary = temp.path().join("skill-lock.json");
        write(
            &primary,
            br#"{"version":3,"defaultTargetAgents":{"global":["codex"],"project":[]},"lastSelectedAgents":["codex"],"skills":{}}"#,
        );
        let repository = LockRepository::new(EnvironmentLockIo::Host);
        let target = LockTarget {
            primary: locator(&primary),
            legacy: None,
            schema: LockSchema::Global,
        };
        let mut transaction = repository
            .begin(
                target,
                LockMutationTargets {
                    entries: Vec::new(),
                    default_target_agents: true,
                },
            )
            .await
            .expect("begin transaction");
        write(
            &primary,
            br#"{"version":3,"defaultTargetAgents":{"global":["claude-code"],"project":[]},"lastSelectedAgents":["claude-code"],"skills":{}}"#,
        );
        transaction
            .set_default_target_agents(json!({"global":["cursor"],"project":[]}), json!(["cursor"]))
            .expect("queue defaults");

        assert!(matches!(
            transaction.commit().await,
            Err(AppError::LockConflict {
                target: LockConflictTarget::RootField { field }
            }) if field == "defaultTargetAgents"
        ));
    }

    #[tokio::test]
    async fn one_commit_applies_multiple_entry_replacements() {
        let temp = tempdir().expect("tempdir");
        let primary = temp.path().join("skills-lock.json");
        write(
            &primary,
            br#"{"version":1,"skills":{"toolkit":{"source":"old-a"},"review":{"source":"old-b"}}}"#,
        );
        let repository = LockRepository::new(EnvironmentLockIo::Host);
        let mut transaction = repository
            .begin(
                project_target(&primary, None),
                entry_targets(&["toolkit", "review"]),
            )
            .await
            .expect("begin transaction");
        transaction
            .replace_entry("toolkit", json!({"source":"new-a"}))
            .expect("queue toolkit");
        transaction
            .replace_entry("review", json!({"source":"new-b"}))
            .expect("queue review");

        transaction.commit().await.expect("commit transaction");

        let committed = read_json(&primary);
        assert_eq!(committed["skills"]["toolkit"]["source"], "new-a");
        assert_eq!(committed["skills"]["review"]["source"], "new-b");
    }
}
