use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use crate::application::git_transport::UnavailableGitSourceTransport;
use crate::application::resources::SkillIdentity;
use crate::application::source_acquisition::{
    AcquireSelectedPayloadsRequest, SelectedPayloadAcquisitionService,
};
use crate::application::source_evidence::SourceEvidenceCoordinator;
use crate::application::source_evidence_provider::RuntimeSourceEvidenceDetector;
use crate::application::source_snapshot_reuse::SourceSnapshotReuseIndex;
use crate::application::update::{
    SkillUpdateCheckStatus, UpdateCheckMode, UpdateCheckRequest, UpdateCheckSelection,
};
use crate::application::update_check::UpdateCheckService;
use crate::application::update_records::InstalledUpdateRecordProvider;
use crate::application::wellknown_access::UnavailableWellKnownAccess;
use crate::application::wsl_source_access::UnavailableWslSourceAccess;
use crate::runtime::download::RuntimeDownloadAccess;
use crate::runtime::source_acquisition::SourceDiscoveryService;
use crate::runtime::update_service::RuntimeSkillSourceModule;

pub(crate) struct DownloadFixture {
    pub(crate) url: String,
    pub(crate) body: Arc<Mutex<Vec<u8>>>,
    pub(crate) requests: Arc<AtomicUsize>,
    stopped: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl DownloadFixture {
    pub(crate) fn new(archive: bool, redirected: bool) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/{}",
            server.server_addr(),
            if archive { "bundle.zip" } else { "SKILL.md" }
        );
        let actual = format!(
            "http://localhost:{}/content",
            server.server_addr().to_ip().unwrap().port()
        );
        let body = Arc::new(Mutex::new(content(archive, "old", "v1")));
        let requests = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let (bytes, count, stop) = (body.clone(), requests.clone(), stopped.clone());
        let worker = thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if let Some(request) = server.recv_timeout(Duration::from_millis(100)).unwrap() {
                    count.fetch_add(1, Ordering::SeqCst);
                    if redirected && request.url() != "/content" {
                        request
                            .respond(
                                tiny_http::Response::empty(302).with_header(
                                    tiny_http::Header::from_bytes("Location", actual.as_bytes())
                                        .unwrap(),
                                ),
                            )
                            .unwrap();
                    } else {
                        request
                            .respond(tiny_http::Response::from_data(
                                bytes.lock().unwrap().clone(),
                            ))
                            .unwrap();
                    }
                }
            }
        });
        Self {
            url,
            body,
            requests,
            stopped,
            worker: Some(worker),
        }
    }
}

impl Drop for DownloadFixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) fn markdown(version: &str) -> String {
    format!("---\nname: demo\ndescription: Maintained download\n---\n{version}\n")
}

