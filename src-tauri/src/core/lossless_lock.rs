use serde_json::{Map, Value};

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq)]
pub struct LockEntrySnapshot(Option<Value>);

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

    pub fn empty() -> Self {
        Self {
            root: Value::Object(Map::from_iter([(
                "skills".to_string(),
                Value::Object(Map::new()),
            )])),
        }
    }

    pub fn snapshot(&self, skill_name: &str) -> LockEntrySnapshot {
        LockEntrySnapshot(self.skills().get(skill_name).cloned())
    }

    pub fn replace_entry(
        &mut self,
        skill_name: &str,
        expected: &LockEntrySnapshot,
        replacement: Value,
    ) -> Result<(), AppError> {
        if self.skills().get(skill_name) != expected.0.as_ref() {
            return Err(AppError::Custom {
                message: format!("lock entry '{skill_name}' changed externally"),
            });
        }
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
            return Err(AppError::Custom {
                message: format!("lock entry '{skill_name}' changed externally"),
            });
        }
        self.skills_mut().remove(skill_name);
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::LosslessLockDocument;

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
        let snapshot = document.snapshot("toolkit");

        document
            .replace_entry(
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
        let stale = document.snapshot("toolkit");
        document
            .replace_entry("toolkit", &stale, json!({"source":"external"}))
            .expect("external update");

        assert!(document
            .replace_entry("toolkit", &stale, json!({"source":"gui"}))
            .is_err());
    }

    #[test]
    fn rejects_non_object_root_or_skills() {
        assert!(LosslessLockDocument::parse(br#"[]"#).is_err());
        assert!(LosslessLockDocument::parse(br#"{"skills":[]}"#).is_err());
    }
}
