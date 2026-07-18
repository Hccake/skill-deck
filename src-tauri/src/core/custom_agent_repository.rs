use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tempfile::NamedTempFile;

use super::agent_definition::{AgentFieldError, AgentId, CustomAgentDefinition};
use super::agent_settings::CustomAgentRecord;
use super::app_config::get_config_path;
use crate::error::AppError;

pub const CUSTOM_AGENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomAgentFile {
    pub schema_version: u32,
    pub records: Vec<CustomAgentRecord>,
    pub root_extensions: Map<String, Value>,
}

impl Default for CustomAgentFile {
    fn default() -> Self {
        Self {
            schema_version: CUSTOM_AGENT_SCHEMA_VERSION,
            records: Vec::new(),
            root_extensions: Map::new(),
        }
    }
}

impl CustomAgentFile {
    #[cfg(test)]
    pub fn valid_records(&self) -> impl Iterator<Item = (&CustomAgentDefinition, &Value)> {
        self.records.iter().filter_map(|record| match record {
            CustomAgentRecord::Valid { definition, raw } => Some((definition, raw)),
            _ => None,
        })
    }

    #[cfg(test)]
    pub fn invalid_records(&self) -> impl Iterator<Item = (usize, &Value, &[AgentFieldError])> {
        self.records.iter().filter_map(|record| match record {
            CustomAgentRecord::Invalid { index, raw, errors } => {
                Some((*index, raw, errors.as_slice()))
            }
            _ => None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CustomAgentRepository {
    path: PathBuf,
}

impl CustomAgentRepository {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn for_current_user() -> Result<Self, AppError> {
        let config_path = get_config_path()?;
        Ok(Self::new(config_path.with_file_name("custom-agents.json")))
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn backup_path(&self) -> PathBuf {
        self.path.with_extension("json.bak")
    }

    pub fn load(&self) -> Result<CustomAgentFile, AppError> {
        match read_file(&self.path) {
            Ok(file) => Ok(file),
            Err(ReadFailure::UnsupportedSchema(version)) => Err(unsupported_schema_error(version)),
            Err(ReadFailure::Io(error)) => Err(error),
            Err(ReadFailure::Missing) => match read_file(&self.backup_path()) {
                Ok(file) => Ok(file),
                Err(ReadFailure::UnsupportedSchema(version)) => {
                    Err(unsupported_schema_error(version))
                }
                Err(_) => Ok(CustomAgentFile::default()),
            },
            Err(ReadFailure::Corrupt(primary_error)) => match read_file(&self.backup_path()) {
                Ok(file) => Ok(file),
                Err(ReadFailure::UnsupportedSchema(version)) => {
                    Err(unsupported_schema_error(version))
                }
                Err(_) => Err(primary_error),
            },
        }
    }

    pub fn save(&self, file: &CustomAgentFile) -> Result<(), AppError> {
        if file.schema_version != CUSTOM_AGENT_SCHEMA_VERSION {
            return Err(unsupported_schema_error(file.schema_version as u64));
        }
        validate_records(&file.records)?;

        match fs::read(&self.path) {
            Ok(existing) => match parse_file(&existing) {
                Ok(_) => write_atomic(&self.backup_path(), &existing)?,
                Err(ReadFailure::UnsupportedSchema(version)) => {
                    return Err(unsupported_schema_error(version));
                }
                Err(ReadFailure::Io(error)) => return Err(error),
                Err(ReadFailure::Missing | ReadFailure::Corrupt(_)) => {}
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        let bytes = serialize_file(file)?;
        write_atomic(&self.path, &bytes)
    }

    pub fn upsert(&self, definition: CustomAgentDefinition) -> Result<CustomAgentFile, AppError> {
        definition.validate().map_err(invalid_definition_error)?;
        let id = definition.id.clone();
        let canonical_raw = serde_json::to_value(&definition)?;
        let mut file = self.load()?;

        if let Some(record) = file
            .records
            .iter_mut()
            .find(|record| record_definition(record).is_some_and(|definition| definition.id == id))
        {
            let existing_raw = match record {
                CustomAgentRecord::Valid { raw, .. } => raw,
                CustomAgentRecord::Invalid { .. } => {
                    unreachable!("record_definition only matches persisted valid records")
                }
            };
            let raw = merge_updated_definition_raw(existing_raw, canonical_raw)?;
            *record = CustomAgentRecord::Valid { definition, raw };
        } else {
            file.records.push(CustomAgentRecord::Valid {
                definition,
                raw: canonical_raw,
            });
        }

        self.save(&file)?;
        Ok(file)
    }

    pub fn delete(&self, id: &AgentId) -> Result<CustomAgentFile, AppError> {
        let mut file = self.load()?;
        let previous_len = file.records.len();
        file.records
            .retain(|record| match record_definition(record) {
                Some(definition) => &definition.id != id,
                None => true,
            });
        if file.records.len() != previous_len {
            refresh_invalid_indices(&mut file.records);
            self.save(&file)?;
        } else {
            return Err(AppError::InvalidAgent {
                agent: id.to_string(),
            });
        }
        Ok(file)
    }

    pub fn delete_invalid(&self, index: usize) -> Result<CustomAgentFile, AppError> {
        let mut file = self.load()?;
        if !matches!(
            file.records.get(index),
            Some(CustomAgentRecord::Invalid { .. })
        ) {
            return Err(AppError::Json {
                message: format!("custom agent record at index {index} is not invalid"),
            });
        }
        file.records.remove(index);
        refresh_invalid_indices(&mut file.records);
        self.save(&file)?;
        Ok(file)
    }

    pub fn duplicate_draft(
        &self,
        source_id: &AgentId,
        new_id: AgentId,
    ) -> Result<CustomAgentDefinition, AppError> {
        let file = self.load()?;
        let mut definition = file
            .records
            .iter()
            .find_map(|record| {
                record_definition(record).filter(|definition| &definition.id == source_id)
            })
            .cloned()
            .ok_or_else(|| AppError::InvalidAgent {
                agent: source_id.to_string(),
            })?;
        definition.id = new_id;
        definition.validate().map_err(invalid_definition_error)?;
        Ok(definition)
    }
}

fn record_definition(record: &CustomAgentRecord) -> Option<&CustomAgentDefinition> {
    match record {
        CustomAgentRecord::Valid { definition, .. } => Some(definition),
        CustomAgentRecord::Invalid { .. } => None,
    }
}

/// Merge a typed update into its previous raw form using the persisted schema's
/// ownership boundaries. Unknown keys survive at the definition, scope, and
/// path-object levels while schema-owned keys are replaced from `canonical`.
fn merge_updated_definition_raw(existing: &Value, canonical: Value) -> Result<Value, AppError> {
    let mut merged = existing
        .as_object()
        .cloned()
        .ok_or_else(|| AppError::Json {
            message: "valid custom agent raw record must be an object".to_string(),
        })?;
    let canonical = canonical.as_object().ok_or_else(|| AppError::Json {
        message: "serialized custom agent definition must be an object".to_string(),
    })?;

    replace_known_field(&mut merged, canonical, "id");
    replace_known_field(&mut merged, canonical, "displayName");
    merged.insert(
        "global".to_string(),
        merge_scope_value(merged.get("global"), canonical.get("global"), "global")?,
    );
    merged.insert(
        "project".to_string(),
        merge_scope_value(merged.get("project"), canonical.get("project"), "project")?,
    );
    merged.insert(
        "detectionPaths".to_string(),
        merge_detection_paths(
            merged.get("detectionPaths"),
            canonical.get("detectionPaths"),
        )?,
    );
    Ok(Value::Object(merged))
}

fn merge_scope_value(
    existing: Option<&Value>,
    canonical: Option<&Value>,
    field: &str,
) -> Result<Value, AppError> {
    let canonical = canonical
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Json {
            message: format!("serialized custom agent {field} scope must be an object"),
        })?;
    let mut merged = existing
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    replace_known_field(&mut merged, canonical, "enabled");
    replace_known_field(&mut merged, canonical, "location");
    match canonical.get("privatePath") {
        Some(Value::Null) => {
            merged.insert("privatePath".to_string(), Value::Null);
        }
        Some(canonical_path) => {
            let path = merge_path_value(merged.get("privatePath"), canonical_path)?;
            merged.insert("privatePath".to_string(), path);
        }
        None => {
            merged.remove("privatePath");
        }
    }
    Ok(Value::Object(merged))
}

fn merge_path_value(existing: Option<&Value>, canonical: &Value) -> Result<Value, AppError> {
    const KNOWN_PATH_FIELDS: [&str; 4] = ["kind", "base", "relativePath", "path"];

    let canonical = canonical.as_object().ok_or_else(|| AppError::Json {
        message: "serialized custom agent path must be an object".to_string(),
    })?;
    let mut merged = existing
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    for field in KNOWN_PATH_FIELDS {
        merged.remove(field);
    }
    for field in KNOWN_PATH_FIELDS {
        if let Some(value) = canonical.get(field) {
            merged.insert(field.to_string(), value.clone());
        }
    }
    Ok(Value::Object(merged))
}

/// Detection-path extensions are positional: old/new elements at the same
/// index are merged as path objects. Removed indices are truncated, while newly
/// appended indices are serialized canonically without inheriting old metadata.
fn merge_detection_paths(
    existing: Option<&Value>,
    canonical: Option<&Value>,
) -> Result<Value, AppError> {
    let canonical = canonical
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Json {
            message: "serialized custom agent detectionPaths must be an array".to_string(),
        })?;
    let existing = existing.and_then(Value::as_array);
    let merged = canonical
        .iter()
        .enumerate()
        .map(
            |(index, canonical_path)| match existing.and_then(|paths| paths.get(index)) {
                Some(existing_path) => merge_path_value(Some(existing_path), canonical_path),
                None => Ok(canonical_path.clone()),
            },
        )
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(Value::Array(merged))
}

fn replace_known_field(target: &mut Map<String, Value>, source: &Map<String, Value>, field: &str) {
    match source.get(field) {
        Some(value) => {
            target.insert(field.to_string(), value.clone());
        }
        None => {
            target.remove(field);
        }
    }
}

fn refresh_invalid_indices(records: &mut [CustomAgentRecord]) {
    for (index, record) in records.iter_mut().enumerate() {
        if let CustomAgentRecord::Invalid {
            index: record_index,
            ..
        } = record
        {
            *record_index = index;
        }
    }
}

fn validate_records(records: &[CustomAgentRecord]) -> Result<(), AppError> {
    for record in records {
        match record {
            CustomAgentRecord::Valid { definition, .. } => {
                definition.validate().map_err(invalid_definition_error)?;
            }
            CustomAgentRecord::Invalid { .. } => {}
        }
    }
    Ok(())
}

fn invalid_definition_error(error: AgentFieldError) -> AppError {
    AppError::Custom {
        message: format!("invalid custom agent definition: {error}"),
    }
}

fn unsupported_schema_error(version: u64) -> AppError {
    let _ = version;
    AppError::ConfigurationReadOnly
}

fn serialize_file(file: &CustomAgentFile) -> Result<Vec<u8>, AppError> {
    let agents = file
        .records
        .iter()
        .map(|record| match record {
            CustomAgentRecord::Valid { raw, .. } | CustomAgentRecord::Invalid { raw, .. } => {
                Ok(raw.clone())
            }
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let mut root = file.root_extensions.clone();
    root.insert(
        "schemaVersion".to_string(),
        Value::from(file.schema_version),
    );
    root.insert("agents".to_string(), Value::Array(agents));
    let mut bytes = serde_json::to_vec_pretty(&Value::Object(root))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn read_file(path: &Path) -> Result<CustomAgentFile, ReadFailure> {
    match fs::read(path) {
        Ok(bytes) => parse_file(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(ReadFailure::Missing),
        Err(error) => Err(ReadFailure::Io(error.into())),
    }
}

fn parse_file(bytes: &[u8]) -> Result<CustomAgentFile, ReadFailure> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|error| ReadFailure::Corrupt(error.into()))?;
    let object = root.as_object().ok_or_else(|| {
        ReadFailure::Corrupt(AppError::Json {
            message: "custom agent repository root must be an object".to_string(),
        })
    })?;
    let schema_version = object
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            ReadFailure::Corrupt(AppError::Json {
                message: "custom agent repository schemaVersion must be an integer".to_string(),
            })
        })?;
    if schema_version != CUSTOM_AGENT_SCHEMA_VERSION as u64 {
        return Err(ReadFailure::UnsupportedSchema(schema_version));
    }
    let agents = object
        .get("agents")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ReadFailure::Corrupt(AppError::Json {
                message: "custom agent repository agents must be an array".to_string(),
            })
        })?;

    let records = agents
        .iter()
        .enumerate()
        .map(|(index, raw)| parse_record(index, raw.clone()))
        .collect();
    Ok(CustomAgentFile {
        schema_version: schema_version as u32,
        records,
        root_extensions: object
            .iter()
            .filter(|(key, _)| key.as_str() != "schemaVersion" && key.as_str() != "agents")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    })
}

fn parse_record(index: usize, raw: Value) -> CustomAgentRecord {
    match serde_json::from_value::<CustomAgentDefinition>(raw.clone()) {
        Ok(definition) => match definition.validate() {
            Ok(()) => CustomAgentRecord::Valid { definition, raw },
            Err(error) => CustomAgentRecord::Invalid {
                index,
                raw,
                errors: vec![error],
            },
        },
        Err(error) => CustomAgentRecord::Invalid {
            index,
            raw,
            errors: vec![deserialization_error(&error)],
        },
    }
}

fn deserialization_error(error: &serde_json::Error) -> AgentFieldError {
    let message = error.to_string();
    let field = message
        .strip_prefix("missing field `")
        .and_then(|rest| rest.split('`').next())
        .unwrap_or("record");
    AgentFieldError::new(field, "invalidDefinition")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

enum ReadFailure {
    Missing,
    UnsupportedSchema(u64),
    Corrupt(AppError),
    Io(AppError),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use serde_json::{json, Value};
    use tempfile::{tempdir, TempDir};

    use super::*;
    use crate::core::agent_definition::{
        AgentId, CustomAgentDefinition, CustomPathBase, CustomPathSpec, CustomScopeDefinition,
        ScopeLocation,
    };
    use crate::core::agent_settings::CustomAgentRecord;
    use crate::core::app_config::get_config_path;
    use crate::error::AppError;

    fn definition(id: &str) -> CustomAgentDefinition {
        CustomAgentDefinition {
            id: AgentId::parse(id).expect("valid ID"),
            display_name: format!("{id} display"),
            global: CustomScopeDefinition {
                enabled: true,
                location: ScopeLocation::Shared,
                private_path: None,
            },
            project: CustomScopeDefinition {
                enabled: false,
                location: ScopeLocation::Shared,
                private_path: None,
            },
            detection_paths: vec![CustomPathSpec::based(
                CustomPathBase::Home,
                format!(".{id}"),
            )],
        }
    }

    fn raw_definition(id: &str) -> Value {
        serde_json::to_value(definition(id)).expect("serialize definition")
    }

    fn file(records: Vec<CustomAgentRecord>) -> CustomAgentFile {
        CustomAgentFile {
            schema_version: CUSTOM_AGENT_SCHEMA_VERSION,
            records,
            root_extensions: Map::new(),
        }
    }

    fn repository_with_value(value: Value) -> (TempDir, CustomAgentRepository) {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("custom-agents.json");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&value).expect("serialize fixture"),
        )
        .expect("write fixture");
        (temp, CustomAgentRepository::new(path))
    }

    #[test]
    fn missing_file_returns_empty_current_schema() {
        let temp = tempdir().expect("tempdir");
        let repository = CustomAgentRepository::new(temp.path().join("custom-agents.json"));

        let loaded = repository.load().expect("load missing repository");

        assert_eq!(loaded.schema_version, CUSTOM_AGENT_SCHEMA_VERSION);
        assert!(loaded.records.is_empty());
    }

    #[test]
    fn repository_for_current_user_sits_beside_app_config() {
        let repository = CustomAgentRepository::for_current_user().expect("repository path");
        let config_path = get_config_path().expect("config path");

        assert_eq!(repository.path().parent(), config_path.parent());
        assert_eq!(
            repository.path().file_name().and_then(|name| name.to_str()),
            Some("custom-agents.json")
        );
    }

    #[test]
    fn valid_roundtrip_preserves_raw_unknown_fields() {
        let temp = tempdir().expect("tempdir");
        let repository = CustomAgentRepository::new(temp.path().join("custom-agents.json"));
        let definition = definition("roundtrip-agent");
        let mut raw = serde_json::to_value(&definition).expect("serialize definition");
        raw.as_object_mut()
            .expect("definition object")
            .insert("futureField".to_string(), json!({ "keep": true }));
        let expected = file(vec![CustomAgentRecord::Valid {
            definition,
            raw: raw.clone(),
        }]);

        repository.save(&expected).expect("save repository");
        let loaded = repository.load().expect("load repository");

        assert_eq!(loaded, expected);
        assert_eq!(loaded.valid_records().count(), 1);
        assert_eq!(loaded.valid_records().next().unwrap().1, &raw);
    }

    #[test]
    fn one_invalid_record_does_not_hide_valid_records() {
        let valid = raw_definition("ok-agent");
        let invalid = json!({ "id": "bad id" });
        let (_temp, repository) = repository_with_value(json!({
            "schemaVersion": 1,
            "agents": [valid, invalid]
        }));

        let loaded = repository.load().expect("load mixed records");

        assert_eq!(loaded.valid_records().count(), 1);
        assert_eq!(loaded.invalid_records().count(), 1);
        assert_eq!(loaded.records.len(), 2);
        assert!(matches!(
            &loaded.records[1],
            CustomAgentRecord::Invalid { index: 1, raw, .. }
                if raw == &json!({ "id": "bad id" })
        ));
    }

    #[test]
    fn future_schema_is_read_only_and_cannot_be_overwritten() {
        let value = json!({
            "schemaVersion": CUSTOM_AGENT_SCHEMA_VERSION + 1,
            "agents": []
        });
        let (temp, repository) = repository_with_value(value);
        let path = temp.path().join("custom-agents.json");
        let before = fs::read(&path).expect("read future schema fixture");

        let load_error = repository.load().expect_err("future schema must fail");
        assert_eq!(load_error, AppError::ConfigurationReadOnly);
        let save_error = repository
            .upsert(definition("new-agent"))
            .expect_err("future schema must block mutation");
        assert_eq!(save_error, AppError::ConfigurationReadOnly);
        assert_eq!(fs::read(path).expect("read unchanged fixture"), before);
    }

    #[test]
    fn delete_invalid_record_preserves_all_other_raw_records() {
        let before = json!({
            "schemaVersion": 1,
            "agents": [raw_definition("ok-agent"), { "broken": true }, { "future": 7 }]
        });
        let (_temp, repository) = repository_with_value(before);

        let after = repository.delete_invalid(1).expect("delete invalid record");

        assert_eq!(after.records.len(), 2);
        assert!(matches!(
            &after.records[1],
            CustomAgentRecord::Invalid { index: 1, raw, .. }
                if *raw == json!({ "future": 7 })
        ));
    }

    #[test]
    fn delete_invalid_preserves_same_schema_root_extensions() {
        let (_temp, repository) = repository_with_value(json!({
            "schemaVersion": 1,
            "rootExtension": { "keep": [1, 2, 3] },
            "agents": [raw_definition("ok-agent"), { "broken": true }]
        }));

        repository.delete_invalid(1).expect("delete invalid record");

        let stored: Value =
            serde_json::from_slice(&fs::read(repository.path()).expect("read stored repository"))
                .expect("parse stored repository");
        assert_eq!(stored["rootExtension"], json!({ "keep": [1, 2, 3] }));
    }

    #[test]
    fn delete_invalid_rejects_future_schema_without_modifying_bytes() {
        let value = json!({
            "schemaVersion": CUSTOM_AGENT_SCHEMA_VERSION + 1,
            "agents": [{ "broken": true }]
        });
        let (temp, repository) = repository_with_value(value);
        let path = temp.path().join("custom-agents.json");
        let before = fs::read(&path).expect("read future schema fixture");

        let error = repository
            .delete_invalid(0)
            .expect_err("future schema must remain read-only");

        assert_eq!(error, AppError::ConfigurationReadOnly);
        assert_eq!(fs::read(path).expect("read unchanged fixture"), before);
    }

    #[test]
    fn corrupt_primary_recovers_the_latest_valid_backup() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("custom-agents.json");
        let repository = CustomAgentRepository::new(path.clone());
        let first = file(vec![CustomAgentRecord::valid(definition("first-agent"))]);
        let second = file(vec![CustomAgentRecord::valid(definition("second-agent"))]);
        repository.save(&first).expect("save first version");
        repository.save(&second).expect("save second version");
        fs::write(&path, b"{broken").expect("corrupt primary");

        let recovered = repository.load().expect("recover backup");

        assert_eq!(recovered, first);
    }

    #[test]
    fn atomic_write_replaces_the_primary_and_retains_one_backup() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("custom-agents.json");
        let repository = CustomAgentRepository::new(path.clone());
        let first = file(vec![CustomAgentRecord::valid(definition("first-agent"))]);
        let second = file(vec![CustomAgentRecord::valid(definition("second-agent"))]);

        repository.save(&first).expect("save first version");
        repository.save(&second).expect("save second version");

        assert_eq!(repository.load().expect("load primary"), second);
        let backup_repository = CustomAgentRepository::new(repository.backup_path());
        assert_eq!(backup_repository.load().expect("load backup"), first);
        let names = fs::read_dir(temp.path())
            .expect("read repository directory")
            .map(|entry| {
                entry
                    .expect("directory entry")
                    .file_name()
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            names,
            BTreeSet::from([
                "custom-agents.json".to_string(),
                "custom-agents.json.bak".to_string(),
            ])
        );
    }

    #[test]
    fn detection_and_global_private_paths_allow_absolute_values() {
        let (_temp, repository) = repository_with_value(json!({
            "schemaVersion": 1,
            "agents": [{
                "id": "absolute-agent",
                "displayName": "Absolute Agent",
                "global": {
                    "enabled": true,
                    "location": "private",
                    "privatePath": { "kind": "absolute", "path": "/opt/agent/skills" }
                },
                "project": { "enabled": false, "location": "shared", "privatePath": null },
                "detectionPaths": [
                    { "kind": "absolute", "path": "/opt/agent" }
                ]
            }]
        }));

        let loaded = repository.load().expect("load absolute paths");

        assert_eq!(loaded.valid_records().count(), 1);
        assert_eq!(loaded.invalid_records().count(), 0);
    }

    #[test]
    fn project_private_path_rejects_absolute_values() {
        let (_temp, repository) = repository_with_value(json!({
            "schemaVersion": 1,
            "agents": [{
                "id": "absolute-project-agent",
                "displayName": "Absolute Project Agent",
                "global": { "enabled": false, "location": "shared", "privatePath": null },
                "project": {
                    "enabled": true,
                    "location": "private",
                    "privatePath": { "kind": "absolute", "path": "/opt/project/skills" }
                },
                "detectionPaths": [
                    { "kind": "based", "base": "home", "relativePath": ".agent" }
                ]
            }]
        }));

        let loaded = repository.load().expect("load invalid absolute path");

        assert_eq!(loaded.valid_records().count(), 0);
        let invalid = loaded.invalid_records().next().expect("invalid record");
        assert_eq!(invalid.2[0].field, "project.privatePath.kind");
        assert_eq!(invalid.2[0].code, "absolutePathNotAllowed");
    }

    #[test]
    fn upsert_preserves_order_and_unrelated_invalid_raw_records() {
        let invalid = json!({ "id": "bad id", "futureField": ["keep"] });
        let (_temp, repository) = repository_with_value(json!({
            "schemaVersion": 1,
            "agents": [raw_definition("first-agent"), invalid.clone(), raw_definition("last-agent")]
        }));
        let mut updated = definition("first-agent");
        updated.display_name = "Updated".to_string();

        let saved = repository.upsert(updated).expect("upsert definition");

        assert_eq!(saved.records.len(), 3);
        assert!(matches!(
            &saved.records[1],
            CustomAgentRecord::Invalid { index: 1, raw, .. } if raw == &invalid
        ));
        assert_eq!(
            saved.valid_records().next().unwrap().0.display_name,
            "Updated"
        );
        let stored: Value =
            serde_json::from_slice(&fs::read(repository.path()).expect("read stored repository"))
                .expect("parse stored repository");
        assert_eq!(stored["agents"][1], invalid);
    }

    #[test]
    fn upsert_preserves_top_level_extensions_while_replacing_known_fields() {
        let mut original = raw_definition("update-agent");
        original
            .as_object_mut()
            .unwrap()
            .insert("futureField".to_string(), json!({ "keep": [1, 2, 3] }));
        let (_temp, repository) = repository_with_value(json!({
            "schemaVersion": 1,
            "agents": [
                raw_definition("first-agent"),
                original,
                raw_definition("last-agent")
            ]
        }));
        let mut updated = definition("update-agent");
        updated.display_name = "Updated Agent".to_string();
        updated.global = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Private,
            private_path: Some(CustomPathSpec::based(
                CustomPathBase::Home,
                ".updated/skills",
            )),
        };
        updated.detection_paths = vec![CustomPathSpec::based(
            CustomPathBase::ConfigHome,
            "updated/detection",
        )];

        let saved = repository.upsert(updated).expect("upsert definition");

        let ids = saved
            .valid_records()
            .map(|(definition, _)| definition.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["first-agent", "update-agent", "last-agent"]);
        let updated_raw = match &saved.records[1] {
            CustomAgentRecord::Valid { raw, .. } => raw,
            record => panic!("expected updated valid record, got {record:?}"),
        };
        assert_eq!(updated_raw["futureField"], json!({ "keep": [1, 2, 3] }));
        assert_eq!(updated_raw["displayName"], "Updated Agent");
        assert_eq!(updated_raw["global"]["location"], "private");
        assert_eq!(
            updated_raw["global"]["privatePath"],
            json!({
                "kind": "based",
                "base": "home",
                "relativePath": ".updated/skills"
            })
        );
        assert_eq!(
            updated_raw["detectionPaths"],
            json!([{
                "kind": "based",
                "base": "configHome",
                "relativePath": "updated/detection"
            }])
        );
        let stored: Value =
            serde_json::from_slice(&fs::read(repository.path()).expect("read stored repository"))
                .expect("parse stored repository");
        assert_eq!(stored["agents"][1], *updated_raw);
    }