pub(crate) fn content(archive: bool, directory: &str, version: &str) -> Vec<u8> {
    if !archive {
        return markdown(version).into_bytes();
    }
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in [
        ("SKILL.md", markdown(version)),
        ("metadata.json", "{\"source\":true}".into()),
        ("a-b.txt", "dash".into()),
        ("a_b.txt", "underscore".into()),
    ] {
        zip.start_file(
            format!("{directory}/demo/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(&mut zip, content.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[tokio::test]
async fn native_download_install_check_preview_and_update_preserve_scope_and_source(
) -> Result<(), AppError> {
    for global in [true, false] {
        for archive in [false, true] {
            for redirected in [false, true] {
                verify_download_workflow(global, archive, redirected).await?;
            }
        }
    }
    Ok(())
}

async fn verify_download_workflow(
    global: bool,
    archive: bool,
    redirected: bool,
) -> Result<(), AppError> {
    let temp = tempfile::tempdir()?;
    let project_path = temp.path().join("project");
    fs::create_dir_all(&project_path)?;
    fs::create_dir_all(temp.path().join("home"))?;
    fs::create_dir_all(temp.path().join("config"))?;
    let projects_path = temp.path().join("projects.json");
    write_json(
        &projects_path,
        &json!({ "schemaVersion": 1, "projects": [project("source", &project_path, "Source")] }),
    )?;
    let environment = Arc::new(WslRuntime::default());
    let facts = RuntimePlanningFactSource::with_native_snapshot(
        Arc::new(StaticRegistry(Arc::new(test_registry()))),
        environment.clone(),
        NativeRuntimeSnapshot {
            home: temp.path().join("home"),
            config_home: temp.path().join("config"),
            projects_path,
            global_lock_path: temp.path().join("global-lock.json"),
            environment_variables: BTreeMap::new(),
        },
    );
    let context = if global {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    } else {
        project_context("source")
    };
    let targets = RuntimeTargetFactResolver::new(environment.clone());
    let payloads = Arc::new(PayloadSessionManager::in_memory(
        PayloadSessionLimits {
            ttl_ms: 60_000,
            max_sessions: 12,
            max_bytes: 8 * 1024 * 1024,
        },
        || 1_000,
    ));
    let execution =
        RuntimeExecutionDependencies::new(environment.clone(), temp.path().join("recovery"))?;
    let remote = DownloadFixture::new(archive, redirected);
    let transport: Arc<dyn crate::application::git_transport::GitSourceTransport> =
        Arc::new(UnavailableGitSourceTransport);
    let fetched = SourceDiscoveryService::new(
        payloads.clone(),
        transport.clone(),
        Arc::new(UnavailableWellKnownAccess),
        RuntimeDownloadAccess::for_test(),
        Arc::new(UnavailableWslSourceAccess),
    )
    .discover(EnvironmentRef::Native, remote.url.clone(), |_| {})
    .await?;
    assert_eq!(
        fetched.redirected_download_host.as_deref(),
        redirected.then_some("localhost")
    );
    let handles = SelectedPayloadAcquisitionService::new(payloads.clone())
        .acquire(AcquireSelectedPayloadsRequest {
            discovery_session: fetched.discovery_session.clone(),
            skill_paths: fetched
                .skills
                .iter()
                .map(|skill| skill.relative_path.clone())
                .collect(),
        })
        .await?;
    let scope = ScopePlanningSnapshotSource::snapshot(&facts, &context).await?;
    let root = PathBuf::from(&scope.resolved_context.skill_root.native_path);
    let lock_path = PathBuf::from(&scope.resolved_context.lock.native_path);
    let selection = test_submission_for_agents_and_own_directories(
        &context,
        &scope.agent_runtime,
        &scope.eve_targets,
        &scope.resolved_context.skill_root,
        &targets,
        &[],
        InstallMode::Copy,
    )
    .await;
    let installer = InstallService::new(
        payloads.clone(),
        ConcreteInstallPlanner::new(
            facts.clone(),
            targets.clone(),
            payloads.clone(),
            fixed_time,
            Arc::new(EmptyLibraryCandidateSource),
        ),
        executor(&execution, &environment, &facts),
    );
    let install_request = InstallRequest {
        context: context.clone(),
        source: remote.url.clone(),
        discovery_session: fetched.discovery_session,
        payloads: handles,
        skills: vec!["demo".into()],
        agent_selection: selection,
        acknowledge_redirect: redirected,
    };
    let InstallPreviewOutcome::Ready { preview } = installer.preview(&install_request).await?
    else {
        panic!("ready download installation");
    };
    let installed = installer
        .execute(
            &install_request,
            preview.token,
            CancellationSignal::default(),
        )
        .await?;
    assert_eq!(
        installed.units[0].status,
        MutationUnitStatus::Succeeded,
        "global={global}, archive={archive}: {:?}",
        installed.units[0]
    );
    assert!(installed.units[0].lock_committed);
    let mut lock: Value = serde_json::from_slice(&fs::read(&lock_path)?)?;
    lock["skills"]["demo"]["externalField"] = json!("preserved");
    write_json(&lock_path, &lock)?;
    assert_eq!(lock["skills"]["demo"]["sourceUrl"], remote.url);
    let initial_requests = remote.requests.load(Ordering::SeqCst);

    let snapshots = Arc::new(SourceSnapshotReuseIndex::default());
    let evidence = SourceEvidenceCoordinator::new(Arc::new(
        RuntimeSourceEvidenceDetector::with_git_transport(
            payloads.clone(),
            snapshots.clone(),
            transport.clone(),
        ),
    ));
    let libraries = update_library_repository(temp.path());
    let checks = UpdateCheckService::new(
        InstalledUpdateRecordProvider::new(facts.clone(), targets.clone(), libraries.clone()),
        evidence.clone(),
    );
    let mut check = UpdateCheckRequest {
        context: context.clone(),
        mode: UpdateCheckMode::Automatic,
        selection: UpdateCheckSelection::Skills(vec![SkillIdentity {
            context: context.clone(),
            skill_name: "demo".into(),
        }]),
    };
    let skipped = checks.check(&check).await?;
    assert_eq!(skipped.skipped.len(), 1);
    assert!(skipped.skills.is_empty());
    assert_eq!(remote.requests.load(Ordering::SeqCst), initial_requests);
    // ZIP 场景仅移动来源路径，原始文件场景改变内容。
    let expected_version = if archive { "v1" } else { "v2" };
    *remote.body.lock().unwrap() = content(archive, "new", expected_version);
    check.mode = UpdateCheckMode::Force;
    let checked = checks.check(&check).await?;
    assert_eq!(
        checked.skills[0].status,
        SkillUpdateCheckStatus::UpdateAvailable,
        "global={global}, archive={archive}: {checked:?}"
    );
    assert_eq!(checked.skills[0].path_change.is_some(), archive);
    let source = RuntimeSkillSourceModule::with_git_transport(
        payloads.clone(),
        snapshots,
        evidence,
        transport,
    );
    let updates = UpdateService::new(
        payloads.clone(),
        ConcreteUpdatePlanner::new(facts.clone(), targets, payloads, libraries, fixed_time),
        source,
        executor(&execution, &environment, &facts),
    );
    let prepared = updates
        .prepare(
            &UpdateRequest {
                context,
                skill_names: vec!["demo".into()],
            },
            CancellationSignal::default(),
        )
        .await?;
    assert!(
        prepared.preview.blocked.is_empty(),
        "{:?}",
        prepared.preview.blocked
    );
    if redirected {
        let route = prepared.preview.sources[0]
            .download_redirect
            .as_ref()
            .unwrap();
        assert_eq!(route.original_host, "127.0.0.1");
        assert_eq!(route.actual_host, "localhost");
    }
    assert_eq!(
        remote.requests.load(Ordering::SeqCst),
        initial_requests + if redirected { 2 } else { 1 }
    );
    *remote.body.lock().unwrap() = content(archive, "future", "v3");
    let updated = updates
        .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
        .await?;
    assert_eq!(
        updated.skills[0].mutation.as_ref().unwrap().status,
        MutationUnitStatus::Succeeded
    );
    assert_eq!(
        fs::read_to_string(root.join("demo/SKILL.md"))?,
        markdown(expected_version)
    );
    assert_eq!(
        remote.requests.load(Ordering::SeqCst),
        initial_requests + if redirected { 2 } else { 1 }
    );
    let lock: Value = serde_json::from_slice(&fs::read(lock_path)?)?;
    assert_eq!(lock["skills"]["demo"]["externalField"], "preserved");
    if archive {
        assert_eq!(lock["skills"]["demo"]["skillPath"], "new/demo/SKILL.md");
    }
    *remote.body.lock().unwrap() = content(archive, "new", expected_version);
    assert_eq!(
        checks.check(&check).await?.skills[0].status,
        SkillUpdateCheckStatus::UpToDate
    );
    Ok(())
}
