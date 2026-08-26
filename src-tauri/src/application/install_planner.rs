use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::application::agent_selection::{
    build_agent_selection_catalog, resolve_agent_selection_submission, AgentInstallOptionId,
    AgentSelectionCatalog, AgentSelectionResolution, DirectoryContentKind, DirectoryPlacementId,
    InstallAgentSelectionSnapshot,
};
use crate::application::install::{
    InstallFuture, InstallOperation, InstallPlanner, InstallPreview, InstallPreviewOutcome,
    InstallRequest, InstallSkillPreview,
};
use crate::application::installed_skill_resolver::SkillDirectoryName;
#[cfg(test)]
use crate::application::library_candidates::EmptyLibraryCandidateSource;
use crate::application::library_candidates::{LibraryCandidateSnapshot, LibraryCandidateSource};
use crate::application::mutation::plan::{
    stable_digest, MutationPlan, PreparedEntryAction, PreviewToken,
};
use crate::application::mutation::planning::{
    assemble_plan, issue_preview_token, MutationPlanDraft, MutationUnitDraft,
    PreparedMutationEntries, PreviewTokenDraft,
};
use crate::application::payload_session::{PayloadSessionManager, PinnedPayloadLease};
use crate::application::planning_facts::{ScopePlanningSnapshot, ScopePlanningSnapshotSource};
use crate::application::scope_skill_planning::{
    DirectContentIdentity, DirectPlacementChange, DirectSkillChangeRequest, LibraryElectionState,
    PreparedDirectVersion, ScopeSkillPlacementSet, ScopeSkillPlanner,
};
use crate::application::skill_changes::ValidatedSkillPayload;
use crate::application::skill_paths::SkillPathObserver;
use crate::core::lossless_lock::LockSchema;
use crate::core::skill_payload::{validate_manifest_for_target, PayloadId, TargetPathProfile};
use crate::environment::content_manifest::ContentManifestReader;
use crate::environment::planning::{ResolvedTargetFact, TargetFactResolver};
use crate::environment::runtime::ExecutionBackend;
use crate::environment::types::same_environment_identity;
use crate::error::AppError;
use crate::models::InstallMode;
#[cfg(test)]
use crate::models::InstallTargetInfo;
use crate::storage::lock_plan::{LockEntryMutation, LockExpectedState, PreparedLockMutation};
use serde_json::{json, Map, Value};

pub struct ConcreteInstallPlanner<F, T> {
    facts: F,
    targets: T,
    payloads: Arc<PayloadSessionManager>,
    now: Arc<dyn Fn() -> String + Send + Sync>,
    library_candidates: Arc<dyn LibraryCandidateSource>,
}

impl<F, T> ConcreteInstallPlanner<F, T> {
    pub fn new(
        facts: F,
        targets: T,
        payloads: Arc<PayloadSessionManager>,
        now: impl Fn() -> String + Send + Sync + 'static,
        library_candidates: Arc<dyn LibraryCandidateSource>,
    ) -> Self {
        Self {
            facts,
            targets,
            payloads,
            now: Arc::new(now),
            library_candidates,
        }
    }
}

impl<F, T> InstallPlanner for ConcreteInstallPlanner<F, T>
where
    F: ScopePlanningSnapshotSource,
    T: TargetFactResolver + ContentManifestReader,
{
    fn preview<'a>(
        &'a self,
        operation: InstallOperation,
        request: &'a InstallRequest,
        payloads: Vec<PinnedPayloadLease>,
    ) -> InstallFuture<'a, Result<InstallPreviewOutcome, AppError>> {
        Box::pin(async move {
            match self.build(operation, request, payloads, false).await? {
                BuiltInstallOutcome::Ready(built) => Ok(InstallPreviewOutcome::Ready {
                    preview: built.preview,
                }),
                BuiltInstallOutcome::SelectionStale(snapshot) => {
                    Ok(InstallPreviewOutcome::SelectionStale { snapshot })
                }
            }
        })
    }

    fn rebuild<'a>(
        &'a self,
        operation: InstallOperation,
        request: &'a InstallRequest,
        payloads: Vec<PinnedPayloadLease>,
    ) -> InstallFuture<'a, Result<(PreviewToken, MutationPlan), AppError>> {
        Box::pin(async move {
            let BuiltInstallOutcome::Ready(built) =
                self.build(operation, request, payloads, true).await?
            else {
                return Err(AppError::StaleTarget);
            };
            Ok((
                built.preview.token,
                built.plan.expect("execute build produces a plan"),
            ))
        })
    }
}

struct BuiltInstall {
    preview: InstallPreview,
    plan: Option<MutationPlan>,
}

enum BuiltInstallOutcome {
    Ready(BuiltInstall),
    SelectionStale(InstallAgentSelectionSnapshot),
}

struct SkillSeed {
    original_payload_index: usize,
    eve_payload_index: Option<usize>,
    placements: Vec<InstallSkillPlacement>,
}

struct InstallSkillPlacement {
    id: DirectoryPlacementId,
    fact: ResolvedTargetFact,
    content: DirectoryContentKind,
    direct: bool,
}

struct InstallSelection {
    selected_option_ids: BTreeSet<AgentInstallOptionId>,
    eve_subagents: BTreeSet<String>,
}

