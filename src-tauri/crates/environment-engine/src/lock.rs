use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockSchema {
    Global,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryMutation {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockMutation {
    pub schema: LockSchema,
    pub entry: EntryMutation,
    pub root_replacements: BTreeMap<String, Value>,
    pub expected_entries: BTreeMap<String, Option<Value>>,
    pub expected_roots: BTreeMap<String, Option<Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockReceipt {
    pub entries: BTreeMap<String, Option<Value>>,
    pub roots: BTreeMap<String, Option<Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedLock {
    pub bytes: Vec<u8>,
    pub receipt: LockReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    InvalidDocument { message: String },
    UnsupportedSchema { version: u64, supported: u64 },
    MissingExpectedEntry { key: String },
    MissingExpectedRoot { field: String },
    EntryConflict { key: String },
    RootConflict { field: String },
}

impl fmt::Display for LockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LockError {}

pub fn apply(
    current: Option<&[u8]>,
    legacy: Option<&[u8]>,
    mutation: &LockMutation,
) -> Result<AppliedLock, LockError> {
    let mut document = load_document(current, legacy, mutation.schema)?;
    for key in affected_keys(&mutation.entry) {
        let expected =
            mutation
                .expected_entries
                .get(key)
                .ok_or_else(|| LockError::MissingExpectedEntry {
                    key: key.to_string(),
                })?;
        document.validate_entry(key, expected)?;
    }
    match &mutation.entry {
        EntryMutation::Replace { key, replacement } => {
            document.replace_entry(mutation.schema, key, replacement.clone())
        }
        EntryMutation::Remove { key } => document.remove_entry(key),
        EntryMutation::MoveAndReplace {
            from,
            to,
            replacement,
        } => document.move_and_replace(mutation.schema, from, to, replacement.clone()),
    }
    for (field, replacement) in &mutation.root_replacements {
        let expected =
            mutation
                .expected_roots
                .get(field)
                .ok_or_else(|| LockError::MissingExpectedRoot {
                    field: field.clone(),
                })?;
        document.replace_root(field, expected, replacement.clone())?;
    }
    let receipt = LockReceipt {
        entries: mutation
            .expected_entries
            .keys()
            .map(|key| (key.clone(), document.entry(key).cloned()))
            .collect(),
        roots: mutation
            .expected_roots
            .keys()
            .map(|field| (field.clone(), document.root.get(field).cloned()))
            .collect(),
    };
    let mut bytes = serde_json::to_vec_pretty(&document.root).map_err(json_error)?;
    bytes.push(b'\n');
    Ok(AppliedLock { bytes, receipt })
}

fn affected_keys(entry: &EntryMutation) -> Vec<&str> {
    match entry {
        EntryMutation::Replace { key, .. } | EntryMutation::Remove { key } => vec![key],
        EntryMutation::MoveAndReplace { from, to, .. } => vec![from, to],
    }
}

fn load_document(
    current: Option<&[u8]>,
    legacy: Option<&[u8]>,
    schema: LockSchema,
) -> Result<Document, LockError> {
    if let Some(bytes) = current {
        ensure_supported_schema(bytes, schema)?;
        return Document::parse(bytes);
    }
    let Some(bytes) = legacy else {
        return Ok(Document::empty(schema));
    };
    let document = Document::parse(bytes)?;
    match schema {
        LockSchema::Global => Ok(document),
        LockSchema::Project => convert_legacy_project(document),
    }
}

fn ensure_supported_schema(bytes: &[u8], schema: LockSchema) -> Result<(), LockError> {
    let value: Value = serde_json::from_slice(bytes).map_err(json_error)?;
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| LockError::InvalidDocument {
            message: "lock version is missing".to_string(),
        })?;
    let supported = schema_version(schema);
    if version > supported {
        Err(LockError::UnsupportedSchema { version, supported })
    } else {
        Ok(())
    }
}

struct Document {
    root: Value,
}

impl Document {
    fn parse(bytes: &[u8]) -> Result<Self, LockError> {
        let root: Value = serde_json::from_slice(bytes).map_err(json_error)?;
        let object = root.as_object().ok_or_else(|| LockError::InvalidDocument {
            message: "lock root must be a JSON object".to_string(),
        })?;
        if !object.get("skills").is_some_and(Value::is_object) {
            return Err(LockError::InvalidDocument {
                message: "lock skills must be a JSON object".to_string(),
            });
        }
        Ok(Self { root })
    }

    fn empty(schema: LockSchema) -> Self {
        Self {
            root: serde_json::json!({ "version": schema_version(schema), "skills": {} }),
        }
    }

    fn skills(&self) -> &Map<String, Value> {
        self.root["skills"]
            .as_object()
            .expect("validated lock skills")
    }

    fn skills_mut(&mut self) -> &mut Map<String, Value> {
        self.root["skills"]
            .as_object_mut()
            .expect("validated lock skills")
    }

    fn entry(&self, key: &str) -> Option<&Value> {
        self.skills().get(key)
    }

    fn validate_entry(&self, key: &str, expected: &Option<Value>) -> Result<(), LockError> {
        if self.entry(key) == expected.as_ref() {
            Ok(())
        } else {
            Err(LockError::EntryConflict {
                key: key.to_string(),
            })
        }
    }

    fn replace_entry(&mut self, schema: LockSchema, key: &str, replacement: Value) {
        let replacement = merge_entry_fields(schema, self.entry(key), replacement);
        self.skills_mut().insert(key.to_string(), replacement);
    }

    fn remove_entry(&mut self, key: &str) {
        self.skills_mut().remove(key);
    }

    fn move_and_replace(&mut self, schema: LockSchema, from: &str, to: &str, replacement: Value) {
        let replacement = merge_entry_fields(schema, self.entry(from), replacement);
        self.skills_mut().remove(from);
        self.skills_mut().insert(to.to_string(), replacement);
    }

    fn replace_root(
        &mut self,
        field: &str,
        expected: &Option<Value>,
        replacement: Value,
    ) -> Result<(), LockError> {
        if self.root.get(field) != expected.as_ref() {
            return Err(LockError::RootConflict {
                field: field.to_string(),
            });
        }
        self.root
            .as_object_mut()
            .expect("validated lock root")
            .insert(field.to_string(), replacement);
        Ok(())
    }
}

fn schema_version(schema: LockSchema) -> u64 {
    match schema {
        LockSchema::Global => 3,
        LockSchema::Project => 1,
    }
}

fn merge_entry_fields(schema: LockSchema, current: Option<&Value>, replacement: Value) -> Value {
    let known_fields: &[&str] = match schema {
        LockSchema::Global => &[
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
        ],
        LockSchema::Project => &[
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
        ],
    };
    match (current.and_then(Value::as_object), replacement) {
        (current, Value::Object(replacement)) => {
            let mut merged = current.cloned().unwrap_or_default();
            for field in known_fields {
                merged.remove(*field);
            }
            merged.extend(replacement);
            Value::Object(merged)
        }
        (_, replacement) => replacement,
    }
}

fn convert_legacy_project(document: Document) -> Result<Document, LockError> {
    let mut root =
        document
            .root
            .as_object()
            .cloned()
            .ok_or_else(|| LockError::InvalidDocument {
                message: "lock root must be a JSON object".to_string(),
            })?;
    root.insert("version".to_string(), Value::from(1));
    for field in ["dismissed", "lastSelectedAgents", "defaultTargetAgents"] {
        root.remove(field);
    }
    let skills = root
        .get_mut("skills")
        .and_then(Value::as_object_mut)
        .expect("validated lock skills");
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
    Ok(Document {
        root: Value::Object(root),
    })
}

fn json_error(error: serde_json::Error) -> LockError {
    LockError::InvalidDocument {
        message: error.to_string(),
    }
}