    #[test]
    fn upsert_preserves_nested_extensions_with_schema_aware_index_merge() {
        let mut original_definition = definition("nested-agent");
        original_definition.global = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Private,
            private_path: Some(CustomPathSpec::absolute("/opt/old/global-skills")),
        };
        original_definition.project = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Private,
            private_path: Some(CustomPathSpec::based(
                CustomPathBase::Project,
                ".old/project-skills",
            )),
        };
        original_definition.detection_paths = vec![
            CustomPathSpec::absolute("/opt/old/detect-one"),
            CustomPathSpec::based(CustomPathBase::Home, ".old/detect-two"),
            CustomPathSpec::based(CustomPathBase::ConfigHome, "old/detect-three"),
        ];
        let mut original = serde_json::to_value(&original_definition).unwrap();
        original
            .as_object_mut()
            .unwrap()
            .insert("rootExtension".to_string(), json!({ "keep": "root" }));
        original["global"]
            .as_object_mut()
            .unwrap()
            .insert("scopeExtension".to_string(), json!({ "keep": "global" }));
        original["global"]["privatePath"]
            .as_object_mut()
            .unwrap()
            .extend([
                (
                    "pathExtension".to_string(),
                    json!({ "keep": "global-path" }),
                ),
                ("base".to_string(), json!("home")),
                ("relativePath".to_string(), json!("obsolete")),
            ]);
        original["project"]
            .as_object_mut()
            .unwrap()
            .insert("scopeExtension".to_string(), json!({ "keep": "project" }));
        original["project"]["privatePath"]
            .as_object_mut()
            .unwrap()
            .insert(
                "pathExtension".to_string(),
                json!({ "dropAfterNull": true }),
            );
        original["detectionPaths"][0]
            .as_object_mut()
            .unwrap()
            .extend([
                ("pathExtension".to_string(), json!({ "keep": "first" })),
                ("base".to_string(), json!("home")),
                ("relativePath".to_string(), json!("obsolete")),
            ]);
        original["detectionPaths"][1]
            .as_object_mut()
            .unwrap()
            .extend([
                ("pathExtension".to_string(), json!({ "keep": "second" })),
                ("path".to_string(), json!("/obsolete")),
            ]);
        original["detectionPaths"][2]
            .as_object_mut()
            .unwrap()
            .insert(
                "pathExtension".to_string(),
                json!({ "mustBeTruncated": true }),
            );
        let (_temp, repository) = repository_with_value(json!({
            "schemaVersion": 1,
            "agents": [
                raw_definition("first-agent"),
                original,
                raw_definition("last-agent")
            ]
        }));

        let mut first_update = definition("nested-agent");
        first_update.display_name = "Nested Agent Updated".to_string();
        first_update.global = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Private,
            private_path: Some(CustomPathSpec::based(
                CustomPathBase::Home,
                ".new/global-skills",
            )),
        };
        first_update.project = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Shared,
            private_path: None,
        };
        first_update.detection_paths = vec![
            CustomPathSpec::based(CustomPathBase::ConfigHome, "new/detect-one"),
            CustomPathSpec::absolute("/opt/new/detect-two"),
        ];

        let first_saved = repository.upsert(first_update).expect("first upsert");
        let ids = first_saved
            .valid_records()
            .map(|(definition, _)| definition.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["first-agent", "nested-agent", "last-agent"]);
        let first_raw = match &first_saved.records[1] {
            CustomAgentRecord::Valid { raw, .. } => raw,
            record => panic!("expected updated valid record, got {record:?}"),
        };
        assert_eq!(first_raw["rootExtension"], json!({ "keep": "root" }));
        assert_eq!(
            first_raw["global"]["scopeExtension"],
            json!({ "keep": "global" })
        );
        assert_eq!(
            first_raw["global"]["privatePath"]["pathExtension"],
            json!({ "keep": "global-path" })
        );
        assert_eq!(first_raw["global"]["privatePath"]["kind"], "based");
        assert_eq!(first_raw["global"]["privatePath"]["base"], "home");
        assert_eq!(
            first_raw["global"]["privatePath"]["relativePath"],
            ".new/global-skills"
        );
        assert!(first_raw["global"]["privatePath"].get("path").is_none());
        assert_eq!(
            first_raw["project"]["scopeExtension"],
            json!({ "keep": "project" })
        );
        assert!(first_raw["project"]["privatePath"].is_null());
        assert_eq!(first_raw["detectionPaths"].as_array().unwrap().len(), 2);
        assert_eq!(
            first_raw["detectionPaths"][0]["pathExtension"],
            json!({ "keep": "first" })
        );
        assert_eq!(first_raw["detectionPaths"][0]["kind"], "based");
        assert_eq!(first_raw["detectionPaths"][0]["base"], "configHome");
        assert_eq!(
            first_raw["detectionPaths"][0]["relativePath"],
            "new/detect-one"
        );
        assert!(first_raw["detectionPaths"][0].get("path").is_none());
        assert_eq!(
            first_raw["detectionPaths"][1]["pathExtension"],
            json!({ "keep": "second" })
        );
        assert_eq!(first_raw["detectionPaths"][1]["kind"], "absolute");
        assert_eq!(
            first_raw["detectionPaths"][1]["path"],
            "/opt/new/detect-two"
        );
        assert!(first_raw["detectionPaths"][1].get("base").is_none());
        assert!(first_raw["detectionPaths"][1].get("relativePath").is_none());

        let mut second_update = first_saved.valid_records().nth(1).unwrap().0.clone();
        second_update.project = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Private,
            private_path: Some(CustomPathSpec::based(
                CustomPathBase::Project,
                ".new/project-skills",
            )),
        };
        second_update.detection_paths.push(CustomPathSpec::based(
            CustomPathBase::Home,
            ".new/detect-three",
        ));

        let second_saved = repository.upsert(second_update).expect("second upsert");
        let second_raw = match &second_saved.records[1] {
            CustomAgentRecord::Valid { raw, .. } => raw,
            record => panic!("expected updated valid record, got {record:?}"),
        };
        assert_eq!(
            second_raw["project"]["scopeExtension"],
            json!({ "keep": "project" })
        );
        assert_eq!(
            second_raw["project"]["privatePath"],
            json!({
                "kind": "based",
                "base": "project",
                "relativePath": ".new/project-skills"
            })
        );
        assert_eq!(second_raw["detectionPaths"].as_array().unwrap().len(), 3);
        assert_eq!(
            second_raw["detectionPaths"][2],
            json!({
                "kind": "based",
                "base": "home",
                "relativePath": ".new/detect-three"
            })
        );
        let stored: Value =
            serde_json::from_slice(&fs::read(repository.path()).expect("read stored repository"))
                .expect("parse stored repository");
        assert_eq!(stored["agents"][1], *second_raw);
    }

    #[test]
    fn valid_record_can_be_deleted_by_id_even_when_registry_may_classify_it_as_conflict() {
        let temp = tempdir().expect("tempdir");
        let repository = CustomAgentRepository::new(temp.path().join("custom-agents.json"));
        repository
            .save(&file(vec![CustomAgentRecord::valid(definition(
                "claude-code",
            ))]))
            .expect("save potentially conflicting definition");

        let saved = repository
            .delete(&AgentId::parse("claude-code").unwrap())
            .expect("delete by persisted custom ID");

        assert!(saved.records.is_empty());
        assert!(repository.load().unwrap().records.is_empty());
    }

    #[test]
    fn delete_missing_definition_returns_invalid_agent_without_writing() {
        let (_temp, repository) = repository_with_value(json!({
            "schemaVersion": 1,
            "agents": [raw_definition("kept-agent")]
        }));
        let before = fs::read(repository.path()).expect("repository bytes");

        let error = repository
            .delete(&AgentId::parse("missing-agent").unwrap())
            .expect_err("missing definition must not be a successful no-op");

        assert_eq!(
            error,
            AppError::InvalidAgent {
                agent: "missing-agent".to_string(),
            }
        );
        assert_eq!(fs::read(repository.path()).unwrap(), before);
    }

    #[test]
    fn delete_preserves_invalid_records_and_never_touches_agent_skill_directories() {
        let temp = tempdir().expect("tempdir");
        let repository_path = temp.path().join("state/custom-agents.json");
        let repository = CustomAgentRepository::new(repository_path);
        let skill_directory = temp.path().join("home/.agents/skills/existing-skill");
        fs::create_dir_all(&skill_directory).expect("create skill directory");
        let marker = skill_directory.join("SKILL.md");
        fs::write(&marker, "keep me").expect("write marker");
        let invalid = json!({ "id": "bad id", "path": skill_directory });
        fs::create_dir_all(repository.path().parent().unwrap()).expect("create state directory");
        fs::write(
            repository.path(),
            serde_json::to_vec_pretty(&json!({
                "schemaVersion": 1,
                "agents": [raw_definition("delete-agent"), invalid.clone(), raw_definition("keep-agent")]
            }))
            .expect("serialize fixture"),
        )
        .expect("write fixture");

        let saved = repository
            .delete(&AgentId::parse("delete-agent").unwrap())
            .expect("delete definition");

        assert_eq!(saved.records.len(), 2);
        assert_eq!(
            saved.valid_records().next().unwrap().0.id.as_str(),
            "keep-agent"
        );
        assert!(matches!(
            &saved.records[0],
            CustomAgentRecord::Invalid { index: 0, raw, .. } if raw == &invalid
        ));
        assert_eq!(repository.load().expect("reload repository"), saved);
        assert_eq!(fs::read_to_string(marker).expect("read marker"), "keep me");
    }

    #[test]
    fn duplicate_draft_changes_only_the_id_without_persisting() {
        let temp = tempdir().expect("tempdir");
        let repository = CustomAgentRepository::new(temp.path().join("custom-agents.json"));
        let source = definition("source-agent");
        repository
            .save(&file(vec![CustomAgentRecord::valid(source.clone())]))
            .expect("save source");

        let draft = repository
            .duplicate_draft(
                &source.id,
                AgentId::parse("source-agent-copy").expect("copy ID"),
            )
            .expect("duplicate draft");

        assert_eq!(draft.id.as_str(), "source-agent-copy");
        assert_eq!(draft.display_name, source.display_name);
        assert_eq!(repository.load().unwrap().valid_records().count(), 1);
    }

    #[test]
    fn duplicate_draft_finds_a_source_after_other_records() {
        let temp = tempdir().expect("tempdir");
        let repository = CustomAgentRepository::new(temp.path().join("custom-agents.json"));
        repository
            .save(&file(vec![
                CustomAgentRecord::valid(definition("first-agent")),
                CustomAgentRecord::valid(definition("source-agent")),
            ]))
            .expect("save sources");

        let draft = repository
            .duplicate_draft(
                &AgentId::parse("source-agent").unwrap(),
                AgentId::parse("source-agent-copy").unwrap(),
            )
            .expect("duplicate second record");

        assert_eq!(draft.id.as_str(), "source-agent-copy");
        assert_eq!(draft.display_name, "source-agent display");
    }
}