impl<F, T> ConcreteInstallPlanner<F, T>
where
    F: ScopePlanningSnapshotSource,
    T: TargetFactResolver + ContentManifestReader,
{
    async fn build(
        &self,
        operation: InstallOperation,
        request: &InstallRequest,
        payloads: Vec<PinnedPayloadLease>,
        include_plan: bool,
    ) -> Result<BuiltInstallOutcome, AppError> {
        let facts = self.facts.snapshot(&request.context).await?;
        let payloads = validate_install_payloads(request, payloads).await?;
        validate_facts(request, &facts, &payloads)?;
        let uses_direct_download = payloads
            .iter()
            .any(|payload| payload.source().update.source_type == "download");
        if include_plan && uses_direct_download && !request.acknowledge_redirect {
            let redirected_download_host = self
                .payloads
                .source_snapshot(&request.discovery_session)?
                .descriptor()
                .redirected_download_host
                .clone();
            if let Some(host) = redirected_download_host {
                return Err(AppError::DirectDownloadRedirectConfirmationRequired { host });
            }
        }
        if operation != InstallOperation::Install && uses_direct_download {
            return Err(AppError::DirectDownloadUnsupportedOperation);
        }
        let catalog = build_agent_selection_catalog(
            &request.context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &facts.resolved_context.skill_root,
            &self.targets,
        )
        .await?;
        match resolve_agent_selection_submission(&catalog, &request.agent_selection)? {
            AgentSelectionResolution::Ready(_) => {}
            AgentSelectionResolution::Stale => {
                return Ok(BuiltInstallOutcome::SelectionStale(
                    InstallAgentSelectionSnapshot {
                        selection: catalog.snapshot().clone(),
                        selection_history_warning: None,
                    },
                ));
            }
        }
        let selection = install_selection(&catalog, &request.agent_selection.selected_option_ids)?;
        let has_eve_targets = !selection.eve_subagents.is_empty();

        let original_payload_count = payloads.len();
        let collection = SkillPathObserver::resolve_installed_collection(
            &facts.resolved_context,
            &facts.revisions.environment,
        )?;
        let mut derived_payloads = Vec::new();
        let mut validated_payloads = Vec::with_capacity(original_payload_count);
        let mut seeds = Vec::with_capacity(original_payload_count);
        for (original_payload_index, payload) in payloads.into_iter().enumerate() {
            let eve_payload_index = if has_eve_targets {
                let derived = crate::core::eve::derive_eve_skill_payload(payload.payload())?;
                let derived = self
                    .payloads
                    .pin_derived_payload(payload.lease(), "eve", derived)
                    .await?;
                let index = derived_payloads.len();
                derived_payloads.push(derived);
                Some(index)
            } else {
                None
            };
            validated_payloads.push(payload);
            seeds.push(SkillSeed {
                original_payload_index,
                eve_payload_index,
                placements: Vec::new(),
            });
        }
        let mut record_names = facts
            .lock_document
            .clone()
            .into_value()
            .get("skills")
            .and_then(serde_json::Value::as_object)
            .map(|skills| {
                skills
                    .keys()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        record_names.extend(
            validated_payloads
                .iter()
                .map(|payload| payload.name().to_string()),
        );
        let prepared_targets = SkillPathObserver::resolve_install_targets(
            &self.targets,
            &collection,
            validated_payloads
                .iter()
                .map(|payload| payload.name().to_string())
                .collect(),
            record_names,
            None,
        )
        .await?;
        if prepared_targets.len() != seeds.len() {
            return Err(AppError::StaleTarget);
        }
        let mut payloads = Vec::with_capacity(original_payload_count);
        let mut canonical_skills = Vec::with_capacity(original_payload_count);
        for ((seed, payload), target) in seeds
            .iter_mut()
            .zip(validated_payloads)
            .zip(prepared_targets)
        {
            seed.placements.push(InstallSkillPlacement {
                id: DirectoryPlacementId::Standard,
                fact: target.target,
                content: DirectoryContentKind::Original,
                direct: true,
            });
            canonical_skills.push(target.skill_name);
            payloads.push(payload);
        }

        let mut library_candidate_sets = Vec::with_capacity(seeds.len());
        for skill in &canonical_skills {
            let directory_name = SkillDirectoryName::try_from(skill.as_str())?;
            library_candidate_sets.push(
                self.library_candidates
                    .load_candidates(&request.context, &directory_name)
                    .await?,
            );
        }

        for (seed, skill) in seeds.iter_mut().zip(&canonical_skills) {
            let install_dir_name = SkillDirectoryName::try_from(skill.as_str())?;
            let options = catalog
                .options()
                .map(|option| {
                    let direct = selection.selected_option_ids.contains(&option.public.id);
                    (option, direct)
                })
                .collect::<Vec<_>>();
            let destinations = options
                .iter()
                .map(|(option, _)| option.placement.root.join_child(install_dir_name.as_ref()))
                .collect::<Vec<_>>();
            let facts = if destinations.is_empty() {
                Vec::new()
            } else {
                self.targets
                    .resolve(&request.context, &destinations, None)
                    .await?
            };
            if facts.len() != destinations.len() {
                return Err(AppError::StaleTarget);
            }
            seed.placements.extend(options.into_iter().zip(facts).map(
                |((option, direct), fact)| InstallSkillPlacement {
                    id: DirectoryPlacementId::Option(option.public.id.clone()),
                    fact,
                    content: option.placement.content.clone(),
                    direct,
                },
            ));
        }
        validate_payload_targets(&payloads, &derived_payloads, &seeds).await?;

        let target_facts = seeds
            .iter()
            .flat_map(|seed| {
                seed.placements
                    .iter()
                    .map(|placement| placement.fact.clone())
            })
            .collect::<Vec<_>>();
        let candidate_digests = library_candidate_sets
            .iter()
            .map(library_snapshot_digest)
            .collect::<Result<Vec<_>, AppError>>()?;
        let observed_state_digest = stable_digest(&(
            observed_digest(&target_facts, &facts, &payloads[..original_payload_count])?,
            candidate_digests,
        ))?;
        let token = issue_preview_token(PreviewTokenDraft {
            kind: operation.mutation_kind(),
            request,
            revisions: facts.revisions.clone(),
            observed_state_digest,
            planner_contract_version: 3,
        })?;
        let mut preview_skills = Vec::with_capacity(seeds.len());
        let mut units = Vec::with_capacity(seeds.len());
        for (seed, library_candidates) in seeds.iter().zip(&library_candidate_sets) {
            let payload = &payloads[seed.original_payload_index];
            let metadata = payload.planning_metadata();
            let direct_facts = seed
                .placements
                .iter()
                .filter(|placement| placement.direct)
                .map(|placement| placement.fact.clone())
                .collect::<Vec<_>>();
            let direct_download_conflict = direct_download_conflict(payload, &direct_facts);
            preview_skills.push(InstallSkillPreview {
                skill_name: metadata.skill_name.clone(),
                payload: payload.handle().clone(),
                overwrite_targets: direct_facts
                    .iter()
                    .filter(|fact| fact.fingerprint.0 != "entry-v1-missing")
                    .map(|fact| fact.destination.native_path.clone())
                    .collect(),
                blocking_reasons: direct_download_conflict
                    .is_some()
                    .then_some(crate::application::mutation::result::OperationErrorCode::Validation)
                    .into_iter()
                    .collect(),
                fallback_forecasts: Vec::new(),
                overrides_library: !library_candidates.candidates().ordered().is_empty(),
            });
            if let Some(target) = direct_download_conflict {
                if include_plan {
                    return Err(AppError::DirectDownloadConflict { target });
                }
                continue;
            }
            let entries = plan_install_entries(
                request,
                &catalog,
                seed,
                library_candidates,
                payload,
                seed.eve_payload_index.map(|index| &derived_payloads[index]),
            )?;
            let unit = build_unit(
                request,
                &facts,
                payload,
                &selection.eve_subagents,
                entries,
                (self.now)(),
            )?;
            if include_plan {
                units.push(unit);
            }
        }
        let preview = InstallPreview {
            token,
            skills: preview_skills,
        };
        let plan = include_plan.then(|| {
            assemble_plan(MutationPlanDraft {
                kind: operation.mutation_kind(),
                payloads: payloads
                    .into_iter()
                    .map(ValidatedSkillPayload::into_lease)
                    .chain(derived_payloads)
                    .map(|lease| (lease.manifest().payload_id().clone(), lease))
                    .collect::<BTreeMap<PayloadId, PinnedPayloadLease>>(),
                units,
            })
        });
        Ok(BuiltInstallOutcome::Ready(BuiltInstall { preview, plan }))
    }
}

fn install_selection(
    catalog: &AgentSelectionCatalog,
    selected_option_ids: &[AgentInstallOptionId],
) -> Result<InstallSelection, AppError> {
    let selected_option_ids = selected_option_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut eve_subagents = BTreeSet::new();
    for option_id in &selected_option_ids {
        let option = catalog.option(option_id).ok_or(AppError::StaleTarget)?;
        if let Some(subagent) = option.placement.content.eve_subagent() {
            eve_subagents.insert(subagent.unwrap_or("").to_string());
        }
    }
    Ok(InstallSelection {
        selected_option_ids,
        eve_subagents,
    })
}

fn library_snapshot_digest(snapshot: &LibraryCandidateSnapshot) -> Result<String, AppError> {
    let recognized = snapshot
        .candidates()
        .recognized()
        .iter()
        .map(|candidate| {
            (
                candidate.library_id(),
                candidate.member_name(),
                candidate.locator(),
            )
        })
        .collect::<Vec<_>>();
    let ordered = snapshot
        .candidates()
        .ordered()
        .iter()
        .map(|candidate| {
            (
                candidate.library_id(),
                candidate.member_name(),
                candidate.locator(),
            )
        })
        .collect::<Vec<_>>();
    stable_digest(&(
        snapshot.evidence_digest(),
        snapshot.selected_agent_ids(),
        recognized,
        ordered,
    ))
}

fn plan_install_entries(
    request: &InstallRequest,
    catalog: &AgentSelectionCatalog,
    seed: &SkillSeed,
    snapshot: &LibraryCandidateSnapshot,
    payload: &ValidatedSkillPayload,
    eve_payload: Option<&PinnedPayloadLease>,
) -> Result<PreparedMutationEntries, AppError> {
    let original_payload_id = payload.manifest().payload_id().clone();
    let mut resolved = BTreeMap::new();
    let mut direct_changes = BTreeMap::new();
    for placement in &seed.placements {
        resolved.insert(placement.id.clone(), placement.fact.clone());
        let change = if placement.direct {
            let payload_id = if placement.content.uses_eve_payload() {
                eve_payload
                    .expect("Eve target planning pins a derived payload")
                    .manifest()
                    .payload_id()
                    .clone()
            } else {
                original_payload_id.clone()
            };
            DirectPlacementChange::Set(PreparedDirectVersion::new(
                DirectContentIdentity::Payload(payload_id.clone()),
                PreparedEntryAction::Replace {
                    payload_id,
                    requested_mode: if placement.id == DirectoryPlacementId::Standard
                        || placement.content.uses_eve_payload()
                    {
                        InstallMode::Copy
                    } else {
                        request.agent_selection.requested_mode.clone()
                    },
                },
            ))
        } else {
            DirectPlacementChange::Preserve
        };
        direct_changes.insert(placement.id.clone(), change);
    }
    let skill = SkillDirectoryName::try_from(payload.planning_metadata().skill_name.as_str())?;
    ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
        skill,
        catalog,
        placements: ScopeSkillPlacementSet::new(request.context.clone(), resolved),
        libraries: LibraryElectionState {
            candidates: snapshot.candidates(),
            selected_agent_ids: snapshot.selected_agent_ids(),
        },
        direct_changes,
    })
    .map(|plan| plan.compile_entries())
    .map_err(|error| error.into_app_error())
}

