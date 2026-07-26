#![cfg(all(target_os = "windows", feature = "wsl-integration-tests"))]
#![allow(
    clippy::disallowed_methods,
    reason = "WSL 集成测试需要直接终止并清理用后即弃的测试发行版"
)]

use std::process::Command;

use app_lib::wsl_integration_support::{
    build_wsl_exec_args,
    cli_lock_conflict_preserves_external_change as run_cli_lock_conflict_acceptance,
    connect_wsl_environment, decode_nul_records, discover_wsl_distributions,
    map_windows_path_with_wslpath,
    marker_before_batch_stage_failure_converges_after_reconnect as run_marker_before_stage_acceptance,
    reconnect_reindexes_recovery_and_sweeps_payloads, run_full_wsl_mutation_workflow,
    run_wsl_script, session_loss_invalidates_preview, wsl_unc_to_linux_path, AppError,
    CancellationSignal, EnvironmentLockIo, EnvironmentRef, ResourceLocator, WslExecutionFeature,
    WslOperationDescriptor, WslOperationExecutor, WslOperationRequest, WslSession,
    DEFAULT_WSL_STDERR_LIMIT,
};
use tempfile::tempdir;
use tokio::time::Duration;
use uuid::Uuid;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);
const MATERIALIZE_SCRIPT: &str = include_str!("../src/environment/wsl/scripts/materialize.sh");
const BATCH_STAGE_OPERATION: WslOperationDescriptor = WslOperationDescriptor {
    subcommand: "stage",
    script: MATERIALIZE_SCRIPT,
    required_features: &[
        WslExecutionFeature::NulSafeXargs,
        WslExecutionFeature::Sha256Sum,
        WslExecutionFeature::StableStat,
    ],
    map_exit: no_batch_stage_exit_mapping,
};

fn no_batch_stage_exit_mapping(_: Option<i32>, _: &str) -> Option<AppError> {
    None
}

fn reference_distro() -> String {
    std::env::var("SKILL_DECK_TEST_WSL_DISTRO")
        .expect("set SKILL_DECK_TEST_WSL_DISTRO to a disposable WSL test distro")
}

fn locator(session: &WslSession, native_path: String) -> ResourceLocator {
    ResourceLocator {
        environment: EnvironmentRef::Wsl {
            distro_name: session.distro_name.clone(),
        },
        native_path,
    }
}

async fn create_root(session: &WslSession, root: &str) {
    run_wsl_script(
        session,
        r#"mkdir -p -- "$1""#,
        &[root.to_string()],
        Vec::new(),
        TEST_TIMEOUT,
    )
    .await
    .expect("create WSL test root");
}

struct WslTempRoot {
    distro_name: String,
    user: String,
    native_path: String,
}

impl WslTempRoot {
    async fn create(session: &WslSession, native_path: String) -> Self {
        create_root(session, &native_path).await;
        Self {
            distro_name: session.distro_name.clone(),
            user: session.user.clone(),
            native_path,
        }
    }
}

impl Drop for WslTempRoot {
    fn drop(&mut self) {
        let args = build_wsl_exec_args(
            &self.distro_name,
            &self.user,
            r#"rm -rf -- "$1""#,
            std::slice::from_ref(&self.native_path),
        );
        let _ = Command::new("wsl.exe").args(args).status();
    }
}

#[tokio::test]
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO"]
async fn discovers_reference_distro_and_connects_a_stopped_distro_on_demand() {
    let distro = reference_distro();
    let discovered = discover_wsl_distributions()
        .await
        .expect("discover WSL distributions");
    assert!(discovered.contains(&distro));

    let terminate_status = Command::new("wsl.exe")
        .args(["--terminate", distro.as_str()])
        .status()
        .expect("terminate reference test distro");
    assert!(terminate_status.success());

    let session = connect_wsl_environment(&distro)
        .await
        .expect("connect stopped reference distro");
    assert_eq!(session.distro_name, distro);
    assert!(!session.user.is_empty());
    assert!(session.home.starts_with('/'));
    for feature in [
        WslExecutionFeature::NulSafeXargs,
        WslExecutionFeature::NulSafeSort,
        WslExecutionFeature::Sha256Sum,
        WslExecutionFeature::CanonicalReadlink,
        WslExecutionFeature::StableStat,
    ] {
        assert!(
            session.execution_profile.supports(feature),
            "{} is missing required WSL execution feature {feature:?}",
            session.distro_name
        );
    }

    let live = run_wsl_script(
        &session,
        r#"printf '%s\0%s\0' "$(id -un)" "$HOME"; if command -v git >/dev/null 2>&1; then printf '1\0'; else printf '0\0'; fi"#,
        &[],
        Vec::new(),
        TEST_TIMEOUT,
    )
    .await
    .expect("inspect live default user and Git capability");
    let fields = decode_nul_records(&live);
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0], session.user);
    assert_eq!(fields[1], session.home);
    assert_eq!(fields[2] == "1", session.git_available);
}

