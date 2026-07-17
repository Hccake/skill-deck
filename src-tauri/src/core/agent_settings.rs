use serde::{Deserialize, Serialize};
use specta::{datatype::DataType, Generics, Type, TypeCollection};

use super::agent_definition::{AgentDefinition, AgentFieldError, CustomAgentDefinition};
use crate::environment::types::EnvironmentRef;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CustomAgentRecord {
    Valid {
        definition: CustomAgentDefinition,
        raw: serde_json::Value,
    },
    DisabledConflict {
        definition: CustomAgentDefinition,
        builtin: AgentDefinition,
        raw: serde_json::Value,
    },
    Invalid {
        index: usize,
        raw: serde_json::Value,
        errors: Vec<AgentFieldError>,
    },
}

impl CustomAgentRecord {
    pub fn valid(definition: CustomAgentDefinition) -> Self {
        let raw = serde_json::to_value(&definition)
            .expect("custom definition must serialize for a valid record");
        Self::Valid { definition, raw }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RawJsonValue(pub serde_json::Value);

impl From<serde_json::Value> for RawJsonValue {
    fn from(value: serde_json::Value) -> Self {
        Self(value)
    }
}

impl Type for RawJsonValue {
    fn inline(_: &mut TypeCollection, _: Generics) -> DataType {
        DataType::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ActiveCustomAgent {
    pub definition: CustomAgentDefinition,
    pub raw: RawJsonValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct DisabledAgentConflict {
    pub definition: CustomAgentDefinition,
    pub builtin: AgentDefinition,
    pub raw: RawJsonValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InvalidCustomAgentRecord {
    pub index: usize,
    pub raw: RawJsonValue,
    pub errors: Vec<AgentFieldError>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentSettingsRecords {
    pub active_builtin: Vec<AgentDefinition>,
    pub active_custom: Vec<ActiveCustomAgent>,
    pub disabled_conflicts: Vec<DisabledAgentConflict>,
    pub invalid_custom_records: Vec<InvalidCustomAgentRecord>,
}

impl AgentSettingsRecords {
    pub fn snapshot(
        &self,
        registry_revision: impl Into<String>,
        current_environment: EnvironmentRef,
    ) -> AgentSettingsSnapshot {
        AgentSettingsSnapshot {
            registry_revision: registry_revision.into(),
            active_builtin: self.active_builtin.clone(),
            active_custom: self.active_custom.clone(),
            disabled_conflicts: self.disabled_conflicts.clone(),
            invalid_custom_records: self.invalid_custom_records.clone(),
            current_environment,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentSettingsSnapshot {
    pub registry_revision: String,
    pub active_builtin: Vec<AgentDefinition>,
    pub active_custom: Vec<ActiveCustomAgent>,
    pub disabled_conflicts: Vec<DisabledAgentConflict>,
    pub invalid_custom_records: Vec<InvalidCustomAgentRecord>,
    pub current_environment: EnvironmentRef,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentFieldError, AgentId, AgentSource,
        CustomAgentDefinition, CustomPathBase, CustomPathSpec, CustomScopeDefinition,
        DetectionSpec, PathSpec, ScopeDefinition, ScopeLocation,
    };
    use crate::environment::types::EnvironmentRef;

    fn builtin_definition() -> AgentDefinition {
        AgentDefinition {
            id: AgentId::parse("builtin").unwrap(),
            display_name: "Built-in".to_string(),
            source: AgentSource::Builtin,
            aliases: Vec::new(),
            global: ScopeDefinition {
                enabled: true,
                reads_shared: true,
                private_path: None,
            },
            project: ScopeDefinition {
                enabled: false,
                reads_shared: true,
                private_path: None,
            },
            detection: DetectionSpec::AnyPathExists {
                paths: vec![PathSpec::home(".builtin")],
            },
            legacy_paths: Vec::new(),
            adapter: AgentAdapter::Standard,
        }
    }

    fn custom_definition(id: &str) -> CustomAgentDefinition {
        CustomAgentDefinition {
            id: AgentId::parse(id).unwrap(),
            display_name: "Custom".to_string(),
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
            detection_paths: vec![CustomPathSpec::based(CustomPathBase::Home, ".custom")],
        }
    }

    #[test]
    fn valid_constructor_preserves_the_custom_definition() {
        let definition = custom_definition("custom");
        let raw = serde_json::to_value(&definition).unwrap();

        assert_eq!(
            CustomAgentRecord::valid(definition.clone()),
            CustomAgentRecord::Valid { definition, raw }
        );
    }

    #[test]
    fn settings_snapshot_preserves_every_management_state() {
        let builtin = builtin_definition();
        let active_custom_definition = custom_definition("active-custom");
        let active_custom_raw = serde_json::json!({
            "id": "active-custom",
            "futureField": { "keep": true }
        });
        let active_custom = ActiveCustomAgent {
            definition: active_custom_definition.clone(),
            raw: active_custom_raw.clone().into(),
        };
        let conflicting_custom = custom_definition("builtin");
        let conflict_raw = serde_json::json!({
            "id": "builtin",
            "futureField": ["keep"]
        });
        let conflict = DisabledAgentConflict {
            definition: conflicting_custom,
            builtin: builtin.clone(),
            raw: conflict_raw.clone().into(),
        };
        let invalid = InvalidCustomAgentRecord {
            index: 3,
            raw: serde_json::json!({ "id": "bad id" }).into(),
            errors: vec![AgentFieldError::new("id", "invalidAgentId")],
        };
        let records = AgentSettingsRecords {
            active_builtin: vec![builtin.clone()],
            active_custom: vec![active_custom.clone()],
            disabled_conflicts: vec![conflict.clone()],
            invalid_custom_records: vec![invalid.clone()],
        };

        let snapshot = records.snapshot("abc123", EnvironmentRef::Host);

        assert_eq!(snapshot.registry_revision, "abc123");
        assert_eq!(snapshot.active_builtin, vec![builtin]);
        assert_eq!(snapshot.active_custom, vec![active_custom]);
        assert_eq!(snapshot.active_custom[0].raw.0, active_custom_raw);
        assert_eq!(snapshot.disabled_conflicts, vec![conflict]);
        assert_eq!(snapshot.disabled_conflicts[0].raw.0, conflict_raw);
        assert_eq!(snapshot.invalid_custom_records, vec![invalid]);
        assert_eq!(snapshot.current_environment, EnvironmentRef::Host);
    }
}