fn direct_download_conflict(
    payload: &ValidatedSkillPayload,
    facts: &[ResolvedTargetFact],
) -> Option<String> {
    if payload.source().update.source_type != "download" {
        return None;
    }
    facts
        .iter()
        .find(|fact| fact.fingerprint.0 != "entry-v1-missing")
        .map(|fact| fact.destination.native_path.clone())
}

async fn validate_install_payloads(
    request: &InstallRequest,
    payloads: Vec<PinnedPayloadLease>,
) -> Result<Vec<ValidatedSkillPayload>, AppError> {
    if payloads.len() != request.payloads.len() || payloads.len() != request.skills.len() {
        return Err(AppError::StalePayload);
    }
    let mut validated = Vec::with_capacity(payloads.len());
    for (index, lease) in payloads.into_iter().enumerate() {
        validated.push(
            ValidatedSkillPayload::validate(
                request.payloads[index].clone(),
                &request.discovery_session,
                &request.context.environment,
                &request.skills[index],
                lease,
            )
            .await?,
        );
    }
    Ok(validated)
}

fn validate_facts(
    request: &InstallRequest,
    facts: &ScopePlanningSnapshot,
    payloads: &[ValidatedSkillPayload],
) -> Result<(), AppError> {
    if facts.resolved_context.context != request.context
        || !same_environment_identity(
            &facts.agent_runtime.environment,
            &request.context.environment,
        )
        || facts.agent_runtime.registry_revision != facts.revisions.registry
        || facts.agent_runtime.environment_revision != facts.revisions.environment
        || payloads.len() != request.skills.len()
        || payloads.iter().enumerate().any(|(index, payload)| {
            payload.planning_metadata().validate().is_err()
                || payload.planning_metadata().skill_name != request.skills[index]
                || payload.handle().payload_id != request.payloads[index].payload_id
                || payload.content_manifest() != request.payloads[index].manifest_hash
        })
    {
        return Err(AppError::StaleContext);
    }
    Ok(())
}

async fn validate_payload_targets(
    payloads: &[ValidatedSkillPayload],
    derived_payloads: &[PinnedPayloadLease],
    seeds: &[SkillSeed],
) -> Result<(), AppError> {
    for seed in seeds {
        let canonical = payloads[seed.original_payload_index].payload();
        let eve = match seed.eve_payload_index {
            Some(index) => Some(derived_payloads[index].load_payload().await?),
            None => None,
        };
        for placement in seed.placements.iter().filter(|placement| placement.direct) {
            let payload = if placement.content.uses_eve_payload() {
                eve.as_ref().ok_or(AppError::StalePayload)?
            } else {
                canonical
            };
            let profile = match placement.fact.key.backend {
                ExecutionBackend::NativeWindows => TargetPathProfile::native_windows(),
                ExecutionBackend::NativeUnix | ExecutionBackend::WslPosix { .. } => {
                    TargetPathProfile::native_unix()
                }
            };
            validate_manifest_for_target(payload, &placement.fact.destination, &profile)?;
        }
    }
    Ok(())
}

fn build_unit(
    request: &InstallRequest,
    facts: &ScopePlanningSnapshot,
    payload: &ValidatedSkillPayload,
    eve_subagents: &BTreeSet<String>,
    entries: PreparedMutationEntries,
    now: String,
) -> Result<MutationUnitDraft, AppError> {
    let metadata = payload.planning_metadata();
    Ok(MutationUnitDraft {
        id: format!("install:{}", metadata.install_dir_name),
        skill_name: metadata.skill_name.clone(),
        source: None,
        target: request.context.clone(),
        expected_revisions: facts.revisions.clone(),
        entries,
        lock_mutation: (metadata.source_type != "download")
            .then(|| lock_mutation(facts, metadata, eve_subagents, now))
            .transpose()?,
    })
}

