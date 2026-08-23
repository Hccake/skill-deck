use std::collections::BTreeSet;

use crate::application::agent_selection::{
    apply_initial_agent_selection, build_agent_selection_catalog,
    resolve_agent_selection_submission, AgentSelectionAgent, AgentSelectionCatalog,
    AgentSelectionHistoryWarning, AgentSelectionIntent, AgentSelectionResolution,
    AgentSelectionSubmission, ConfirmInstallAgentSelectionOutcome, InstallAgentSelectionSnapshot,
    ResolvedAgentSelection,
};
use crate::application::agent_selection_history;
use crate::application::planning_facts::ScopePlanningSnapshotSource;
use crate::application::runtime_admission::{MutationPermit, RuntimeAdmissionCoordinator};
use crate::core::agent_definition::AgentId;
use crate::core::builtin_agent_catalog::is_cli_history_agent;
use crate::environment::agent_environment::AgentRuntimeSnapshot;
use crate::environment::agent_environment::DetectionState;
use crate::environment::planning::TargetFactResolver;
use crate::environment::types::SkillLocationRef;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;

/// 读取当前 Agent 选择目录，并按安装意图套用初始选择。
pub async fn get_install_agent_selection<F, T>(
    context: SkillLocationRef,
    intent: AgentSelectionIntent,
    facts_source: &F,
    targets: &T,
    wsl: &WslRuntime,
) -> Result<InstallAgentSelectionSnapshot, AppError>
where
    F: ScopePlanningSnapshotSource,
    T: TargetFactResolver,
{
    let facts = facts_source.snapshot(&context).await?;
    let catalog = build_agent_selection_catalog(
        &context,
        &facts.agent_runtime,
        &facts.eve_targets,
        &facts.resolved_context.skill_root,
        targets,
    )
    .await?;

    Ok(load_initialized_install_agent_selection_snapshot(catalog, &context, &intent, wsl).await)
}

/// 确认用户提交的 Agent 选择，并在准入允许时记录本次选择历史。
pub async fn confirm_install_agent_selection<F, T>(
    context: SkillLocationRef,
    submission: AgentSelectionSubmission,
    intent: AgentSelectionIntent,
    facts_source: &F,
    targets: &T,
    wsl: &WslRuntime,
    admission: &RuntimeAdmissionCoordinator,
) -> Result<ConfirmInstallAgentSelectionOutcome, AppError>
where
    F: ScopePlanningSnapshotSource,
    T: TargetFactResolver,
{
    let (history_recording, facts) =
        read_confirmation_facts_after_admission(&intent, admission, context.clone(), || {
            facts_source.snapshot(&context)
        })
        .await?;
    let catalog = build_agent_selection_catalog(
        &context,
        &facts.agent_runtime,
        &facts.eve_targets,
        &facts.resolved_context.skill_root,
        targets,
    )
    .await?;
    let selection = match resolve_agent_selection_submission(&catalog, &submission)? {
        AgentSelectionResolution::Ready(selection) => selection,
        AgentSelectionResolution::Stale => {
            let snapshot =
                load_initialized_install_agent_selection_snapshot(catalog, &context, &intent, wsl)
                    .await;
            return Ok(ConfirmInstallAgentSelectionOutcome::SelectionStale { snapshot });
        }
    };
    let selected_agent_ids = selected_cli_agent_ids(&selection, &facts.agent_runtime);
    let environment = context.environment.clone();
    let warning = record_confirmed_agent_history(
        history_recording,
        selected_agent_ids,
        |selected_agent_ids| async move {
            agent_selection_history::set_last_selected_agents(
                &environment,
                wsl,
                &selected_agent_ids,
            )
            .await
        },
    )
    .await;

    Ok(ConfirmInstallAgentSelectionOutcome::Ready { warning })
}

enum AgentSelectionHistoryRecording {
    Skipped,
    Admitted(MutationPermit),
    Failed,
}

