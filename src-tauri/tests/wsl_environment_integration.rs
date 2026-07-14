#![cfg(target_os = "windows")]

use std::process::Command;

use app_lib::wsl_integration_support::{
    build_wsl_exec_args, connect_wsl_environment, decode_nul_records, discover_wsl_distributions,
    map_windows_path_with_wslpath, read_wsl_projects, run_wsl_script, write_wsl_projects,
    wsl_unc_to_linux_path, EnvironmentLockIo, EnvironmentRef, ProjectBinding, ResourceLocator,
    WslSession,
};
use tempfile::tempdir;
use tokio::time::Duration;
use uuid::Uuid;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

fn first_configured_distro() -> String {
    std::env::var("SKILL_DECK_TEST_WSL_DISTRO_A")
        .expect("set SKILL_DECK_TEST_WSL_DISTRO_A to a disposable WSL test distro")
}

fn configured_distros() -> (String, String) {
    let first = first_configured_distro();
    let second = std::env::var("SKILL_DECK_TEST_WSL_DISTRO_B")
        .expect("set SKILL_DECK_TEST_WSL_DISTRO_B to a different WSL test distro");
    assert_ne!(first, second, "the two WSL test distros must be different");
    (first, second)
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
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO_A/B"]
async fn discovers_configured_distros_and_connects_a_stopped_distro_on_demand() {
    let (first, second) = configured_distros();
    let discovered = discover_wsl_distributions()
        .await
        .expect("discover WSL distributions");
    assert!(discovered.contains(&first));
    assert!(discovered.contains(&second));

    let terminate_status = Command::new("wsl.exe")
        .args(["--terminate", first.as_str()])
        .status()
        .expect("terminate first test distro");
    assert!(terminate_status.success());

    let first_session = connect_wsl_environment(&first)
        .await
        .expect("connect stopped first distro");
    let second_session = connect_wsl_environment(&second)
        .await
        .expect("connect second distro");
    assert_eq!(first_session.distro_name, first);
    assert_eq!(second_session.distro_name, second);
    assert!(!first_session.user.is_empty());
    assert!(first_session.home.starts_with('/'));

    let live = run_wsl_script(
        &first_session,
        r#"printf '%s\0%s\0' "$(id -un)" "$HOME"; if command -v git >/dev/null 2>&1; then printf '1\0'; else printf '0\0'; fi"#,
        &[],
        Vec::new(),
        TEST_TIMEOUT,
    )
    .await
    .expect("inspect live default user and Git capability");
    let fields = decode_nul_records(&live);
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0], first_session.user);
    assert_eq!(fields[1], first_session.home);
    assert_eq!(fields[2] == "1", first_session.git_available);
}

#[tokio::test]
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO_A/B"]
async fn keeps_same_native_project_and_lock_paths_isolated_between_distros() {
    let (first, second) = configured_distros();
    let first_session = connect_wsl_environment(&first)
        .await
        .expect("connect first distro");
    let second_session = connect_wsl_environment(&second)
        .await
        .expect("connect second distro");
    let root = format!("/tmp/skill-deck-integration-{}", Uuid::new_v4());
    let _first_root = WslTempRoot::create(&first_session, root.clone()).await;
    let _second_root = WslTempRoot::create(&second_session, root.clone()).await;
    let first_io = EnvironmentLockIo::Wsl(first_session.clone());
    let second_io = EnvironmentLockIo::Wsl(second_session.clone());

    let mut first_project_session = first_session.clone();
    first_project_session.home = root.clone();
    let mut second_project_session = second_session.clone();
    second_project_session.home = root.clone();
    write_wsl_projects(
        &first_project_session,
        vec![ProjectBinding {
            id: "first".to_string(),
            native_path: "/tmp/first".to_string(),
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        }],
    )
    .await
    .expect("write first distro projects");
    write_wsl_projects(
        &second_project_session,
        vec![ProjectBinding {
            id: "second".to_string(),
            native_path: "/tmp/second".to_string(),
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        }],
    )
    .await
    .expect("write second distro projects");
    let first_projects = read_wsl_projects(&first_project_session)
        .await
        .expect("read first projects");
    let second_projects = read_wsl_projects(&second_project_session)
        .await
        .expect("read second projects");
    assert_eq!(first_projects[0].id, "first");
    assert_eq!(second_projects[0].id, "second");

    let first_lock = locator(&first_session, format!("{root}/skills-lock.json"));
    let second_lock = locator(&second_session, format!("{root}/skills-lock.json"));
    first_io
        .write_atomic(
            &first_lock,
            br#"{"skills":{"toolkit":{"owner":"first"}}}"#.to_vec(),
        )
        .await
        .expect("write first distro lock");
    second_io
        .write_atomic(
            &second_lock,
            br#"{"skills":{"toolkit":{"owner":"second"}}}"#.to_vec(),
        )
        .await
        .expect("write second distro lock");
    assert_ne!(
        first_io.read(&first_lock).await.expect("read first lock"),
        second_io
            .read(&second_lock)
            .await
            .expect("read second lock")
    );
    assert!(first_io.read(&second_lock).await.is_err());

    for session in [&first_session, &second_session] {
        let leftovers = run_wsl_script(
            session,
            r#"find "$1" -maxdepth 1 -type f -name '.lock.*' -print"#,
            std::slice::from_ref(&root),
            Vec::new(),
            TEST_TIMEOUT,
        )
        .await
        .expect("inspect atomic-write leftovers");
        assert!(
            leftovers.is_empty(),
            "atomic lock temp files were not cleaned up"
        );
    }
}

#[tokio::test]
#[ignore = "requires Windows and SKILL_DECK_TEST_WSL_DISTRO_A/B"]
async fn round_trips_wsl_native_drvfs_and_unc_paths_with_non_ascii_names() {
    let first = first_configured_distro();
    let session = connect_wsl_environment(&first)
        .await
        .expect("connect first distro");
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