fn lock_mutation(
    facts: &ScopePlanningSnapshot,
    metadata: &crate::application::payload_session::PayloadPlanningMetadata,
    eve_subagents: &BTreeSet<String>,
    now: String,
) -> Result<PreparedLockMutation, AppError> {
    let resolved = crate::application::installed_skill_resolver::InstalledSkillResolver::resolve(
        &metadata.skill_name,
        &facts.lock_document,
    )?;
    if resolved.install_dir_name != metadata.install_dir_name {
        return Err(AppError::ConfigurationCorrupted {
            message: format!(
                "Skill '{}' install directory does not match its resolved identity",
                metadata.skill_name
            ),
        });
    }
    let affected_keys = if resolved.requires_lock_key_migration() {
        vec![resolved.lock_key.clone(), resolved.skill_name.clone()]
    } else {
        vec![resolved.lock_key.clone()]
    };
    let expected = LockExpectedState::capture(
        &facts.lock_document,
        &affected_keys,
        std::iter::empty::<&str>(),
    );
    let existing = facts
        .lock_document
        .entry_snapshot(&resolved.lock_key)
        .value()
        .cloned();
    let replacement = match facts.lock_schema {
        LockSchema::Global => {
            let installed_at = existing
                .as_ref()
                .and_then(|entry| entry.get("installedAt"))
                .and_then(Value::as_str)
                .unwrap_or(&now)
                .to_string();
            let well_known = metadata.well_known.as_ref();
            json!({
                "source": metadata.source,
                "sourceType": metadata.source_type,
                "sourceUrl": well_known.map(|value| value.artifact_url.clone()).or_else(|| metadata.source_url.clone()),
                "sourceBaseUrl": well_known.and(metadata.source_url.clone()),
                "wellKnownDigest": well_known.map(|value| value.digest.clone()),
                "ref": metadata.ref_name,
                "skillPath": metadata.skill_path,
                "skillFolderHash": metadata.global_skill_folder_hash(),
                "installedAt": installed_at,
                "updatedAt": now,
                "pluginName": metadata.plugin_name,
            })
        }
        LockSchema::Project => {
            let mut entry = Map::new();
            let source = if metadata.source_type == "local" {
                facts
                    .resolved_context
                    .project
                    .as_ref()
                    .map(|project| {
                        crate::core::portable_project_path::serialize_project_source(
                            &project.native_path,
                            &metadata.source,
                        )
                    })
                    .unwrap_or_else(|| metadata.source.clone())
            } else {
                metadata.source.clone()
            };
            entry.insert("source".to_string(), json!(source));
            entry.insert("sourceType".to_string(), json!(metadata.source_type));
            entry.insert("sourceUrl".to_string(), json!(metadata.source_url));
            if let Some(well_known) = &metadata.well_known {
                entry.insert("wellKnownDigest".to_string(), json!(well_known.digest));
            }
            entry.insert("ref".to_string(), json!(metadata.ref_name));
            entry.insert("skillPath".to_string(), json!(metadata.skill_path));
            entry.insert("computedHash".to_string(), json!(metadata.computed_hash));
            entry.insert("pluginName".to_string(), json!(metadata.plugin_name));
            if let Some(remote_hash) = &metadata.upstream_revision {
                entry.insert("remoteHash".to_string(), json!(remote_hash));
            }
            if eve_subagents.iter().any(|target| !target.is_empty()) {
                entry.insert("subagents".to_string(), json!(eve_subagents));
            }
            Value::Object(entry)
        }
    };
    Ok(PreparedLockMutation {
        target: facts.resolved_context.lock.clone(),
        legacy_target: None,
        schema: facts.lock_schema,
        entry: if resolved.requires_lock_key_migration() {
            LockEntryMutation::MoveAndReplace {
                from: resolved.lock_key,
                to: resolved.skill_name,
                replacement,
            }
        } else {
            LockEntryMutation::Replace {
                key: resolved.lock_key,
                replacement,
            }
        },
        root_replacements: BTreeMap::new(),
        expected,
    })
}