fn begin_agent_selection_history_recording(
    intent: &AgentSelectionIntent,
    admission: &RuntimeAdmissionCoordinator,
    context: SkillLocationRef,
) -> AgentSelectionHistoryRecording {
    if intent.is_imported() {
        return AgentSelectionHistoryRecording::Skipped;
    }
    match admission.begin_install_from_active_wizard(context) {
        Ok(permit) => AgentSelectionHistoryRecording::Admitted(permit),
        Err(_) => AgentSelectionHistoryRecording::Failed,
    }
}

async fn read_confirmation_facts_after_admission<T, Read, ReadFuture>(
    intent: &AgentSelectionIntent,
    admission: &RuntimeAdmissionCoordinator,
    context: SkillLocationRef,
    read: Read,
) -> Result<(AgentSelectionHistoryRecording, T), AppError>
where
    Read: FnOnce() -> ReadFuture,
    ReadFuture: std::future::Future<Output = Result<T, AppError>>,
{
    let recording = begin_agent_selection_history_recording(intent, admission, context);
    let facts = read().await?;
    Ok((recording, facts))
}

async fn load_initialized_install_agent_selection_snapshot(
    catalog: AgentSelectionCatalog,
    context: &SkillLocationRef,
    intent: &AgentSelectionIntent,
    wsl: &WslRuntime,
) -> InstallAgentSelectionSnapshot {
    if intent.is_imported() {
        return initialized_install_agent_selection_snapshot(catalog, intent, None, None);
    }

    match agent_selection_history::get_last_selected_agents(&context.environment, wsl).await {
        Ok(history) => {
            initialized_install_agent_selection_snapshot(catalog, intent, history.as_deref(), None)
        }
        Err(_) => initialized_install_agent_selection_snapshot(
            catalog,
            intent,
            None,
            Some(AgentSelectionHistoryWarning::ReadFailed),
        ),
    }
}

fn initialized_install_agent_selection_snapshot(
    catalog: AgentSelectionCatalog,
    intent: &AgentSelectionIntent,
    last_selected_agent_ids: Option<&[String]>,
    selection_history_warning: Option<AgentSelectionHistoryWarning>,
) -> InstallAgentSelectionSnapshot {
    let initial_agent_ids =
        initial_agent_ids(&catalog.snapshot().agents, intent, last_selected_agent_ids);
    let mut selection = catalog.snapshot().clone();
    apply_initial_agent_selection(&catalog, &mut selection, &initial_agent_ids);
    if !intent.is_imported() || intent.wildcard_requested {
        selection.unavailable_explicit_agents.clear();
    }

    InstallAgentSelectionSnapshot {
        selection,
        selection_history_warning,
    }
}

fn selected_cli_agent_ids(
    selection: &ResolvedAgentSelection,
    runtime: &AgentRuntimeSnapshot,
) -> Vec<String> {
    selected_cli_agent_ids_from_ids(&selection.selected_agent_ids(true), runtime)
}

fn selected_cli_agent_ids_from_ids(
    selected_agent_ids: &[AgentId],
    runtime: &AgentRuntimeSnapshot,
) -> Vec<String> {
    selected_agent_ids
        .iter()
        .filter(|agent_id| {
            runtime
                .agents
                .get(*agent_id)
                .is_some_and(|agent| is_cli_history_agent(agent_id, agent.definition.source))
        })
        .cloned()
        .collect::<BTreeSet<AgentId>>()
        .into_iter()
        .map(|agent_id| agent_id.to_string())
        .collect()
}

async fn record_confirmed_agent_history<Write, WriteFuture>(
    recording: AgentSelectionHistoryRecording,
    selected_agent_ids: Vec<String>,
    write: Write,
) -> Option<AgentSelectionHistoryWarning>
where
    Write: FnOnce(Vec<String>) -> WriteFuture,
    WriteFuture: std::future::Future<Output = Result<(), AppError>>,
{
    let _permit = match recording {
        AgentSelectionHistoryRecording::Skipped => return None,
        AgentSelectionHistoryRecording::Admitted(permit) => permit,
        AgentSelectionHistoryRecording::Failed => {
            return Some(AgentSelectionHistoryWarning::WriteFailed);
        }
    };
    write(selected_agent_ids)
        .await
        .err()
        .map(|_| AgentSelectionHistoryWarning::WriteFailed)
}