#[tokio::test]
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO"]
async fn round_trips_wsl_native_drvfs_and_unc_paths_with_non_ascii_names() {
    let session = connect_wsl_environment(&reference_distro())
        .await
        .expect("connect reference distro");
    let base_root = format!("/tmp/skill-deck-integration-{}", Uuid::new_v4());
    let root = format!("{base_root}/项目");
    let _root = WslTempRoot::create(&session, base_root).await;
    create_root(&session, &root).await;
    let io = EnvironmentLockIo::Wsl(session.clone());
    let native_file = format!("{root}/技能-lock.json");
    let native_locator = locator(&session, native_file.clone());
    io.write_atomic(&native_locator, "WSL 原生路径".as_bytes().to_vec())
        .await
        .expect("write WSL native path");
    assert_eq!(
        io.read(&native_locator)
            .await
            .expect("read WSL native path"),
        "WSL 原生路径".as_bytes()
    );

    let unc_path = format!(
        r"\\wsl.localhost\{}\{}",
        session.distro_name,
        native_file.trim_start_matches('/').replace('/', r"\")
    );
    assert_eq!(
        wsl_unc_to_linux_path(&unc_path, &session.distro_name).expect("map UNC path"),
        native_file
    );
    assert_eq!(
        std::fs::read(&unc_path).expect("read through WSL UNC"),
        "WSL 原生路径".as_bytes()
    );

    let host_temp = tempdir().expect("create Windows tempdir");
    let host_path = host_temp.path().to_string_lossy();
    let drvfs_root = map_windows_path_with_wslpath(&session, &host_path)
        .await
        .expect("map Windows tempdir through distro wslpath");
    let drvfs_file = format!("{drvfs_root}/skill deck-项目.txt");
    run_wsl_script(
        &session,
        r#"cat > "$1""#,
        &[drvfs_file],
        "DrvFS 内容".as_bytes().to_vec(),
        TEST_TIMEOUT,
    )
    .await
    .expect("write DrvFS path from WSL");
    assert_eq!(
        std::fs::read(host_temp.path().join("skill deck-项目.txt"))
            .expect("read DrvFS file from Windows"),
        "DrvFS 内容".as_bytes()
    );

    let leftovers = run_wsl_script(
        &session,
        r#"find "$1" -maxdepth 1 -type f -name '.lock.*' -print"#,
        &[root],
        Vec::new(),
        TEST_TIMEOUT,
    )
    .await
    .expect("inspect non-ASCII atomic-write leftovers");
    assert!(
        leftovers.is_empty(),
        "atomic lock temp files were not cleaned up"
    );
}

#[tokio::test]
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO"]
async fn cancellation_reaps_wsl_child_and_stops_writes() {
    let distro = reference_distro();
    let session = connect_wsl_environment(&distro)
        .await
        .expect("connect cancellation distro");
    let root = format!("/tmp/skill-deck-cancel-{}", Uuid::new_v4());
    let _root = WslTempRoot::create(&session, root.clone()).await;
    let operation_id = format!("cancel-{}", Uuid::new_v4().simple());
    let operation_root = format!("/tmp/skill-deck-operation-{operation_id}");
    let destinations = format!("{root}/targets");
    run_wsl_script(
        &session,
        r#"mkdir -p -- "$1"; printf '1\n%s\n' "$2" > "$1/.skill-deck-owner"; printf '{}' > "$1/recovery.json""#,
        &[operation_root.clone(), operation_id.clone()],
        Vec::new(),
        TEST_TIMEOUT,
    )
    .await
    .expect("initialize materialize operation root");
    let request = batch_remove_request(&destinations, 20_000);
    let cancellation = CancellationSignal::default();
    let operation_session = session.clone();
    let operation_cancellation = cancellation.clone();
    let operation_root_for_task = operation_root.clone();
    let operation_id_for_task = operation_id.clone();
    let operation = tokio::spawn(async move {
        WslOperationExecutor::execute(
            &BATCH_STAGE_OPERATION,
            WslOperationRequest {
                session: operation_session,
                args: vec![operation_root_for_task, operation_id_for_task],
                stdin: request,
                timeout: TEST_TIMEOUT,
                stdout_limit: 32,
                stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
                cancellation: Some(operation_cancellation),
            },
        )
        .await
    });

    let mut batch_started = false;
    for _ in 0..100 {
        if operation_batch_started(&session, &operation_root).await {
            batch_started = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        batch_started,
        "materialize batch did not start before cancellation"
    );
    cancellation.cancel();
    let error = operation
        .await
        .expect("join materialize batch")
        .expect_err("write probe must be cancelled");
    assert_eq!(error, AppError::MutationCancelled);

    let entries_after_cancel = operation_entry_count(&session, &operation_root).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        operation_entry_count(&session, &operation_root).await,
        entries_after_cancel,
        "the batch materialize child continued staging after cancellation returned"
    );
    run_wsl_script(
        &session,
        r#"rm -rf -- "$1""#,
        &[operation_root],
        Vec::new(),
        TEST_TIMEOUT,
    )
    .await
    .expect("clean cancelled operation root");
}

#[tokio::test]
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO"]
async fn runs_full_wsl_mutation_workflow_with_complete_payloads() {
    let session = connect_wsl_environment(&reference_distro())
        .await
        .expect("connect workflow distro");
    let root = format!("/tmp/skill-deck-workflow-{}", Uuid::new_v4());
    let _root = WslTempRoot::create(&session, root.clone()).await;

    run_full_wsl_mutation_workflow(session, root)
        .await
        .expect("complete WSL workflow");
}

#[tokio::test]
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO"]
async fn session_loss_invalidates_preview_before_execution() {
    let session = connect_wsl_environment(&reference_distro())
        .await
        .expect("connect session-loss distro");
    let root = format!("/tmp/skill-deck-session-loss-{}", Uuid::new_v4());
    let _root = WslTempRoot::create(&session, root.clone()).await;

    session_loss_invalidates_preview(session, root)
        .await
        .expect("session loss invalidates preview");
}

#[tokio::test]
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO"]
async fn cli_lock_conflict_preserves_external_change() {
    let session = connect_wsl_environment(&reference_distro())
        .await
        .expect("connect CLI conflict distro");
    let root = format!("/tmp/skill-deck-cli-conflict-{}", Uuid::new_v4());
    let _root = WslTempRoot::create(&session, root.clone()).await;

    run_cli_lock_conflict_acceptance(session, root)
        .await
        .expect("CLI conflict is preserved");
}

#[tokio::test]
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO"]
async fn reconnect_reindexes_recovery_and_sweeps_only_owned_orphan_payloads() {
    let session = connect_wsl_environment(&reference_distro())
        .await
        .expect("connect reconnect-closure distro");
    let root = format!("/tmp/skill-deck-reconnect-closure-{}", Uuid::new_v4());
    let _root = WslTempRoot::create(&session, root.clone()).await;

    reconnect_reindexes_recovery_and_sweeps_payloads(session, root)
        .await
        .expect("reconnect recovery and payload closure");
}

#[tokio::test]
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO"]
async fn marker_before_batch_stage_failure_converges_after_reconnect() {
    let session = connect_wsl_environment(&reference_distro())
        .await
        .expect("connect stage-failure distro");
    let root = format!("/tmp/skill-deck-stage-failure-{}", Uuid::new_v4());
    let _root = WslTempRoot::create(&session, root.clone()).await;

    run_marker_before_stage_acceptance(session, root)
        .await
        .expect("marker-before-stage recovery convergence");
}

fn batch_remove_request(destination_root: &str, entry_count: usize) -> Vec<u8> {
    let mut request = Vec::new();
    append_batch_record(
        &mut request,
        [
            "H".to_string(),
            "1".to_string(),
            (entry_count + 1).to_string(),
            entry_count.to_string(),
            String::new(),
            String::new(),
            String::new(),
        ],
    );
    for index in 0..entry_count {
        append_batch_record(
            &mut request,
            [
                "E".to_string(),
                format!("{index:06}"),
                format!("{destination_root}/entry-{index:06}"),
                "remove".to_string(),
                String::new(),
                "entry-v1-missing".to_string(),
                "0".to_string(),
            ],
        );
    }
    request
}

fn append_batch_record(request: &mut Vec<u8>, fields: [String; 7]) {
    for field in fields {
        request.extend_from_slice(field.as_bytes());
        request.push(0);
    }
}

async fn operation_entry_count(session: &WslSession, operation_root: &str) -> u32 {
    let output = run_wsl_script(
        session,
        r#"find "$1" -mindepth 1 -maxdepth 1 -type d -name 'entry-*' | wc -l"#,
        &[operation_root.to_string()],
        Vec::new(),
        TEST_TIMEOUT,
    )
    .await
    .expect("count materialize entries");
    String::from_utf8(output)
        .expect("entry count is UTF-8")
        .trim()
        .parse()
        .expect("entry count is numeric")
}

async fn operation_batch_started(session: &WslSession, operation_root: &str) -> bool {
    run_wsl_script(
        session,
        r#"if [ -d "$1/request" ]; then printf '1'; else printf '0'; fi"#,
        &[operation_root.to_string()],
        Vec::new(),
        TEST_TIMEOUT,
    )
    .await
    .expect("inspect materialize batch state")
        == b"1"
}

async fn file_size(session: &WslSession, path: &str) -> String {
    let output = run_wsl_script(
        session,
        r#"if [ -f "$1" ]; then wc -c < "$1"; else printf '0'; fi"#,
        &[path.to_string()],
        Vec::new(),
        TEST_TIMEOUT,
    )
    .await
    .expect("read WSL probe size");
    String::from_utf8(output)
        .expect("size is UTF-8")
        .trim()
        .to_string()
}