fn observed_digest(
    target_facts: &[ResolvedTargetFact],
    facts: &ScopePlanningSnapshot,
    payloads: &[ValidatedSkillPayload],
) -> Result<String, AppError> {
    let lock_entries = payloads
        .iter()
        .map(|payload| {
            let skill_name = &payload.planning_metadata().skill_name;
            let resolved =
                crate::application::installed_skill_resolver::InstalledSkillResolver::resolve(
                    skill_name,
                    &facts.lock_document,
                )?;
            Ok((
                resolved.lock_key.clone(),
                facts
                    .lock_document
                    .entry_snapshot(&resolved.lock_key)
                    .value()
                    .cloned(),
            ))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let targets = target_facts
        .iter()
        .map(|target| (&target.key, &target.fingerprint))
        .collect::<Vec<_>>();
    stable_digest(&(targets, lock_entries))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::application::agent_selection::test_submission_for_agents;
    use crate::application::install::{InstallPlanner, InstallRequest};
    use crate::application::mutation::plan::RuntimeRevisions;
    use crate::application::payload_session::{
        DiscoverySourceDescriptor, DiscoverySourceLocation, PayloadPlanningMetadata,
        PayloadSessionLimits, PayloadSessionManager, RetainedDiscoverySource,
        WellKnownPlanningMetadata,
    };
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::core::mutation::MutationKind;
    use crate::core::skill_payload::build_skill_payload;
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::environment::context_resolver::ResolvedContext;
    use crate::environment::planning::RuntimeTargetFactResolver;
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, ResourceLocator, SkillLocation, SkillLocationRef,
    };
    use crate::environment::wsl::WslRuntime;
    use crate::models::InstallMode;

    struct Facts(ScopePlanningSnapshot);

    impl ScopePlanningSnapshotSource for Facts {
        fn snapshot<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> crate::application::install::InstallFuture<
            'a,
            Result<ScopePlanningSnapshot, crate::error::AppError>,
        > {
            Box::pin(async move { Ok(self.0.clone()) })
        }
    }

    struct FixedCandidates(crate::application::library_candidates::LibraryCandidateSnapshot);

    impl crate::application::library_candidates::LibraryCandidateSource for FixedCandidates {
        fn load_candidates<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
            _skill: &'a crate::application::installed_skill_resolver::SkillDirectoryName,
        ) -> crate::application::library_candidates::LibraryCandidateFuture<
            'a,
            Result<crate::application::library_candidates::LibraryCandidateSnapshot, AppError>,
        > {
            Box::pin(async move { Ok(self.0.clone()) })
        }
    }

    #[tokio::test]
    async fn planner_builds_canonical_private_and_lock_as_one_unit() {
        let temp = tempdir().unwrap();
        let physical_root = fs::canonicalize(temp.path()).unwrap();
        let canonical_root = physical_root.join(".agents/skills");
        fs::create_dir_all(&canonical_root).unwrap();
        let source = temp.path().join("source/demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\nbody",
        )
        .unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let payload_id = payload.payload_id.clone();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let discovery = manager
            .discover(EnvironmentRef::Native, "source-fingerprint")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                "skills/demo",
                payload,
                PayloadPlanningMetadata {
                    skill_name: "demo".to_string(),
                    install_dir_name: "demo".to_string(),
                    source: "/tmp/local-skills".to_string(),
                    source_type: "local".to_string(),
                    source_url: None,
                    ref_name: None,
                    skill_path: "skills/demo".to_string(),
                    plugin_name: None,
                    computed_hash: "cli-computed-hash".to_string(),
                    upstream_revision: None,
                    well_known: None,
                },
            )
            .await
            .unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let private_root = physical_root.join(".custom/skills");
        let facts = ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator(&physical_root),
                skill_root: locator(&canonical_root),
                lock: locator(&physical_root.join(".agents/.skill-lock.json")),
            },
            agent_runtime: runtime(private_root.to_string_lossy().as_ref()),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-install").unwrap(),
            },
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::empty(LockSchema::Global),
            eve_targets: Vec::new(),
        };
        let target_resolver = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));
        let agent_selection = test_submission_for_agents(
            &context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &facts.resolved_context.skill_root,
            &target_resolver,
            &["custom-private"],
            InstallMode::Symlink,
        )
        .await;
        let planner = ConcreteInstallPlanner::new(
            Facts(facts),
            target_resolver,
            Arc::clone(&manager),
            || "2026-07-18T00:00:00.000Z".to_string(),
            Arc::new(EmptyLibraryCandidateSource),
        );
        let request = InstallRequest {
            context,
            source: "owner/repo".to_string(),
            discovery_session: discovery,
            payloads: vec![handle.clone()],
            skills: vec!["demo".to_string()],
            agent_selection,
            acknowledge_redirect: true,
        };

        let preview_lease = manager.pin_verified(&handle).await.unwrap();
        let preview = planner
            .preview(InstallOperation::Install, &request, vec![preview_lease])
            .await
            .unwrap();
        let execute_lease = manager.pin_verified(&handle).await.unwrap();
        let (token, plan) = planner
            .rebuild(InstallOperation::Install, &request, vec![execute_lease])
            .await
            .unwrap();
        let InstallPreviewOutcome::Ready {
            preview: repair_preview,
        } = planner
            .preview(
                InstallOperation::Repair,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await
            .unwrap()
        else {
            panic!("expected ready repair preview");
        };
        let (repair_token, repair_plan) = planner
            .rebuild(
                InstallOperation::Repair,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await
            .unwrap();

        let InstallPreviewOutcome::Ready { preview } = preview else {
            panic!("expected ready preview");
        };
        assert_eq!(preview.token, token);
        assert_eq!(plan.kind, MutationKind::Install);
        assert_eq!(repair_preview.token, repair_token);
        assert_eq!(repair_plan.kind, MutationKind::Repair);
        assert_ne!(preview.token, repair_preview.token);
        assert_eq!(preview.skills.len(), 1);
        assert_eq!(plan.payloads.len(), 1);
        assert_eq!(plan.units.len(), 1);
        let unit = &plan.units[0];
        let canonical = unit.primary_entry.as_ref().unwrap();
        assert_eq!(
            canonical.destination.native_path,
            canonical_root.join("demo").to_string_lossy()
        );
        assert!(canonical.reader_agent_ids.is_empty());
        assert_eq!(unit.additional_entries.len(), 1);
        assert_eq!(
            unit.additional_entries[0].destination.native_path,
            private_root.join("demo").to_string_lossy()
        );
        assert!(matches!(
            unit.additional_entries[0].action,
            crate::application::mutation::plan::PreparedEntryAction::Replace {
                requested_mode: InstallMode::Symlink,
                ..
            }
        ));
        let lock = unit.lock_mutation.as_ref().unwrap();
        assert_eq!(lock.skill_name(), "demo");
        assert_eq!(
            lock.replacement().unwrap()["skillFolderHash"],
            "cli-computed-hash"
        );
        assert_ne!(payload_id.as_str(), "cli-computed-hash");
    }

    #[tokio::test]
    async fn install_without_an_applied_library_rejects_an_external_file_target() {
        let temp = tempdir().unwrap();
        let physical_root = fs::canonicalize(temp.path()).unwrap();
        let canonical_root = physical_root.join(".agents/skills");
        fs::create_dir_all(&canonical_root).unwrap();
        let external_target = canonical_root.join("demo");
        fs::write(&external_target, b"external content").unwrap();
        let source = physical_root.join("source/demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\nbody",
        )
        .unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let discovery = manager
            .discover(EnvironmentRef::Native, "source-fingerprint")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                "skills/demo",
                payload,
                PayloadPlanningMetadata {
                    skill_name: "demo".to_string(),
                    install_dir_name: "demo".to_string(),
                    source: "owner/repo".to_string(),
                    source_type: "github".to_string(),
                    source_url: Some("https://github.com/owner/repo.git".to_string()),
                    ref_name: Some("main".to_string()),
                    skill_path: "skills/demo".to_string(),
                    plugin_name: None,
                    computed_hash: "computed-hash".to_string(),
                    upstream_revision: Some("remote-hash".to_string()),
                    well_known: None,
                },
            )
            .await
            .unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let facts = ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator(&physical_root),
                skill_root: locator(&canonical_root),
                lock: locator(&physical_root.join(".agents/.skill-lock.json")),
            },
            agent_runtime: runtime(
                physical_root
                    .join(".custom/skills")
                    .to_string_lossy()
                    .as_ref(),
            ),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-no-library-file").unwrap(),
            },
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::empty(LockSchema::Global),
            eve_targets: Vec::new(),
        };
        let target_resolver = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));
        let agent_selection = test_submission_for_agents(
            &context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &facts.resolved_context.skill_root,
            &target_resolver,
            &[],
            InstallMode::Copy,
        )
        .await;
        let planner = ConcreteInstallPlanner::new(
            Facts(facts),
            target_resolver,
            Arc::clone(&manager),
            || "2026-08-30T00:00:00.000Z".to_string(),
            Arc::new(EmptyLibraryCandidateSource),
        );
        let request = InstallRequest {
            context,
            source: "owner/repo".to_string(),
            discovery_session: discovery,
            payloads: vec![handle.clone()],
            skills: vec!["demo".to_string()],
            agent_selection,
            acknowledge_redirect: true,
        };

        assert!(matches!(
            planner
                .preview(
                    InstallOperation::Install,
                    &request,
                    vec![manager.pin_verified(&handle).await.unwrap()],
                )
                .await,
            Err(AppError::SkillPlacementTargetConflict {
                skill_name,
                target_kind: crate::error::SkillPlacementTargetKind::File,
                ..
            }) if skill_name == "demo"
        ));

        let result = planner
            .rebuild(
                InstallOperation::Install,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("external content must not produce an install plan"),
        };

        assert!(matches!(
            error,
            AppError::SkillPlacementTargetConflict {
                skill_name,
                agent_ids,
                target_path,
                target_kind,
            } if skill_name == "demo"
                && agent_ids.is_empty()
                && target_path == external_target.to_string_lossy()
                && target_kind == crate::error::SkillPlacementTargetKind::File
        ));
    }

    #[tokio::test]
    async fn direct_download_only_plans_new_install_without_lock_mutation() {
        let temp = tempdir().unwrap();
        let physical_root = fs::canonicalize(temp.path()).unwrap();
        let canonical_root = physical_root.join(".agents/skills");
        fs::create_dir_all(&canonical_root).unwrap();
        let source = temp.path().join("source/demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\nbody",
        )
        .unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let discovery = manager
            .discover_with_retained_source(
                EnvironmentRef::Native,
                "download-fingerprint",
                RetainedDiscoverySource::new(
                    DiscoverySourceLocation::Native {
                        root: source.clone(),
                        ref_revision: None,
                    },
                    DiscoverySourceDescriptor {
                        source: "https://example.com/SKILL.md".to_string(),
                        source_type: "download".to_string(),
                        source_url: Some("https://example.com/SKILL.md".to_string()),
                        ref_name: None,
                        redirected_download_host: Some("cdn.example.net".to_string()),
                    },
                    BTreeMap::new(),
                    (),
                ),
            )
            .await
            .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                "SKILL.md",
                payload,
                PayloadPlanningMetadata {
                    skill_name: "demo".to_string(),
                    install_dir_name: "demo".to_string(),
                    source: "https://example.com/SKILL.md".to_string(),
                    source_type: "download".to_string(),
                    source_url: Some("https://example.com/SKILL.md".to_string()),
                    ref_name: None,
                    skill_path: "SKILL.md".to_string(),
                    plugin_name: None,
                    computed_hash: "download-computed-hash".to_string(),
                    upstream_revision: None,
                    well_known: None,
                },
            )
            .await
            .unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let facts = ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator(&physical_root),
                skill_root: locator(&canonical_root),
                lock: locator(&physical_root.join(".agents/.skill-lock.json")),
            },
            agent_runtime: runtime(
                physical_root
                    .join(".custom/skills")
                    .to_string_lossy()
                    .as_ref(),
            ),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-download").unwrap(),
            },
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::empty(LockSchema::Global),
            eve_targets: Vec::new(),
        };
        let target_resolver = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));
        let agent_selection = test_submission_for_agents(
            &context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &facts.resolved_context.skill_root,
            &target_resolver,
            &[],
            InstallMode::Copy,
        )
        .await;
        let planner = ConcreteInstallPlanner::new(
            Facts(facts),
            target_resolver,
            Arc::clone(&manager),
            || "2026-08-13T00:00:00.000Z".to_string(),
            Arc::new(EmptyLibraryCandidateSource),
        );
        let request = InstallRequest {
            context,
            source: "https://example.com/SKILL.md".to_string(),
            discovery_session: discovery,
            payloads: vec![handle.clone()],
            skills: vec!["demo".to_string()],
            agent_selection,
            acknowledge_redirect: false,
        };

        let unacknowledged = planner
            .rebuild(
                InstallOperation::Install,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await;
        assert!(matches!(
            unacknowledged,
            Err(AppError::DirectDownloadRedirectConfirmationRequired { host })
                if host == "cdn.example.net"
        ));
        let request = InstallRequest {
            acknowledge_redirect: true,
            ..request
        };

        let (_, plan) = planner
            .rebuild(
                InstallOperation::Install,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await
            .unwrap();
        assert!(plan.units[0].lock_mutation.is_none());

        let repair = planner
            .preview(
                InstallOperation::Repair,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await;
        assert!(matches!(
            repair,
            Err(AppError::DirectDownloadUnsupportedOperation)
        ));

        fs::create_dir_all(canonical_root.join("demo")).unwrap();
        let conflict_preview = planner
            .preview(
                InstallOperation::Install,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await
            .unwrap();
        let InstallPreviewOutcome::Ready { preview } = conflict_preview else {
            panic!("expected ready conflict preview");
        };
        assert_eq!(preview.skills[0].overwrite_targets.len(), 1);
        assert_eq!(
            preview.skills[0].blocking_reasons,
            vec![crate::application::mutation::result::OperationErrorCode::Validation]
        );

        let conflict = planner
            .rebuild(
                InstallOperation::Install,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await;
        assert!(matches!(
            conflict,
            Err(AppError::DirectDownloadConflict { .. })
        ));
    }

    #[tokio::test]
    async fn eve_targets_use_a_derived_payload_without_changing_canonical_lock_hashes() {
        let temp = tempdir().unwrap();
        let project = temp.path().join("project");
        let canonical_root = project.join(".agents/skills");
        let source = temp.path().join("source/demo");
        fs::create_dir_all(source.join("scripts")).unwrap();
        fs::create_dir_all(&canonical_root).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n# Demo\n",
        )
        .unwrap();
        fs::write(source.join("scripts/run.sh"), "#!/bin/sh\necho demo\n").unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let original_payload_id = payload.payload_id.clone();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let discovery = manager
            .discover(EnvironmentRef::Native, "source-fingerprint")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                "skills/demo",
                payload,
                PayloadPlanningMetadata {
                    skill_name: "demo".to_string(),
                    install_dir_name: "demo".to_string(),
                    source: "owner/repo".to_string(),
                    source_type: "github".to_string(),
                    source_url: Some("https://github.com/owner/repo.git".to_string()),
                    ref_name: Some("main".to_string()),
                    skill_path: "skills/demo".to_string(),
                    plugin_name: None,
                    computed_hash: "canonical-computed-hash".to_string(),
                    upstream_revision: Some("canonical-tree-hash".to_string()),
                    well_known: None,
                },
            )
            .await
            .unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "project-1".to_string(),
            },
        };
        let facts = ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: Some(crate::environment::types::RegisteredProject {
                    id: "project-1".to_string(),
                    native_path: project.to_string_lossy().into_owned(),
                    display_name: None,
                    order: None,
                    suppress_cross_storage_warning: false,
                }),
                home: locator(temp.path()),
                skill_root: locator(&canonical_root),
                lock: locator(&project.join("skills-lock.json")),
            },
            agent_runtime: eve_runtime(project.to_string_lossy().as_ref()),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-eve").unwrap(),
            },
            lock_schema: LockSchema::Project,
            lock_document: LosslessLockDocument::empty(LockSchema::Project),
            eve_targets: vec![InstallTargetInfo {
                target_id: "eve:root".to_string(),
                agent: AgentId::parse("eve").unwrap(),
                display_name: "Eve (root)".to_string(),
                subagent: None,
                path: project.join("agent/skills").to_string_lossy().into_owned(),
            }],
        };
        let target_resolver = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));
        let agent_selection = test_submission_for_agents(
            &context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &facts.resolved_context.skill_root,
            &target_resolver,
            &["eve"],
            InstallMode::Symlink,
        )
        .await;
        let planner = ConcreteInstallPlanner::new(
            Facts(facts),
            target_resolver,
            Arc::clone(&manager),
            || "2026-07-18T00:00:00.000Z".to_string(),
            Arc::new(EmptyLibraryCandidateSource),
        );
        let request = InstallRequest {
            context,
            source: "owner/repo".to_string(),
            discovery_session: discovery,
            payloads: vec![handle.clone()],
            skills: vec!["demo".to_string()],
            agent_selection,
            acknowledge_redirect: true,
        };

        let preview = planner
            .preview(
                InstallOperation::Install,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await
            .unwrap();
        let (token, plan) = planner
            .rebuild(
                InstallOperation::Install,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await
            .unwrap();

        let InstallPreviewOutcome::Ready { preview } = preview else {
            panic!("expected ready preview");
        };
        assert_eq!(preview.token, token);
        assert_eq!(plan.payloads.len(), 2);
        let unit = &plan.units[0];
        assert_eq!(
            unit.lock_mutation
                .as_ref()
                .and_then(PreparedLockMutation::replacement)
                .and_then(|entry| entry.get("subagents")),
            None
        );
        let canonical = unit.primary_entry.as_ref().unwrap();
        let eve = &unit.additional_entries[0];
        let PreparedEntryAction::Replace {
            payload_id: canonical_id,
            ..
        } = &canonical.action
        else {
            panic!("canonical install must replace")
        };
        let PreparedEntryAction::Replace {
            payload_id: eve_id, ..
        } = &eve.action
        else {
            panic!("Eve install must replace")
        };
        assert_eq!(canonical_id, &original_payload_id);
        assert_ne!(eve_id, canonical_id);
        let original_payload = plan
            .payloads
            .get(canonical_id)
            .unwrap()
            .load_payload()
            .await
            .unwrap();
        let eve_payload = plan
            .payloads
            .get(eve_id)
            .unwrap()
            .load_payload()
            .await
            .unwrap();
        assert!(payload_skill_md(&original_payload).contains("name: demo"));
        assert!(!payload_skill_md(&eve_payload).contains("name: demo"));
        let lock = unit.lock_mutation.as_ref().unwrap();
        assert_eq!(
            lock.replacement().unwrap()["computedHash"],
            "canonical-computed-hash"
        );
        assert_eq!(
            lock.replacement().unwrap()["remoteHash"],
            "canonical-tree-hash"
        );
    }

    #[test]
    fn eve_lock_placement_matches_cli_root_and_named_target_semantics() {
        let temp = tempdir().unwrap();
        let project = temp.path().join("project");
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "project-1".to_string(),
            },
        };
        let facts = ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator(temp.path()),
                skill_root: locator(&project.join(".agents/skills")),
                lock: locator(&project.join("skills-lock.json")),
            },
            agent_runtime: eve_runtime(project.to_string_lossy().as_ref()),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-eve").unwrap(),
            },
            lock_schema: LockSchema::Project,
            lock_document: LosslessLockDocument::empty(LockSchema::Project),
            eve_targets: Vec::new(),
        };
        let metadata = PayloadPlanningMetadata {
            skill_name: "demo".to_string(),
            install_dir_name: "demo".to_string(),
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo.git".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: "skills/demo".to_string(),
            plugin_name: None,
            computed_hash: "computed".to_string(),
            upstream_revision: Some("remote".to_string()),
            well_known: None,
        };
        let no_targets = BTreeSet::new();
        let no_targets_mutation =
            lock_mutation(&facts, &metadata, &no_targets, "now".to_string()).unwrap();
        let no_targets = no_targets_mutation.replacement().unwrap();
        assert!(!no_targets.as_object().unwrap().contains_key("subagents"));

        let root_only = BTreeSet::from(["".to_string()]);
        let root_only_mutation =
            lock_mutation(&facts, &metadata, &root_only, "now".to_string()).unwrap();
        let root_only = root_only_mutation.replacement().unwrap();
        assert!(!root_only.as_object().unwrap().contains_key("subagents"));

        let named_and_root = BTreeSet::from(["".to_string(), "builder".to_string()]);
        let named_and_root_mutation =
            lock_mutation(&facts, &metadata, &named_and_root, "now".to_string()).unwrap();
        let named_and_root = named_and_root_mutation.replacement().unwrap();
        assert_eq!(named_and_root["subagents"], json!(["", "builder"]));
    }

    #[tokio::test]
    async fn direct_install_keeps_unselected_private_library_links() {
        let temp = tempdir().unwrap();
        let physical_root = temp.path().to_path_buf();
        let canonical_root = physical_root.join(".agents/skills");
        fs::create_dir_all(&canonical_root).unwrap();
        let source = physical_root.join("source/demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\nbody",
        )
        .unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let discovery = manager
            .discover(EnvironmentRef::Native, "source-fingerprint")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                "skills/demo",
                payload,
                PayloadPlanningMetadata {
                    skill_name: "demo".to_string(),
                    install_dir_name: "demo".to_string(),
                    source: "owner/repo".to_string(),
                    source_type: "github".to_string(),
                    source_url: Some("https://github.com/owner/repo.git".to_string()),
                    ref_name: Some("main".to_string()),
                    skill_path: "skills/demo".to_string(),
                    plugin_name: None,
                    computed_hash: "computed-hash".to_string(),
                    upstream_revision: Some("remote-hash".to_string()),
                    well_known: None,
                },
            )
            .await
            .unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let private_root = physical_root.join(".custom/skills");
        let facts = ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator(&physical_root),
                skill_root: locator(&canonical_root),
                lock: locator(&physical_root.join(".agents/.skill-lock.json")),
            },
            agent_runtime: runtime(private_root.to_string_lossy().as_ref()),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-library-election").unwrap(),
            },
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::empty(LockSchema::Global),
            eve_targets: Vec::new(),
        };
        let target_resolver = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));
        let library_target = physical_root.join("libraries/lib-1/skills/demo");
        fs::create_dir_all(&library_target).unwrap();
        fs::create_dir_all(&private_root).unwrap();
        create_directory_link(&library_target, &private_root.join("demo"));
        let agent_selection = test_submission_for_agents(
            &context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &facts.resolved_context.skill_root,
            &target_resolver,
            &[],
            InstallMode::Symlink,
        )
        .await;
        let candidate = crate::application::library_candidates::LibraryVersionCandidate::new(
            crate::application::skill_libraries::LibraryId::parse("lib-1"),
            "demo",
            locator(&library_target),
        );
        let library_candidates =
            crate::application::library_candidates::LibraryCandidateSnapshot::new(
                "library-evidence-1",
                vec![AgentId::parse("custom-private").unwrap()],
                crate::application::library_candidates::LibraryCandidateSet::new(
                    vec![candidate.clone()],
                    vec![candidate],
                )
                .unwrap(),
            )
            .unwrap();
        let planner = ConcreteInstallPlanner::new(
            Facts(facts),
            target_resolver,
            Arc::clone(&manager),
            || "2026-08-30T00:00:00.000Z".to_string(),
            Arc::new(FixedCandidates(library_candidates)),
        );
        let request = InstallRequest {
            context,
            source: "owner/repo".to_string(),
            discovery_session: discovery,
            payloads: vec![handle.clone()],
            skills: vec!["demo".to_string()],
            agent_selection,
            acknowledge_redirect: true,
        };

        let (_, plan) = planner
            .rebuild(
                InstallOperation::Install,
                &request,
                vec![manager.pin_verified(&handle).await.unwrap()],
            )
            .await
            .unwrap();

        let expected_private = fs::canonicalize(&private_root)
            .expect("canonical private root")
            .join("demo");
        let private = plan.units[0]
            .additional_entries
            .iter()
            .find(|entry| std::path::Path::new(&entry.destination.native_path) == expected_private)
            .expect("library-eligible private placement");
        assert_eq!(private.action, PreparedEntryAction::Keep);
    }

    #[test]
    fn well_known_lock_keeps_the_cli_base_url_in_both_schemas() {
        let temp = tempdir().unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let facts = ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator(temp.path()),
                skill_root: locator(&temp.path().join(".agents/skills")),
                lock: locator(&temp.path().join(".agents/.skill-lock.json")),
            },
            agent_runtime: runtime(temp.path().to_string_lossy().as_ref()),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-well-known").unwrap(),
            },
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::empty(LockSchema::Global),
            eve_targets: Vec::new(),
        };
        let metadata = PayloadPlanningMetadata {
            skill_name: "demo".to_string(),
            install_dir_name: "demo".to_string(),
            source: "skills.example.com".to_string(),
            source_type: "well-known".to_string(),
            source_url: Some("https://skills.example.com/catalog".to_string()),
            ref_name: None,
            skill_path: "demo".to_string(),
            plugin_name: None,
            computed_hash: "computed".to_string(),
            upstream_revision: None,
            well_known: Some(WellKnownPlanningMetadata {
                artifact_url: "https://cdn.example.com/demo.tar.gz".to_string(),
                digest: format!("sha256:{}", "a".repeat(64)),
            }),
        };
        let eve_subagents = BTreeSet::new();

        let global = lock_mutation(&facts, &metadata, &eve_subagents, "now".to_string())
            .unwrap()
            .replacement()
            .unwrap()
            .clone();
        assert_eq!(
            global["sourceBaseUrl"],
            "https://skills.example.com/catalog"
        );
        assert_eq!(global["sourceUrl"], "https://cdn.example.com/demo.tar.gz");

        let mut project_facts = facts;
        project_facts.lock_schema = LockSchema::Project;
        project_facts.lock_document = LosslessLockDocument::empty(LockSchema::Project);
        let project = lock_mutation(&project_facts, &metadata, &eve_subagents, "now".to_string())
            .unwrap()
            .replacement()
            .unwrap()
            .clone();
        assert_eq!(project["sourceUrl"], "https://skills.example.com/catalog");
    }

    fn payload_skill_md(payload: &crate::core::skill_payload::SkillPayload) -> &str {
        let entry = payload
            .entries
            .iter()
            .find(|entry| entry.relative_path == "SKILL.md")
            .unwrap();
        std::str::from_utf8(
            payload
                .blobs
                .get(entry.blob_id.as_deref().unwrap())
                .unwrap(),
        )
        .unwrap()
    }

    fn locator(path: &std::path::Path) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string_lossy().into_owned(),
        }
    }

    fn create_directory_link(target: &std::path::Path, link: &std::path::Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        junction::create(target, link).unwrap();
    }

    fn runtime(private_root: &str) -> AgentRuntimeSnapshot {
        let id = AgentId::parse("custom-private").unwrap();
        let scope = ResolvedAgentScope {
            enabled: true,
            reads_standard: false,
            standard_path: None,
            private_path: Some(private_root.to_string()),
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        AgentRuntimeSnapshot {
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: None,
            agents: BTreeMap::from([(
                id.clone(),
                ResolvedAgent {
                    definition: AgentDefinition {
                        id,
                        display_name: "Custom Private".to_string(),
                        source: AgentSource::Custom,
                        aliases: Vec::new(),
                        global: ScopeDefinition {
                            enabled: true,
                            reads_standard: false,
                            private_path: Some(PathSpec::home(".custom/skills")),
                        },
                        project: ScopeDefinition {
                            enabled: false,
                            reads_standard: false,
                            private_path: None,
                        },
                        detection: DetectionSpec::AnyPathExists {
                            paths: vec![PathSpec::home(".custom")],
                        },
                        legacy_paths: Vec::new(),
                        adapter: AgentAdapter::Standard,
                    },
                    detection: DetectionState::Detected,
                    detection_reason: None,
                    global: scope.clone(),
                    project: scope,
                },
            )]),
        }
    }

    fn eve_runtime(project_path: &str) -> AgentRuntimeSnapshot {
        let id = AgentId::parse("eve").unwrap();
        let disabled = ResolvedAgentScope {
            enabled: false,
            reads_standard: false,
            standard_path: None,
            private_path: None,
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        let project = ResolvedAgentScope {
            enabled: true,
            reads_standard: false,
            standard_path: None,
            private_path: None,
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        AgentRuntimeSnapshot {
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: Some(project_path.to_string()),
            agents: BTreeMap::from([(
                id.clone(),
                ResolvedAgent {
                    definition: AgentDefinition {
                        id,
                        display_name: "Eve".to_string(),
                        source: AgentSource::Builtin,
                        aliases: Vec::new(),
                        global: ScopeDefinition {
                            enabled: false,
                            reads_standard: false,
                            private_path: None,
                        },
                        project: ScopeDefinition {
                            enabled: true,
                            reads_standard: false,
                            private_path: None,
                        },
                        detection: DetectionSpec::AnyPathExists {
                            paths: vec![
                                PathSpec::project("agent"),
                                PathSpec::project("package.json"),
                            ],
                        },
                        legacy_paths: Vec::new(),
                        adapter: AgentAdapter::Eve,
                    },
                    detection: DetectionState::Detected,
                    detection_reason: None,
                    global: disabled,
                    project,
                },
            )]),
        }
    }
}