fn initial_agent_ids(
    agents: &[AgentSelectionAgent],
    intent: &AgentSelectionIntent,
    last_selected_agent_ids: Option<&[String]>,
) -> Vec<String> {
    if intent.wildcard_requested {
        return agents
            .iter()
            .map(|agent| agent.id.as_str().to_string())
            .collect();
    }
    if !intent.explicit_agent_ids.is_empty() {
        return intent.explicit_agent_ids.clone();
    }

    let detected_agent_ids = agents
        .iter()
        .filter(|agent| agent.detection == DetectionState::Detected)
        .map(|agent| agent.id.as_str())
        .collect::<Vec<_>>();
    let candidates: Vec<&str> = match detected_agent_ids.as_slice() {
        [] => vec!["claude-code", "opencode", "codex"],
        [agent_id] => vec![agent_id],
        _ => last_selected_agent_ids
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .collect(),
    };

    candidates
        .into_iter()
        .filter(|candidate| agents.iter().any(|agent| agent.id.as_str() == *candidate))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent_selection::{
        AgentInstallOption, AgentInstallOptionId, AgentInstallOptionKind, AgentSelectionAgent,
        AgentSelectionAgentKind, AgentSelectionCatalog, AgentSelectionIntent,
        AgentSelectionModeConstraint, AgentSelectionRevision, SkillDirectoryAccess,
    };
    use crate::application::runtime_admission::{
        RuntimeAdmissionCoordinator, WizardWindowObservation,
    };
    use crate::core::agent_definition::{AgentDefinition, AgentId, AgentSource};
    use crate::core::builtin_agent_catalog::builtin_agent_definitions;
    use crate::environment::agent_environment::{ResolvedAgent, ResolvedAgentScope};
    use crate::environment::types::{EnvironmentRef, EnvironmentStatus, SkillLocation};
    use std::cell::Cell;
    use std::collections::BTreeMap;

    #[test]
    fn multiple_detected_agents_without_history_start_unselected() {
        let agents = vec![agent("claude-code"), agent("codex")];

        assert!(initial_agent_ids(&agents, &AgentSelectionIntent::default(), None).is_empty());
    }

    #[test]
    fn multiple_detected_agents_restore_only_valid_history() {
        let agents = vec![agent("claude-code"), agent("codex")];
        let history = vec!["codex".to_string(), "removed-agent".to_string()];

        assert_eq!(
            initial_agent_ids(&agents, &AgentSelectionIntent::default(), Some(&history)),
            vec!["codex"]
        );
    }

    #[test]
    fn one_detected_agent_is_selected_without_history() {
        let agents = vec![agent("claude-code"), undetected_agent("codex")];

        assert_eq!(
            initial_agent_ids(&agents, &AgentSelectionIntent::default(), None),
            vec!["claude-code"]
        );
    }

    #[test]
    fn no_detected_agents_use_cli_default_candidates() {
        let agents = vec![
            undetected_agent("claude-code"),
            undetected_agent("opencode"),
            undetected_agent("codex"),
            undetected_agent("cursor"),
        ];

        assert_eq!(
            initial_agent_ids(&agents, &AgentSelectionIntent::default(), None),
            vec!["claude-code", "opencode", "codex"]
        );
    }

    #[test]
    fn explicit_agents_have_priority_over_detection_and_history() {
        let agents = vec![agent("claude-code"), agent("codex")];
        let explicit = vec!["claude-code".to_string()];
        let intent = AgentSelectionIntent {
            wildcard_requested: false,
            explicit_agent_ids: explicit.clone(),
        };
        let history = vec!["codex".to_string()];

        assert_eq!(
            initial_agent_ids(&agents, &intent, Some(&history)),
            explicit
        );
    }

    #[test]
    fn agent_wildcard_selects_every_available_agent_without_using_history() {
        let agents = vec![agent("claude-code"), undetected_agent("codex")];
        let intent = AgentSelectionIntent {
            wildcard_requested: true,
            explicit_agent_ids: vec!["removed-agent".to_string()],
        };
        let history = vec!["claude-code".to_string()];

        assert_eq!(
            initial_agent_ids(&agents, &intent, Some(&history)),
            vec!["claude-code", "codex"]
        );
    }

    #[test]
    fn stale_snapshot_maps_explicit_agent_to_the_latest_install_option() {
        let option_id = AgentInstallOptionId("cursor-new-location".to_string());
        let catalog = AgentSelectionCatalog::from_snapshot_for_test(
            crate::application::agent_selection::AgentSelectionSnapshot {
                agents: vec![AgentSelectionAgent {
                    install_option_id: Some(option_id.clone()),
                    ..agent("cursor")
                }],
                install_options: vec![AgentInstallOption {
                    id: option_id.clone(),
                    kind: AgentInstallOptionKind::StandardDirectory,
                    agent_ids: vec![AgentId::parse("cursor").unwrap()],
                    display_name: "Cursor".to_string(),
                    path: "~/.cursor/skills-v2".to_string(),
                    group_id: None,
                    selectable: true,
                    mode_constraint: AgentSelectionModeConstraint::UserSelectable,
                    disabled_reason: None,
                }],
                groups: Vec::new(),
                initial_selected_option_ids: Vec::new(),
                unavailable_explicit_agents: Vec::new(),
                user_mode_option_ids: vec![option_id.clone()],
                revision: AgentSelectionRevision("latest".to_string()),
            },
        );

        let snapshot = initialized_install_agent_selection_snapshot(
            catalog,
            &AgentSelectionIntent {
                wildcard_requested: false,
                explicit_agent_ids: vec!["cursor".to_string()],
            },
            None,
            None,
        );

        assert_eq!(
            snapshot.selection.initial_selected_option_ids,
            vec![option_id]
        );
        assert!(snapshot.selection.unavailable_explicit_agents.is_empty());
    }

    #[test]
    fn history_accepts_cli_builtins_and_rejects_custom_or_internal_agents() {
        assert!(is_cli_history_agent(
            &AgentId::parse("codex").unwrap(),
            AgentSource::Builtin
        ));
        assert!(!is_cli_history_agent(
            &AgentId::parse("codex").unwrap(),
            AgentSource::Custom
        ));
        assert!(!is_cli_history_agent(
            &AgentId::parse("eve").unwrap(),
            AgentSource::Builtin
        ));
        assert!(is_cli_history_agent(
            &AgentId::parse("posit-assistant").unwrap(),
            AgentSource::Builtin
        ));
    }

    #[test]
    fn confirmed_history_contains_cli_agents_from_canonical_and_selected_roots() {
        let runtime = runtime_with_agents([
            ("codex", AgentSource::Builtin),
            ("claude-code", AgentSource::Builtin),
            ("my-agent", AgentSource::Custom),
            ("eve", AgentSource::Builtin),
        ]);
        let selected = agent_ids(&["codex", "my-agent", "eve", "claude-code"]);

        assert_eq!(
            selected_cli_agent_ids_from_ids(&selected, &runtime),
            vec!["claude-code", "codex"]
        );
    }

    #[tokio::test]
    async fn explicit_agent_entry_does_not_write_history() {
        let admission = RuntimeAdmissionCoordinator::default();
        let write_started = Cell::new(false);
        let recording = begin_agent_selection_history_recording(
            &AgentSelectionIntent {
                wildcard_requested: false,
                explicit_agent_ids: vec!["codex".to_string()],
            },
            &admission,
            native_global(),
        );

        let warning =
            record_confirmed_agent_history(recording, vec!["codex".to_string()], |_| async {
                write_started.set(true);
                Ok(())
            })
            .await;

        assert_eq!(warning, None);
        assert!(!write_started.get());
    }

    #[tokio::test]
    async fn confirmation_reads_facts_only_after_reserving_the_wizard_mutation() {
        let admission = active_wizard_admission();
        let facts_read_with_permit = Cell::new(false);

        let (recording, ()) = read_confirmation_facts_after_admission(
            &AgentSelectionIntent::default(),
            &admission,
            native_global(),
            || async {
                facts_read_with_permit.set(admission.active().is_some());
                Ok(())
            },
        )
        .await
        .expect("read confirmation facts");

        assert!(facts_read_with_permit.get());
        assert!(admission.active().is_some());
        assert!(matches!(
            admission.begin_install_from_active_wizard(native_global()),
            Err(AppError::MutationBusy)
        ));
        drop(recording);
        assert!(admission.active().is_none());
    }

    #[tokio::test]
    async fn confirmation_still_reads_facts_when_history_admission_is_unavailable() {
        let admission = RuntimeAdmissionCoordinator::default();
        let facts_read = Cell::new(false);
        let write_started = Cell::new(false);

        let (recording, ()) = read_confirmation_facts_after_admission(
            &AgentSelectionIntent::default(),
            &admission,
            native_global(),
            || async {
                facts_read.set(true);
                Ok(())
            },
        )
        .await
        .expect("read confirmation facts");

        let warning = record_confirmed_agent_history(recording, Vec::new(), |_| async {
            write_started.set(true);
            Ok(())
        })
        .await;

        assert!(facts_read.get());
        assert!(!write_started.get());
        assert_eq!(warning, Some(AgentSelectionHistoryWarning::WriteFailed));
    }

    #[tokio::test]
    async fn history_write_failure_is_non_blocking_after_wizard_admission() {
        let admission = active_wizard_admission();
        let recording = begin_agent_selection_history_recording(
            &AgentSelectionIntent::default(),
            &admission,
            native_global(),
        );

        let warning =
            record_confirmed_agent_history(recording, vec!["codex".to_string()], |_| async {
                Err(AppError::MutationBusy)
            })
            .await;

        assert_eq!(warning, Some(AgentSelectionHistoryWarning::WriteFailed));
        assert!(admission.active().is_none());
    }

    fn agent(id: &str) -> AgentSelectionAgent {
        AgentSelectionAgent {
            kind: AgentSelectionAgentKind::Standard,
            id: AgentId::parse(id).expect("valid Agent id"),
            display_name: id.to_string(),
            detection: DetectionState::Detected,
            directory_access: Some(SkillDirectoryAccess::PrivateOnly),
            install_option_id: None,
            group_id: None,
        }
    }

    fn undetected_agent(id: &str) -> AgentSelectionAgent {
        AgentSelectionAgent {
            detection: DetectionState::NotDetected,
            ..agent(id)
        }
    }

    fn active_wizard_admission() -> RuntimeAdmissionCoordinator {
        let admission = RuntimeAdmissionCoordinator::default();
        admission.observe_install_wizard_window(WizardWindowObservation::Present {
            instance_id: "wizard-test".to_string(),
        });
        admission
    }

    fn native_global() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    }

    fn agent_ids(ids: &[&str]) -> Vec<AgentId> {
        ids.iter()
            .map(|id| AgentId::parse(*id).expect("valid Agent ID"))
            .collect()
    }

    fn runtime_with_agents<const N: usize>(
        agents: [(&str, AgentSource); N],
    ) -> AgentRuntimeSnapshot {
        let builtins = builtin_agent_definitions()
            .into_iter()
            .map(|definition| (definition.id.clone(), definition))
            .collect::<BTreeMap<_, _>>();
        let agents = agents
            .into_iter()
            .map(|(id, source)| {
                let agent_id = AgentId::parse(id).expect("valid Agent ID");
                let mut definition: AgentDefinition = builtins
                    .get(&agent_id)
                    .cloned()
                    .unwrap_or_else(|| builtins[&AgentId::parse("codex").unwrap()].clone());
                definition.id = agent_id.clone();
                definition.source = source;
                (
                    agent_id,
                    ResolvedAgent {
                        definition,
                        detection: DetectionState::Detected,
                        detection_reason: None,
                        global: resolved_scope(),
                        project: resolved_scope(),
                    },
                )
            })
            .collect();
        AgentRuntimeSnapshot {
            registry_revision: "registry".to_string(),
            environment_revision: "environment".to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: None,
            agents,
        }
    }

    fn resolved_scope() -> ResolvedAgentScope {
        ResolvedAgentScope {
            enabled: true,
            reads_standard: true,
            standard_path: Some("/tmp/standard".to_string()),
            private_path: None,
            read_paths: vec!["/tmp/standard".to_string()],
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        }
    }
}
