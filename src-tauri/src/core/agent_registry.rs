use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::collections::{BTreeMap, BTreeSet};

use super::agent_definition::{AgentDefinition, AgentId};
use super::agent_settings::{
    ActiveCustomAgent, AgentSettingsRecords, CustomAgentRecord, DisabledAgentConflict,
    InvalidCustomAgentRecord,
};
use super::builtin_agent_catalog::builtin_agent_definitions;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentRegistrySnapshot {
    pub revision: String,
    pub active_definitions: BTreeMap<AgentId, AgentDefinition>,
}

impl AgentRegistrySnapshot {
    pub fn get(&self, id: &AgentId) -> Option<&AgentDefinition> {
        self.active_definitions.get(id).or_else(|| {
            self.active_definitions
                .values()
                .find(|definition| definition.aliases.contains(id))
        })
    }
}

#[derive(Debug, Clone)]
pub struct AgentRegistry {
    snapshot: AgentRegistrySnapshot,
    settings_records: AgentSettingsRecords,
}

impl AgentRegistry {
    pub fn new(custom_records: Vec<CustomAgentRecord>) -> Self {
        Self::build(builtin_agent_definitions(), custom_records)
    }

    /// Builds the current empty fallback when agent storage cannot be located.
    pub fn empty_unavailable() -> Self {
        Self::build(Vec::new(), Vec::new())
    }

