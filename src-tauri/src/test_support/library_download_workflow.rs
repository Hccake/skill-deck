use super::*;
use std::sync::atomic::Ordering;

use crate::application::git_transport::UnavailableGitSourceTransport;
use crate::application::library_application::{
    ApplicationRegistry, ApplyLibraryApplicationRequest, LibraryApplicationDraft,
    LibraryApplicationModule,
};
use crate::application::library_membership::LibraryMembershipModule;
use crate::application::payload_session::PayloadSessionLimits;
use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::application::skill_libraries::SkillLibraryRepository;
use crate::application::skill_libraries::{
    ExecuteAddLibrarySkillsRequest, LibraryAddSkillStatus, PreviewAddLibrarySkillItem,
    PreviewAddLibrarySkillsRequest,
};
use crate::application::source_acquisition::{
    AcquireSelectedPayloadsRequest, SelectedPayloadAcquisitionService,
};
use crate::application::source_evidence::SourceEvidenceCoordinator;
use crate::application::source_evidence_provider::RuntimeSourceEvidenceDetector;
use crate::application::source_snapshot_reuse::SourceSnapshotReuseIndex;
use crate::application::update::{SkillUpdateCheckStatus, UpdateCheckMode};
use crate::application::update_check::LibraryUpdateCheckService;
use crate::application::update_records::LibraryUpdateRecordProvider;
use crate::application::update_subjects::LibraryUpdateSubjectProvider;
use crate::application::wellknown_access::UnavailableWellKnownAccess;
use crate::application::wsl_source_access::UnavailableWslSourceAccess;
use crate::core::projects::{ProjectMigrationRegistry, ProjectMigrationState};
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
use crate::environment::wsl::WslRuntime;
use crate::native_workflow_integration_support::{
    download_maintenance::{content, markdown, DownloadFixture},
    test_registry, StaticRegistry,
};
use crate::runtime::download::RuntimeDownloadAccess;
use crate::runtime::plan_runner::RuntimeExecutionDependencies;
use crate::runtime::planning_facts::{NativeRuntimeSnapshot, RuntimePlanningFactSource};
use crate::runtime::skill_libraries::RuntimeSkillLibraryRepository;
use crate::runtime::source_acquisition::SourceDiscoveryService;
use crate::runtime::update_service::RuntimeSkillSourceModule;

