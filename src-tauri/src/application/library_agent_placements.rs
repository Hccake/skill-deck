use std::collections::{BTreeMap, BTreeSet};

use crate::application::agent_selection::{
    AgentInstallOptionKind, AgentSelectionCatalog, AgentSelectionModeConstraint,
    AgentSelectionSnapshot, DirectoryContentKind, DirectoryPlacementId, SkillDirectoryAccess,
};
use crate::core::agent_definition::AgentId;
use crate::environment::types::StorageAccess;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LibraryAgentPlacementError {
    UnknownAgent(AgentId),
    PartialSelection(DirectoryPlacementId),
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryAgentPlacement {
    selection_agent_ids: Vec<AgentId>,
}

impl LibraryAgentPlacement {
    pub(crate) fn selection_agent_ids(&self) -> &[AgentId] {
        &self.selection_agent_ids
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryAgentPlacementMap {
    selection: AgentSelectionSnapshot,
    placements: BTreeMap<DirectoryPlacementId, LibraryAgentPlacement>,
}

impl LibraryAgentPlacementMap {
    pub(crate) fn from_catalog(catalog: &AgentSelectionCatalog) -> Self {
        let private_agents = catalog
            .snapshot()
            .agents
            .iter()
            .filter(|agent| agent.directory_access == Some(SkillDirectoryAccess::PrivateOnly))
            .map(|agent| agent.id.clone())
            .collect::<BTreeSet<_>>();
        let mut projected_options = Vec::new();
        let mut placements = BTreeMap::new();
        for option in catalog.options() {
            if option.public.kind != AgentInstallOptionKind::StandardDirectory
                || option.public.mode_constraint != AgentSelectionModeConstraint::UserSelectable
                || !option.public.selectable
                || option.placement.storage_access != StorageAccess::Native
                || option.placement.content != DirectoryContentKind::Original
            {
                continue;
            }
            let selection_agent_ids = option
                .public
                .agent_ids
                .iter()
                .filter(|agent_id| private_agents.contains(*agent_id))
                .cloned()
                .collect::<Vec<_>>();
            if selection_agent_ids.is_empty() {
                continue;
            }
            let mut projected = option.public.clone();
            projected.agent_ids = selection_agent_ids.clone();
            projected_options.push(projected);
            placements.insert(
                option.placement.id.clone(),
                LibraryAgentPlacement {
                    selection_agent_ids,
                },
            );
        }
        let available_agents = placements
            .values()
            .flat_map(|placement| placement.selection_agent_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let mut selection = catalog.snapshot().clone();
        selection
            .agents
            .retain(|agent| available_agents.contains(&agent.id));
        selection.install_options = projected_options;
        selection.groups.clear();
        selection.initial_selected_option_ids.clear();
        selection.unavailable_explicit_agents.clear();
        selection.user_mode_option_ids.clear();
        Self {
            selection,
            placements,
        }
    }

    pub(crate) fn selection_snapshot(&self) -> &AgentSelectionSnapshot {
        &self.selection
    }

    #[cfg(test)]
    pub(crate) fn placement(&self, id: &DirectoryPlacementId) -> Option<&LibraryAgentPlacement> {
        self.placements.get(id)
    }

    pub(crate) fn placements(
        &self,
    ) -> impl Iterator<Item = (&DirectoryPlacementId, &LibraryAgentPlacement)> {
        self.placements.iter()
    }

    pub(crate) fn placements_for(
        &self,
        selected_agent_ids: &[AgentId],
    ) -> Result<BTreeSet<DirectoryPlacementId>, LibraryAgentPlacementError> {
        let selected = selected_agent_ids.iter().cloned().collect::<BTreeSet<_>>();
        let available = self
            .placements
            .values()
            .flat_map(|placement| placement.selection_agent_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        if let Some(agent_id) = selected.difference(&available).next() {
            return Err(LibraryAgentPlacementError::UnknownAgent(agent_id.clone()));
        }
        let mut result = BTreeSet::new();
        for (id, placement) in &self.placements {
            let selected_count = placement
                .selection_agent_ids
                .iter()
                .filter(|agent_id| selected.contains(*agent_id))
                .count();
            if selected_count == placement.selection_agent_ids.len() {
                result.insert(id.clone());
            } else if selected_count != 0 {
                return Err(LibraryAgentPlacementError::PartialSelection(id.clone()));
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent_selection::{
        AgentInstallOption, AgentInstallOptionId, AgentSelectionAgent, AgentSelectionAgentKind,
        AgentSelectionRevision, ResolvedAgentInstallOption, ResolvedDirectoryPlacement,
    };
    use crate::environment::agent_environment::DetectionState;
    use crate::environment::runtime::{
        ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{
        EnvironmentRef, ResourceLocator, SkillLocation, SkillLocationRef,
    };

    fn agent(id: &str, access: SkillDirectoryAccess) -> AgentSelectionAgent {
        AgentSelectionAgent {
            kind: AgentSelectionAgentKind::Standard,
            id: AgentId::parse(id).unwrap(),
            display_name: id.to_string(),
            detection: DetectionState::Detected,
            directory_access: Some(access),
            install_option_id: Some(AgentInstallOptionId("shared".to_string())),
            group_id: None,
        }
    }

    fn placement(id: DirectoryPlacementId, path: &str) -> ResolvedDirectoryPlacement {
        ResolvedDirectoryPlacement {
            id,
            root: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: path.to_string(),
            },
            physical_key: PhysicalTargetKey {
                backend: if cfg!(windows) {
                    ExecutionBackend::NativeWindows
                } else {
                    ExecutionBackend::NativeUnix
                },
                physical_parent: if cfg!(windows) {
                    PhysicalParentIdentity::Windows {
                        volume_serial: 1,
                        file_id: 2,
                    }
                } else {
                    PhysicalParentIdentity::Unix {
                        device: 1,
                        inode: 2,
                    }
                },
                normalized_final_child_name: "skills".to_string(),
            },
            storage_access: StorageAccess::Native,
            content: DirectoryContentKind::Original,
        }
    }

    fn native_path(path: &str) -> String {
        let mut result = if cfg!(windows) {
            std::path::PathBuf::from(r"C:\")
        } else {
            std::path::PathBuf::from("/")
        };
        for component in path.trim_start_matches('/').split('/') {
            result.push(component);
        }
        result.to_string_lossy().into_owned()
    }

    fn catalog() -> AgentSelectionCatalog {
        let private_one = agent("private-one", SkillDirectoryAccess::PrivateOnly);
        let private_two = agent("private-two", SkillDirectoryAccess::PrivateOnly);
        let both = agent("both", SkillDirectoryAccess::Both);
        let option_id = AgentInstallOptionId("shared".to_string());
        let public = AgentInstallOption {
            id: option_id.clone(),
            kind: AgentInstallOptionKind::StandardDirectory,
            agent_ids: vec![
                private_one.id.clone(),
                private_two.id.clone(),
                both.id.clone(),
            ],
            display_name: "Shared".to_string(),
            path: native_path("/agents/shared/skills"),
            group_id: None,
            selectable: true,
            mode_constraint: AgentSelectionModeConstraint::UserSelectable,
            disabled_reason: None,
        };
        AgentSelectionCatalog::from_parts_for_test(
            SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            AgentSelectionSnapshot {
                agents: vec![private_one, private_two, both],
                install_options: vec![public.clone()],
                groups: Vec::new(),
                initial_selected_option_ids: Vec::new(),
                unavailable_explicit_agents: Vec::new(),
                user_mode_option_ids: Vec::new(),
                revision: AgentSelectionRevision("revision".to_string()),
            },
            BTreeMap::from([(
                option_id.clone(),
                ResolvedAgentInstallOption {
                    public,
                    adapter_target_ids: Vec::new(),
                    placement: placement(
                        DirectoryPlacementId::Option(option_id),
                        &native_path("/agents/shared/skills"),
                    ),
                },
            )]),
            placement(
                DirectoryPlacementId::Standard,
                &native_path("/scope/.agents/skills"),
            ),
        )
    }

    #[test]
    fn mixed_shared_directory_uses_catalog_readers_and_private_agents_for_selection() {
        let catalog = catalog();
        let placement_id = DirectoryPlacementId::Option(AgentInstallOptionId("shared".to_string()));
        assert_eq!(catalog.readers(&placement_id).len(), 3);
        let map = LibraryAgentPlacementMap::from_catalog(&catalog);
        let placement = map.placement(&placement_id).unwrap();
        assert_eq!(
            placement.selection_agent_ids(),
            &[
                AgentId::parse("private-one").unwrap(),
                AgentId::parse("private-two").unwrap()
            ]
        );
        assert_eq!(map.selection_snapshot().agents.len(), 2);
        assert!(matches!(
            map.placements_for(&[AgentId::parse("private-one").unwrap()]),
            Err(LibraryAgentPlacementError::PartialSelection(id)) if id == placement_id
        ));
        assert!(map
            .placements_for(&[
                AgentId::parse("private-one").unwrap(),
                AgentId::parse("private-two").unwrap(),
            ])
            .unwrap()
            .contains(&placement_id));
    }
}