    pub fn build(
        builtin_definitions: Vec<AgentDefinition>,
        custom_records: Vec<CustomAgentRecord>,
    ) -> Self {
        let mut active_definitions = BTreeMap::new();
        let mut builtin_ids_and_aliases = BTreeMap::new();

        let custom_ids = custom_records
            .iter()
            .filter_map(|record| match record {
                CustomAgentRecord::Valid { definition, .. } => Some(definition.id.clone()),
                CustomAgentRecord::Invalid { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        let mut original_builtins = BTreeMap::new();

        for definition in &builtin_definitions {
            let id = definition.id.clone();
            builtin_ids_and_aliases.insert(id.clone(), id.clone());
            for alias in &definition.aliases {
                builtin_ids_and_aliases.insert(alias.clone(), id.clone());
            }
            original_builtins.insert(id, definition.clone());
        }

        for definition in builtin_definitions {
            if custom_ids.contains(&definition.id) {
                continue;
            }
            let mut active_definition = definition;
            active_definition
                .aliases
                .retain(|alias| !custom_ids.contains(alias));
            active_definitions.insert(active_definition.id.clone(), active_definition);
        }

        let active_builtin = active_definitions.values().cloned().collect();
        let mut settings_records = AgentSettingsRecords {
            active_builtin,
            ..AgentSettingsRecords::default()
        };

        for (index, record) in custom_records.into_iter().enumerate() {
            match record {
                CustomAgentRecord::Invalid { index, raw, errors } => {
                    settings_records
                        .invalid_custom_records
                        .push(InvalidCustomAgentRecord {
                            index: u32::try_from(index)
                                .expect("custom Agent record index must fit in u32"),
                            raw: raw.into(),
                            errors,
                        });
                }
                CustomAgentRecord::Valid { definition, raw } => {
                    if let Some(builtin_id) = builtin_ids_and_aliases.get(&definition.id) {
                        let builtin = original_builtins
                            .get(builtin_id)
                            .expect("built-in lookup must reference an active definition")
                            .clone();
                        settings_records
                            .disabled_conflicts
                            .push(DisabledAgentConflict {
                                definition,
                                builtin,
                                raw: raw.into(),
                            });
                        continue;
                    }

                    if active_definitions.contains_key(&definition.id) {
                        settings_records
                            .invalid_custom_records
                            .push(InvalidCustomAgentRecord {
                                index: u32::try_from(index)
                                    .expect("custom Agent record index must fit in u32"),
                                raw: raw.into(),
                                errors: vec![super::agent_definition::AgentFieldError::new(
                                    "id",
                                    "duplicateAgentId",
                                )],
                            });
                        continue;
                    }

                    let normalized = definition
                        .normalize()
                        .expect("valid custom record must contain a valid definition");
                    active_definitions.insert(normalized.id.clone(), normalized);
                    settings_records.active_custom.push(ActiveCustomAgent {
                        definition,
                        raw: raw.into(),
                    });
                }
            }
        }

        let revision = registry_revision(&active_definitions);
        Self {
            snapshot: AgentRegistrySnapshot {
                revision,
                active_definitions,
            },
            settings_records,
        }
    }

    pub fn snapshot(&self) -> &AgentRegistrySnapshot {
        &self.snapshot
    }

    pub fn settings_records(&self) -> &AgentSettingsRecords {
        &self.settings_records
    }
}

fn registry_revision(active_definitions: &BTreeMap<AgentId, AgentDefinition>) -> String {
    let normalized = serde_json::to_vec(active_definitions)
        .expect("agent definitions must serialize for registry revision");
    let mut hasher = Sha256::new();
    hasher.update(normalized);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentFieldError, AgentId, AgentSource,
        CustomAgentDefinition, CustomPathBase, CustomPathSpec, CustomScopeDefinition,
        DetectionSpec, PathSpec, ScopeDefinition, ScopeLocation,
    };

    fn standard_definition(id: &str, source: AgentSource) -> AgentDefinition {
        AgentDefinition {
            id: AgentId::parse(id).unwrap(),
            display_name: "Foo".to_string(),
            source,
            aliases: Vec::new(),
            global: ScopeDefinition {
                enabled: true,
                reads_standard: true,
                private_path: None,
            },
            project: ScopeDefinition {
                enabled: false,
                reads_standard: true,
                private_path: None,
            },
            detection: DetectionSpec::AnyPathExists {
                paths: vec![PathSpec::home(".foo")],
            },
            legacy_paths: Vec::new(),
            adapter: AgentAdapter::Standard,
        }
    }

    fn standard_custom_definition(id: &str) -> CustomAgentDefinition {
        CustomAgentDefinition {
            id: AgentId::parse(id).unwrap(),
            display_name: "Foo".to_string(),
            global: CustomScopeDefinition {
                enabled: true,
                location: ScopeLocation::Standard,
                private_path: None,
            },
            project: CustomScopeDefinition {
                enabled: false,
                location: ScopeLocation::Standard,
                private_path: None,
            },
            detection_paths: vec![CustomPathSpec::based(CustomPathBase::Home, ".foo")],
        }
    }

    #[test]
    fn canonical_builtin_id_conflict_suppresses_both_runtime_definitions_until_removed() {
        let builtin = standard_definition("foo", AgentSource::Builtin);
        let custom = CustomAgentRecord::valid(standard_custom_definition("foo"));

        let registry = AgentRegistry::build(vec![builtin], vec![custom]);

        assert!(!registry
            .snapshot()
            .active_definitions
            .contains_key(&AgentId::parse("foo").unwrap()));
        assert!(registry.settings_records().active_builtin.is_empty());
        assert_eq!(registry.settings_records().disabled_conflicts.len(), 1);
        assert!(registry.settings_records().active_custom.is_empty());
    }

    #[test]
    fn builtin_alias_conflict_keeps_canonical_id_but_suppresses_the_conflicting_alias() {
        let mut builtin = standard_definition("foo", AgentSource::Builtin);
        builtin.aliases.push(AgentId::parse("old-foo").unwrap());
        let custom = CustomAgentRecord::valid(standard_custom_definition("old-foo"));

        let registry = AgentRegistry::build(vec![builtin], vec![custom]);

        assert_eq!(
            registry
                .snapshot()
                .get(&AgentId::parse("foo").unwrap())
                .unwrap()
                .id,
            AgentId::parse("foo").unwrap()
        );
        assert!(registry
            .snapshot()
            .get(&AgentId::parse("old-foo").unwrap())
            .is_none());
        assert_eq!(registry.settings_records().disabled_conflicts.len(), 1);
        assert_eq!(
            registry.settings_records().disabled_conflicts[0]
                .builtin
                .aliases,
            vec![AgentId::parse("old-foo").unwrap()]
        );
    }

    #[test]
    fn rebuilding_after_conflicting_custom_removal_restores_builtin_definitions() {
        let mut builtin = standard_definition("foo", AgentSource::Builtin);
        builtin.aliases.push(AgentId::parse("old-foo").unwrap());
        let conflict = CustomAgentRecord::valid(standard_custom_definition("old-foo"));

        let conflicted = AgentRegistry::build(vec![builtin.clone()], vec![conflict]);
        let restored = AgentRegistry::build(vec![builtin], vec![]);

        assert!(conflicted
            .snapshot()
            .get(&AgentId::parse("old-foo").unwrap())
            .is_none());
        assert_eq!(
            restored
                .snapshot()
                .get(&AgentId::parse("old-foo").unwrap())
                .unwrap()
                .id,
            AgentId::parse("foo").unwrap()
        );
    }

    #[test]
    fn invalid_custom_record_never_enters_runtime_snapshot() {
        let record = CustomAgentRecord::Invalid {
            index: 0,
            raw: serde_json::json!({ "id": "bad id" }),
            errors: vec![AgentFieldError::new("id", "invalidAgentId")],
        };

        let registry = AgentRegistry::build(vec![], vec![record]);

        assert!(registry.snapshot().active_definitions.is_empty());
        assert_eq!(registry.settings_records().invalid_custom_records.len(), 1);
    }

    #[test]
    fn valid_custom_definition_enters_runtime_and_settings_snapshots() {
        let custom = standard_custom_definition("foo");

        let registry = AgentRegistry::build(vec![], vec![CustomAgentRecord::valid(custom.clone())]);

        assert_eq!(
            registry
                .snapshot()
                .get(&AgentId::parse("foo").unwrap())
                .unwrap()
                .source,
            AgentSource::Custom
        );
        assert_eq!(
            registry.settings_records().active_custom[0].definition,
            custom
        );
    }

    #[test]
    fn revision_is_stable_for_equivalent_input_order() {
        let foo = standard_definition("foo", AgentSource::Builtin);
        let bar = standard_definition("bar", AgentSource::Builtin);

        let first = AgentRegistry::build(vec![foo.clone(), bar.clone()], vec![]);
        let second = AgentRegistry::build(vec![bar, foo], vec![]);

        assert_eq!(first.snapshot().revision, second.snapshot().revision);
        assert_eq!(first.snapshot().revision.len(), 64);
    }

    #[test]
    fn revision_changes_when_active_definitions_change() {
        let original = AgentRegistry::build(
            vec![standard_definition("foo", AgentSource::Builtin)],
            vec![],
        );
        let mut changed_definition = standard_definition("foo", AgentSource::Builtin);
        changed_definition.display_name = "Changed".to_string();
        let changed = AgentRegistry::build(vec![changed_definition], vec![]);

        assert_ne!(original.snapshot().revision, changed.snapshot().revision);
    }

    #[test]
    fn unavailable_registry_is_empty_with_a_stable_revision() {
        let first = AgentRegistry::empty_unavailable();
        let second = AgentRegistry::empty_unavailable();

        assert!(first.snapshot().active_definitions.is_empty());
        assert!(first.settings_records().active_builtin.is_empty());
        assert!(first.settings_records().active_custom.is_empty());
        assert!(first.settings_records().disabled_conflicts.is_empty());
        assert!(first.settings_records().invalid_custom_records.is_empty());
        assert_eq!(first.snapshot().revision, second.snapshot().revision);
        assert_eq!(first.snapshot().revision.len(), 64);
    }

    #[test]
    fn stale_disabled_conflict_is_recomputed_from_current_builtins() {
        let definition = standard_custom_definition("foo");
        let raw = serde_json::json!({
            "id": "foo",
            "futureField": { "mustSurvive": true }
        });
        let record = CustomAgentRecord::Valid {
            definition: definition.clone(),
            raw: raw.clone(),
        };

        let registry = AgentRegistry::build(vec![], vec![record]);

        assert!(registry.settings_records().disabled_conflicts.is_empty());
        assert_eq!(
            registry.settings_records().active_custom[0].definition,
            definition
        );
        assert_eq!(registry.settings_records().active_custom[0].raw.0, raw);
        assert_eq!(
            registry
                .snapshot()
                .get(&AgentId::parse("foo").unwrap())
                .unwrap()
                .source,
            AgentSource::Custom
        );
    }

    #[test]
    fn duplicate_custom_id_keeps_one_active_and_reports_the_later_record_invalid() {
        let first = standard_custom_definition("foo");
        let mut duplicate = standard_custom_definition("foo");
        duplicate.display_name = "Duplicate".to_string();
        let duplicate_raw = serde_json::json!({
            "id": "foo",
            "displayName": "Duplicate",
            "futureField": ["must", "survive"]
        });

        let registry = AgentRegistry::build(
            vec![],
            vec![
                CustomAgentRecord::valid(first.clone()),
                CustomAgentRecord::Valid {
                    definition: duplicate,
                    raw: duplicate_raw.clone(),
                },
            ],
        );

        assert_eq!(
            registry.settings_records().active_custom[0].definition,
            first
        );
        assert_eq!(registry.settings_records().invalid_custom_records.len(), 1);
        assert_eq!(
            registry.settings_records().invalid_custom_records[0].raw.0,
            duplicate_raw
        );
        assert_eq!(
            registry.settings_records().invalid_custom_records[0].errors,
            vec![AgentFieldError::new("id", "duplicateAgentId")]
        );
        assert_eq!(
            registry
                .snapshot()
                .get(&AgentId::parse("foo").unwrap())
                .unwrap()
                .display_name,
            "Foo"
        );
    }
}