#[tokio::test]
async fn library_download_add_check_and_update_keep_existing_links_and_member_identity(
) -> Result<(), AppError> {
    for archive in [false, true] {
        for redirected in [false, true] {
            verify_library_download(archive, redirected, false).await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn library_download_one_time_import_requires_consent_and_stores_only_origin(
) -> Result<(), AppError> {
    for redirected in [false, true] {
        verify_library_download(false, redirected, true).await?;
    }
    Ok(())
}

async fn verify_library_download(
    archive: bool,
    redirected: bool,
    one_time: bool,
) -> Result<(), AppError> {
    let temp = tempfile::tempdir()?;
    let runtime = Arc::new(WslRuntime::default());
    let repository = Arc::new(RuntimeSkillLibraryRepository::new(
        temp.path().join("libraries"),
        runtime.clone(),
        Arc::new(ProjectMigrationRegistry::new(
            ProjectMigrationState::NotNeeded,
        )),
    ));
    let libraries = Arc::new(SkillLibraryModule::new(repository.clone()));
    let environment = EnvironmentRef::Native;
    let id = libraries
        .create(environment.clone(), "Downloads".into())
        .await?
        .libraries[0]
        .id
        .clone();
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(&config_home)?;
    let projects_path = temp.path().join("projects.json");
    std::fs::write(&projects_path, br#"{"schemaVersion":1,"projects":[]}"#)?;
    let facts = RuntimePlanningFactSource::with_native_snapshot(
        Arc::new(StaticRegistry(Arc::new(test_registry()))),
        runtime.clone(),
        NativeRuntimeSnapshot {
            home: home.clone(),
            config_home,
            projects_path,
            global_lock_path: temp.path().join("global-lock.json"),
            environment_variables: BTreeMap::new(),
        },
    );
    let targets = RuntimeTargetFactResolver::new(runtime.clone());
    let execution =
        RuntimeExecutionDependencies::new(runtime.clone(), temp.path().join("recovery"))?;
    let applications = Arc::new(LibraryApplicationModule::new(
        repository.clone(),
        facts.clone(),
        targets.clone(),
        execution.executor(runtime, Arc::new(facts)),
    ));
    let context = SkillLocationRef {
        environment: environment.clone(),
        scope: SkillLocation::Global,
    };
    let draft = LibraryApplicationDraft {
        context: context.clone(),
        ordered_library_ids: vec![id.clone()],
        selected_agent_ids: vec![],
    };
    let application_preview = applications.preview(draft.clone()).await?;
    applications
        .apply(
            ApplyLibraryApplicationRequest {
                draft,
                expected_token: application_preview.token,
            },
            CancellationSignal::default(),
        )
        .await?;
    let payloads = Arc::new(PayloadSessionManager::in_memory(
        PayloadSessionLimits {
            ttl_ms: 60_000,
            max_sessions: 12,
            max_bytes: 8 * 1024 * 1024,
        },
        || 1_000,
    ));
    let remote = DownloadFixture::new(archive, redirected);
    *remote.body.lock().unwrap() = library_content(archive, "old", "v1", true);
    let url = if one_time {
        format!("{}?token=private-secret", remote.url)
    } else {
        remote.url.clone()
    };
    let transport = Arc::new(UnavailableGitSourceTransport);
    let discovered = SourceDiscoveryService::new(
        payloads.clone(),
        transport.clone(),
        Arc::new(UnavailableWellKnownAccess),
        RuntimeDownloadAccess::for_test(),
        Arc::new(UnavailableWslSourceAccess),
    )
    .discover(environment.clone(), url.clone(), |_| {})
    .await?;
    let acquired = SelectedPayloadAcquisitionService::new(payloads.clone())
        .acquire(AcquireSelectedPayloadsRequest {
            discovery_session: discovered.discovery_session.clone(),
            skill_paths: discovered
                .skills
                .iter()
                .map(|skill| skill.relative_path.clone())
                .collect(),
        })
        .await?;
    let names = discovered
        .skills
        .iter()
        .map(|skill| skill.name.clone())
        .collect::<Vec<_>>();
    let request = PreviewAddLibrarySkillsRequest {
        environment: environment.clone(),
        library_id: id.clone(),
        discovery_session: discovered.discovery_session,
        skills: acquired
            .into_iter()
            .map(|payload| PreviewAddLibrarySkillItem {
                skill_name: discovered
                    .skills
                    .iter()
                    .find(|skill| skill.relative_path == payload.skill_path)
                    .unwrap()
                    .name
                    .clone(),
                payload,
            })
            .collect(),
    };
    let membership = LibraryMembershipModule::new(
        repository.clone(),
        applications,
        repository.clone(),
        Arc::new(RuntimeAdmissionCoordinator::default()),
        libraries.clone(),
        payloads.clone(),
        Arc::new(targets.clone()),
    );
    let preview = membership.preview_add_skills(request.clone()).await?;
    assert_eq!(preview.membership.scopes, vec![context.clone()]);
    let execution = ExecuteAddLibrarySkillsRequest {
        request,
        expected_token: preview.token,
        membership: preview.membership,
        acknowledge_redirect: false,
    };
    if redirected {
        assert!(matches!(
            membership
                .execute_add_skills(execution.clone(), CancellationSignal::default())
                .await,
            Err(AppError::DirectDownloadRedirectConfirmationRequired { .. })
        ));
        assert!(repository.load(&environment).await?.libraries[0]
            .skills
            .is_empty());
    }
    let added = membership
        .execute_add_skills(
            ExecuteAddLibrarySkillsRequest {
                acknowledge_redirect: redirected,
                ..execution
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_eq!(
        added.results[0].status,
        LibraryAddSkillStatus::Succeeded,
        "{:?}",
        added.results
    );
    assert_eq!(added.membership.scopes.len(), 1, "{added:?}");
    let applied = home.join(".agents/skills/demo");
    assert_eq!(
        std::fs::read_to_string(applied.join("SKILL.md"))?,
        markdown("v1")
    );
    let application_before = repository.load_application(&context).await?;
    let mut stored = repository.load(&environment).await?;
    let record = &stored.libraries[0].skills[0].source_record;
    assert!(record["artifactUrl"].is_null());
    assert!(record["refName"].is_null());
    assert!(record["installedRevision"].is_null());
    let details = libraries.detail(environment.clone(), id.clone()).await?;
    let snapshots = Arc::new(SourceSnapshotReuseIndex::default());
    let evidence = SourceEvidenceCoordinator::new(Arc::new(
        RuntimeSourceEvidenceDetector::with_git_transport(
            payloads.clone(),
            snapshots.clone(),
            transport.clone(),
        ),
    ));
    let checks = LibraryUpdateCheckService::new(
        LibraryUpdateRecordProvider::new(repository.clone()),
        evidence.clone(),
    );
    if one_time {
        let serialized = serde_json::to_string(record)?;
        assert!(!serialized.contains("private-secret"));
        assert!(record["reacquisitionUrl"].is_null());
        assert!(details.skills[0].source_url.is_none());
        assert!(
            !details.skills[0]
                .update_capability
                .as_ref()
                .unwrap()
                .can_run_update
        );
        let requests = remote.requests.load(Ordering::SeqCst);
        let result = checks
            .check(
                environment,
                id,
                UpdateCheckMode::Force,
                names.iter().cloned().collect(),
            )
            .await?;
        assert!(result.skills.is_empty());
        assert_eq!(remote.requests.load(Ordering::SeqCst), requests);
        return Ok(());
    }
    assert_eq!(record["reacquisitionUrl"], url);
    assert_eq!(record["computedHash"].as_str().unwrap().len(), 64);
    stored.libraries[0].skills[0].source_record["vendorField"] = serde_json::json!("preserved");
    repository.save(&environment, &stored).await?;
    let root = repository.resolve_collection(&environment, &id).await?.root;
    let link_before = std::fs::read_link(&applied)?;
    let requests = remote.requests.load(Ordering::SeqCst);
    assert!(checks
        .check(
            environment.clone(),
            id.clone(),
            UpdateCheckMode::Automatic,
            names.iter().cloned().collect()
        )
        .await?
        .skills
        .is_empty());
    assert_eq!(remote.requests.load(Ordering::SeqCst), requests);
    *remote.body.lock().unwrap() = library_content(archive, "new", "v2", true);
    let checked = checks
        .check(
            environment.clone(),
            id.clone(),
            UpdateCheckMode::Force,
            names.iter().cloned().collect(),
        )
        .await?;
    assert_eq!(
        checked.skills[0].status,
        SkillUpdateCheckStatus::UpdateAvailable
    );
    let source = RuntimeSkillSourceModule::with_git_transport(
        payloads.clone(),
        snapshots,
        evidence,
        transport,
    );
    let updates = LibraryUpdateService::new(
        payloads,
        LibraryUpdateSubjectProvider::new(repository.clone(), targets.clone()),
        source,
        targets,
        libraries,
    );
    let prepared = updates
        .prepare(
            &UpdateLibrarySkillsRequest {
                environment: environment.clone(),
                library_id: id.clone(),
                skill_names: names.clone(),
            },
            CancellationSignal::default(),
        )
        .await?;
    assert!(
        prepared.preview.blocked.is_empty(),
        "{:?}",
        prepared.preview
    );
    assert_eq!(prepared.preview.skill_names.len(), names.len());
    assert_eq!(prepared.preview.path_changes.is_some(), archive);
    let mapping = serde_json::to_value(&prepared.preview)?;
    if redirected {
        assert_eq!(
            mapping["downloadRedirects"][0]["redirect"]["originalHost"],
            "127.0.0.1"
        );
        assert_eq!(
            mapping["downloadRedirects"][0]["redirect"]["actualHost"],
            "localhost"
        );
    }
    assert_eq!(
        remote.requests.load(Ordering::SeqCst),
        requests + if redirected { 2 } else { 1 }
    );
    // 预览已经固定 v2；随后上游变化不会改变本次提交内容。
    *remote.body.lock().unwrap() = library_content(archive, "new", "v3", true);
    let response = updates
        .execute_prepared(prepared, CancellationSignal::default(), |_| {})
        .await?;
    assert_eq!(
        response.results[0].status,
        LibraryUpdateSkillStatus::Succeeded,
        "{response:?}"
    );
    assert_eq!(std::fs::read_link(&applied)?, link_before);
    assert!(response.membership.scopes.is_empty());
    assert_eq!(
        serde_json::to_value(repository.load_application(&context).await?.record)?,
        serde_json::to_value(application_before.record)?
    );
    assert_eq!(
        std::fs::read_to_string(applied.join("SKILL.md"))?,
        markdown("v2")
    );
    let after = repository.load(&environment).await?;
    assert_eq!(after.libraries[0].id, id);
    assert_eq!(after.libraries[0].skills[0].name, "demo");
    assert_eq!(
        after.libraries[0].skills[0].source_record["vendorField"],
        "preserved"
    );
    assert_eq!(
        repository.resolve_collection(&environment, &id).await?.root,
        root
    );
    *remote.body.lock().unwrap() = library_content(archive, "new", "v2", true);
    let latest = checks
        .check(
            environment.clone(),
            id.clone(),
            UpdateCheckMode::Force,
            names.iter().cloned().collect(),
        )
        .await?;
    assert!(latest
        .skills
        .iter()
        .all(|skill| skill.status == SkillUpdateCheckStatus::UpToDate));
    if archive {
        let mut catalog = repository.load(&environment).await?;
        catalog.libraries[0]
            .skills
            .iter_mut()
            .find(|skill| skill.name == "helper")
            .unwrap()
            .source_record["computedHash"] = serde_json::Value::Null;
        repository.save(&environment, &catalog).await?;
        let checked = checks
            .check(
                environment.clone(),
                id.clone(),
                UpdateCheckMode::Force,
                names.iter().cloned().collect(),
            )
            .await?;
        let helper = checked
            .skills
            .iter()
            .find(|skill| skill.name == "helper")
            .unwrap();
        assert_eq!(
            helper.reason,
            Some(crate::application::update::UpdateCheckReasonCode::MissingRemoteHash)
        );
        assert!(helper.capability.can_run_update);
        let request = UpdateLibrarySkillsRequest {
            environment: environment.clone(),
            library_id: id.clone(),
            skill_names: names.clone(),
        };
        let prepared = updates
            .prepare(&request, CancellationSignal::default())
            .await?;
        assert!(prepared.preview.blocked.is_empty());
        let result = updates
            .execute_prepared(prepared, CancellationSignal::default(), |_| {})
            .await?;
        assert!(result
            .results
            .iter()
            .all(|item| item.status == LibraryUpdateSkillStatus::Succeeded));
        *remote.body.lock().unwrap() = library_content(true, "new", "v3", false);
        checks
            .check(
                environment.clone(),
                id.clone(),
                UpdateCheckMode::Force,
                names.iter().cloned().collect(),
            )
            .await?;
        let prepared = updates
            .prepare(&request, CancellationSignal::default())
            .await?;
        assert_eq!(prepared.preview.skill_names, vec!["demo".to_string()]);
        assert_eq!(prepared.preview.blocked[0].skill_name, "helper");
        let result = updates
            .execute_prepared(prepared, CancellationSignal::default(), |_| {})
            .await?;
        assert_eq!(result.outcome, UpdateOutcome::Partial);
        assert_eq!(
            result
                .results
                .iter()
                .find(|item| item.skill_name == "demo")
                .unwrap()
                .status,
            LibraryUpdateSkillStatus::Succeeded
        );
        assert_eq!(
            result
                .results
                .iter()
                .find(|item| item.skill_name == "helper")
                .unwrap()
                .status,
            LibraryUpdateSkillStatus::DeletedUpstream
        );
        assert_eq!(
            std::fs::read_to_string(applied.join("SKILL.md"))?,
            markdown("v3")
        );
    }
    Ok(())
}

fn library_content(archive: bool, directory: &str, version: &str, helper: bool) -> Vec<u8> {
    let bytes = content(archive, directory, version);
    if !archive || !helper {
        return bytes;
    }
    let mut zip = zip::ZipWriter::new_append(std::io::Cursor::new(bytes)).unwrap();
    zip.start_file(
        format!("{directory}/helper/SKILL.md"),
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    std::io::Write::write_all(
        &mut zip,
        markdown(version)
            .replace("name: demo", "name: helper")
            .as_bytes(),
    )
    .unwrap();
    zip.finish().unwrap().into_inner()
}
