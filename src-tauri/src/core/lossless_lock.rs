use serde_json::{Map, Value};

use crate::error::{AppError, LockConflictTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockSchema {
    Global,
    Project,
}

const GLOBAL_ENTRY_FIELDS: &[&str] = &[
    "source",
    "sourceType",
    "sourceUrl",
    "ref",
    "skillPath",
    "skillFolderHash",
    "installedAt",
    "updatedAt",
    "pluginName",
    "sourceBaseUrl",
    "wellKnownDigest",
];

const PROJECT_ENTRY_FIELDS: &[&str] = &[
    "source",
    "ref",
    "sourceType",
    "sourceUrl",
    "computedHash",
    "remoteHash",
    "skillPath",
    "subagents",
    "pluginName",
    "wellKnownDigest",
];

#[derive(Debug, Clone, PartialEq)]
pub struct LockEntrySnapshot(Option<Value>);

#[derive(Debug, Clone, PartialEq)]
pub struct LockRootSnapshot(Option<Value>);

impl LockEntrySnapshot {
    pub fn value(&self) -> Option<&Value> {
        self.0.as_ref()
    }
}

#[derive(Debug, Clone)]
pub struct LosslessLockDocument {
    root: Value,
}

impl LosslessLockDocument {
    pub fn parse(bytes: &[u8]) -> Result<Self, AppError> {
        let root: Value = serde_json::from_slice(bytes)?;
        let object = root.as_object().ok_or_else(|| AppError::Json {
            message: "lock root must be a JSON object".to_string(),
        })?;
        if !object.get("skills").is_some_and(Value::is_object) {
            return Err(AppError::Json {
                message: "lock skills must be a JSON object".to_string(),
            });
        }
        Ok(Self { root })
    }

    pub fn empty(schema: LockSchema) -> Self {
        let version = match schema {
            LockSchema::Global => 3,
            LockSchema::Project => 1,
        };
        Self {
            root: serde_json::json!({ "version": version, "skills": {} }),
        }
    }

    pub fn entry_snapshot(&self, skill_name: &str) -> LockEntrySnapshot {
        LockEntrySnapshot(self.skills().get(skill_name).cloned())
    }

    pub fn root_snapshot(&self, field: &str) -> LockRootSnapshot {
        LockRootSnapshot(self.root.get(field).cloned())
    }

    pub fn replace_entry(
        &mut self,
        schema: LockSchema,
        skill_name: &str,
        expected: &LockEntrySnapshot,
        replacement: Value,
    ) -> Result<(), AppError> {
        let current = self.skills().get(skill_name);
        if current != expected.0.as_ref() {
            return Err(AppError::LockConflict {
                target: LockConflictTarget::Skill {
                    skill_name: skill_name.to_string(),
                },
            });
        }
        let replacement = merge_entry_fields(schema, current, replacement);
        self.skills_mut()
            .insert(skill_name.to_string(), replacement);
        Ok(())
    }

    pub fn remove_entry(
        &mut self,
        skill_name: &str,
        expected: &LockEntrySnapshot,
    ) -> Result<(), AppError> {
        if self.skills().get(skill_name) != expected.0.as_ref() {
            return Err(AppError::LockConflict {
                target: LockConflictTarget::Skill {
                    skill_name: skill_name.to_string(),
                },
            });
        }
        self.skills_mut().remove(skill_name);
        Ok(())
    }

    pub fn move_and_replace_entry(
        &mut self,
        schema: LockSchema,
        from: &str,
        to: &str,
        expected_from: &LockEntrySnapshot,
        expected_to: &LockEntrySnapshot,
        replacement: Value,
    ) -> Result<(), AppError> {
        self.validate_entry_snapshot(from, expected_from)?;
        self.validate_entry_snapshot(to, expected_to)?;
        let replacement = merge_entry_fields(schema, self.skills().get(from), replacement);
        self.skills_mut().remove(from);
        self.skills_mut().insert(to.to_string(), replacement);
        Ok(())
    }

    pub fn validate_entry_snapshot(
        &self,
        skill_name: &str,
        expected: &LockEntrySnapshot,
    ) -> Result<(), AppError> {
        if self.skills().get(skill_name) != expected.0.as_ref() {
            return Err(AppError::LockConflict {
                target: LockConflictTarget::Skill {
                    skill_name: skill_name.to_string(),
                },
            });
        }
        Ok(())
    }

    pub fn replace_root(
        &mut self,
        field: &str,
        expected: &LockRootSnapshot,
        replacement: Value,
    ) -> Result<(), AppError> {
        if self.root.get(field) != expected.0.as_ref() {
            return Err(AppError::LockConflict {
                target: LockConflictTarget::RootField {
                    field: field.to_string(),
                },
            });
        }
        self.root
            .as_object_mut()
            .expect("validated lock root")
            .insert(field.to_string(), replacement);
        Ok(())
    }

    pub fn to_pretty_bytes(&self) -> Result<Vec<u8>, AppError> {
        let mut bytes = serde_json::to_vec_pretty(&self.root)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn into_value(self) -> Value {
        self.root
    }

    fn skills(&self) -> &Map<String, Value> {
        self.root["skills"].as_object().expect("validated skills")
    }

    fn skills_mut(&mut self) -> &mut Map<String, Value> {
        self.root["skills"]
            .as_object_mut()
            .expect("validated skills")
    }
}

fn merge_entry_fields(schema: LockSchema, current: Option<&Value>, replacement: Value) -> Value {
    match (current.and_then(Value::as_object), replacement) {
        (current, Value::Object(replacement)) => {
            let mut merged = current.cloned().unwrap_or_default();
            let known_fields = match schema {
                LockSchema::Global => GLOBAL_ENTRY_FIELDS,
                LockSchema::Project => PROJECT_ENTRY_FIELDS,
            };
            for field in known_fields {
                merged.remove(*field);
            }
            merged.extend(replacement);
            Value::Object(merged)
        }
        (_, replacement) => replacement,
    }
}

pub fn convert_legacy_project_document(
    document: LosslessLockDocument,
) -> Result<LosslessLockDocument, AppError> {
    let mut root = document
        .root
        .as_object()
        .cloned()
        .ok_or_else(|| AppError::Json {
            message: "lock root must be a JSON object".to_string(),
        })?;
    root.insert("version".to_string(), Value::from(1));
    for field in ["dismissed", "lastSelectedAgents", "defaultTargetAgents"] {
        root.remove(field);
    }
    let skills = root
        .get_mut("skills")
        .and_then(Value::as_object_mut)
        .expect("validated skills");
    for entry in skills.values_mut() {
        let Some(current) = entry.as_object().cloned() else {
            continue;
        };
        let mut replacement = Map::new();
        for field in [
            "source",
            "ref",
            "sourceType",
            "sourceUrl",
            "skillPath",
            "pluginName",
        ] {
            if let Some(value) = current.get(field) {
                replacement.insert(field.to_string(), value.clone());
            }
        }
        replacement.insert("computedHash".to_string(), Value::String(String::new()));
        if let Some(remote_hash) = current
            .get("skillFolderHash")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            replacement.insert(
                "remoteHash".to_string(),
                Value::String(remote_hash.to_string()),
            );
        }
        *entry = merge_entry_fields(
            LockSchema::Global,
            Some(&Value::Object(current)),
            Value::Object(replacement),
        );
    }
    Ok(LosslessLockDocument {
        root: Value::Object(root),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{convert_legacy_project_document, LockSchema, LosslessLockDocument};

    #[test]
    fn global_replacement_clears_known_optional_fields_and_keeps_future_data() {
        let mut document = LosslessLockDocument::parse(include_bytes!(
            "../../tests/fixtures/locks/cli-global-v3-future.json"
        ))
        .expect("parse global fixture");
        let snapshot = document.entry_snapshot("toolkit");

        document
            .replace_entry(
                LockSchema::Global,
                "toolkit",
                &snapshot,
                json!({
                    "source": "owner/new-repo",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/owner/new-repo",
                    "skillFolderHash": "new-hash",
                    "installedAt": "2026-01-01T00:00:00.000Z",
                    "updatedAt": "2026-07-15T00:00:00.000Z"
                }),
            )
            .expect("replace global entry");

        let value = document.into_value();
        assert!(value["skills"]["toolkit"].get("pluginName").is_none());
        assert_eq!(value["skills"]["toolkit"]["cliOnlyFlag"], true);
        assert_eq!(value["skills"]["toolkit"]["futureEntry"]["revision"], 3);
        assert_eq!(value["skills"]["review"]["futureEntry"]["keep"], true);
        assert_eq!(value["futureRoot"]["schema"], 7);
        assert_eq!(value["futureArray"][1]["nested"], true);
    }

    #[test]
    fn project_replacement_clears_known_optional_fields_and_keeps_future_data() {
        let mut document = LosslessLockDocument::parse(include_bytes!(
            "../../tests/fixtures/locks/cli-project-v1-future.json"
        ))
        .expect("parse project fixture");
        let snapshot = document.entry_snapshot("toolkit");

        document
            .replace_entry(
                LockSchema::Project,
                "toolkit",
                &snapshot,
                json!({
                    "source": "owner/new-repo",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/owner/new-repo",
                    "computedHash": "new-computed-hash",
                    "skillPath": "skills/toolkit"
                }),
            )
            .expect("replace project entry");

        let value = document.into_value();
        let entry = &value["skills"]["toolkit"];
        assert!(entry.get("remoteHash").is_none());
        assert!(entry.get("subagents").is_none());
        assert!(entry.get("pluginName").is_none());
        assert_eq!(entry["cliOnlyFlag"], true);
        assert_eq!(entry["futureEntry"]["revision"], 4);
        assert_eq!(value["skills"]["review"]["futureEntry"]["keep"], true);
        assert_eq!(value["futureRoot"]["schema"], 8);
    }

    #[test]
    fn cli_v1_5_22_well_known_fields_are_known_and_unknown_fields_remain_lossless() {
        let cases = [
            (
                LockSchema::Global,
                include_bytes!("../../tests/fixtures/locks/cli-v1.5.22-global-well-known.json")
                    .as_slice(),
            ),
            (
                LockSchema::Project,
                include_bytes!("../../tests/fixtures/locks/cli-v1.5.22-project-well-known.json")
                    .as_slice(),
            ),
        ];

        for (schema, bytes) in cases {
            let mut document = LosslessLockDocument::parse(bytes).expect("parse CLI fixture");
            let snapshot = document.entry_snapshot("ce:review");
            document
                .replace_entry(
                    schema,
                    "ce:review",
                    &snapshot,
                    json!({
                        "source": "owner/repo",
                        "sourceType": "github",
                        "sourceUrl": "https://github.com/owner/repo",
                        "skillPath": "skills/review"
                    }),
                )
                .expect("replace Well-known entry");

            let value = document.into_value();
            let entry = &value["skills"]["ce:review"];
            assert!(entry.get("sourceBaseUrl").is_none());
            assert!(entry.get("wellKnownDigest").is_none());
            assert_eq!(entry["futureEntry"]["keep"], true);
        }
    }

    #[test]
    fn legacy_project_conversion_maps_known_fields_without_losing_future_data() {
        let bytes = include_bytes!("../../tests/fixtures/locks/cli-legacy-project-v3-future.json");
        let document = LosslessLockDocument::parse(bytes).expect("parse legacy fixture");

        let converted = convert_legacy_project_document(document)
            .expect("convert legacy project document")
            .into_value();

        assert_eq!(converted["version"], 1);
        assert!(converted.get("dismissed").is_none());
        assert!(converted.get("lastSelectedAgents").is_none());
        assert!(converted.get("defaultTargetAgents").is_none());
        assert_eq!(converted["futureRoot"]["schema"], 9);
        let entry = &converted["skills"]["toolkit"];
        assert_eq!(entry["source"], "owner/old-repo");
        assert_eq!(entry["ref"], "legacy");
        assert_eq!(entry["computedHash"], "");
        assert_eq!(entry["remoteHash"], "old-remote-hash");
        assert_eq!(entry["pluginName"], "legacy-plugin");
        assert!(entry.get("installedAt").is_none());
        assert!(entry.get("updatedAt").is_none());
        assert!(entry.get("skillFolderHash").is_none());
        assert_eq!(entry["cliOnlyFlag"], true);
        assert_eq!(entry["futureEntry"]["revision"], 5);
        assert_eq!(converted["skills"]["review"]["futureEntry"]["keep"], true);

        let original: serde_json::Value = serde_json::from_slice(bytes).expect("parse original");
        assert_eq!(original["version"], 3);
        assert_eq!(
            original["skills"]["toolkit"]["installedAt"],
            "2026-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn updates_target_entry_without_losing_unknown_fields() {
        let mut document = LosslessLockDocument::parse(
            br#"{
              "version": 1,
              "futureRoot": {"enabled": true},
              "skills": {
                "toolkit": {"source":"old","futureEntry":42},
                "review": {"source":"keep"}
              }
            }"#,
        )
        .expect("parse lock");
        let snapshot = document.entry_snapshot("toolkit");

        document
            .replace_entry(
                LockSchema::Project,
                "toolkit",
                &snapshot,
                json!({"source":"new","futureEntry":42}),
            )
            .expect("replace entry");

        let value = document.into_value();
        assert_eq!(value["futureRoot"]["enabled"], true);
        assert_eq!(value["skills"]["toolkit"]["source"], "new");
        assert_eq!(value["skills"]["toolkit"]["futureEntry"], 42);
        assert_eq!(value["skills"]["review"]["source"], "keep");
    }

    #[test]
    fn rejects_replace_when_target_entry_changed_after_snapshot() {
        let mut document =
            LosslessLockDocument::parse(br#"{"version":1,"skills":{"toolkit":{"source":"old"}}}"#)
                .expect("parse lock");
        let stale = document.entry_snapshot("toolkit");
        document
            .replace_entry(
                LockSchema::Project,
                "toolkit",
                &stale,
                json!({"source":"external"}),
            )
            .expect("external update");

        assert!(document
            .replace_entry(
                LockSchema::Project,
                "toolkit",
                &stale,
                json!({"source":"gui"}),
            )
            .is_err());
    }

    #[test]
    fn rejects_non_object_root_or_skills() {
        assert!(LosslessLockDocument::parse(br#"[]"#).is_err());
        assert!(LosslessLockDocument::parse(br#"{"skills":[]}"#).is_err());
    }
}
