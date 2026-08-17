use crate::core::lock_repository::{LockRepository, LockTarget};
use crate::core::lossless_lock::LockSchema;
use crate::environment::context_resolver::ContextResolver;
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::types::{EnvironmentRef, ResourceLocator, SkillLocation, SkillLocationRef};
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;

pub async fn get_last_selected_agents(
    environment: &EnvironmentRef,
    wsl: &WslRuntime,
) -> Result<Option<Vec<String>>, AppError> {
    let context = SkillLocationRef {
        environment: environment.clone(),
        scope: SkillLocation::Global,
    };
    match environment {
        EnvironmentRef::Native => {
            let resolved = ContextResolver::resolve_native(context)?;
            read_last_selected_agents_with_io(
                EnvironmentLockIo::Native,
                &global_lock_target(resolved.lock),
            )
            .await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            wsl.with_session_retry(&distro_name, move |session| {
                let context = context.clone();
                async move {
                    let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                    read_last_selected_agents_with_io(
                        EnvironmentLockIo::ActiveWsl(session),
                        &global_lock_target(resolved.lock),
                    )
                    .await
                }
            })
            .await
        }
    }
}

pub async fn set_last_selected_agents(
    environment: &EnvironmentRef,
    wsl: &WslRuntime,
    selected_agent_ids: &[String],
) -> Result<(), AppError> {
    let context = SkillLocationRef {
        environment: environment.clone(),
        scope: SkillLocation::Global,
    };
    match environment {
        EnvironmentRef::Native => {
            let resolved = ContextResolver::resolve_native(context)?;
            write_last_selected_agents_with_io(
                EnvironmentLockIo::Native,
                global_lock_target(resolved.lock),
                selected_agent_ids,
            )
            .await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let selected_agent_ids = selected_agent_ids.to_vec();
            wsl.with_session_retry(&distro_name, move |session| {
                let context = context.clone();
                let selected_agent_ids = selected_agent_ids.clone();
                async move {
                    let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                    write_last_selected_agents_with_io(
                        EnvironmentLockIo::ActiveWsl(session),
                        global_lock_target(resolved.lock),
                        &selected_agent_ids,
                    )
                    .await
                }
            })
            .await
        }
    }
}

fn global_lock_target(primary: ResourceLocator) -> LockTarget {
    LockTarget {
        primary,
        legacy: None,
        schema: LockSchema::Global,
    }
}

async fn read_last_selected_agents_with_io(
    io: EnvironmentLockIo,
    target: &LockTarget,
) -> Result<Option<Vec<String>>, AppError> {
    LockRepository::new(io)
        .read_document(target)
        .await?
        .into_value()
        .get("lastSelectedAgents")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(AppError::from)
}

async fn write_last_selected_agents_with_io(
    io: EnvironmentLockIo,
    target: LockTarget,
    selected_agent_ids: &[String],
) -> Result<(), AppError> {
    let repository = LockRepository::new(io);
    let mut transaction = repository
        .begin(
            target,
            crate::core::lock_repository::LockMutationTargets {
                root_fields: vec!["lastSelectedAgents".to_string()],
            },
        )
        .await?;
    transaction.replace_root(
        "lastSelectedAgents",
        serde_json::to_value(selected_agent_ids)?,
    )?;
    transaction.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::lossless_lock::LockSchema;
    use crate::environment::types::{EnvironmentRef, ResourceLocator};

    #[cfg(target_os = "windows")]
    // Run from the repository root on Windows:
    // `$env:SKILL_DECK_TEST_WSL_DISTRO='Ubuntu'; cargo test --manifest-path src-tauri/Cargo.toml application::agent_selection_history::tests::wsl_history_round_trip_preserves_unowned_global_fields -- --ignored --exact`
    // This covers real wsl.exe transport plus lossless global-lock read and write behavior.
    #[tokio::test]
    #[ignore = "requires SKILL_DECK_TEST_WSL_DISTRO and a real WSL distribution"]
    async fn wsl_history_round_trip_preserves_unowned_global_fields() {
        let distro_name = std::env::var("SKILL_DECK_TEST_WSL_DISTRO")
            .expect("set SKILL_DECK_TEST_WSL_DISTRO to an installed WSL distribution");
        let root = format!(
            "/tmp/skill-deck-agent-history-test-{}",
            uuid::Uuid::new_v4()
        );
        let wsl = WslRuntime::new_with_support(true, true);
        let outcome = wsl_history_round_trip(&wsl, &distro_name, &root).await;
        let cleanup = cleanup_wsl_history_test_root(&distro_name, &root).await;

        let (initial, value) = outcome.expect("round-trip WSL selection history");
        cleanup.expect("clean WSL selection history fixture");
        assert_eq!(initial, Some(vec!["codex".to_string()]));
        assert_eq!(
            value["lastSelectedAgents"],
            serde_json::json!(["claude-code"])
        );
        assert_eq!(
            value["defaultTargetAgents"],
            serde_json::json!({"global":["legacy"],"project":["legacy"]})
        );
        assert_eq!(value["futureField"], serde_json::json!({"enabled": true}));
    }

    #[cfg(target_os = "windows")]
    async fn wsl_history_round_trip(
        wsl: &WslRuntime,
        distro_name: &str,
        root: &str,
    ) -> Result<(Option<Vec<String>>, serde_json::Value), AppError> {
        let mut session = wsl.connect(distro_name).await?;
        session.home = root.to_string();
        session.xdg_state_home = None;
        session.config_home = format!("{root}/.config");
        wsl.insert(session.clone());
        let environment = EnvironmentRef::Wsl {
            distro_name: distro_name.to_string(),
        };
        let context = SkillLocationRef {
            environment: environment.clone(),
            scope: SkillLocation::Global,
        };
        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
        let io = EnvironmentLockIo::ActiveWsl(session);
        io.write_atomic(
            &resolved.lock,
            serde_json::to_vec(&serde_json::json!({
                "version": 3,
                "skills": {},
                "defaultTargetAgents": {"global": ["legacy"], "project": ["legacy"]},
                "lastSelectedAgents": ["codex"],
                "futureField": {"enabled": true}
            }))?,
        )
        .await?;

        let initial = get_last_selected_agents(&environment, wsl).await?;
        set_last_selected_agents(&environment, wsl, &["claude-code".to_string()]).await?;
        let value = serde_json::from_slice(&io.read(&resolved.lock).await?)?;
        Ok((initial, value))
    }

    #[cfg(target_os = "windows")]
    async fn cleanup_wsl_history_test_root(distro_name: &str, root: &str) -> Result<(), String> {
        let mut command = crate::background_process::tokio_command("wsl.exe");
        command.args([
            "--distribution",
            distro_name,
            "--exec",
            "/bin/sh",
            "-c",
            "rm -rf -- \"$1\"",
            "--",
            root,
        ]);
        command.kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(15), command.output())
            .await
            .map_err(|_| "timed out cleaning WSL selection history fixture".to_string())?
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }

    #[tokio::test]
    async fn reads_last_selected_agents_without_interpreting_legacy_defaults() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock_path = temp.path().join("skill-lock.json");
        std::fs::write(
            &lock_path,
            br#"{
              "version": 3,
              "skills": {},
              "defaultTargetAgents": {
                "global": ["claude-code", "codex"],
                "project": ["claude-code", "codex"]
              },
              "lastSelectedAgents": ["codex", "removed-agent"]
            }"#,
        )
        .expect("write lock");
        let target = LockTarget {
            primary: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: lock_path.to_string_lossy().to_string(),
            },
            legacy: None,
            schema: LockSchema::Global,
        };

        let selected = read_last_selected_agents_with_io(EnvironmentLockIo::Native, &target)
            .await
            .expect("read history");

        assert_eq!(
            selected,
            Some(vec!["codex".to_string(), "removed-agent".to_string()])
        );
    }

    #[tokio::test]
    async fn writes_last_selected_agents_without_changing_legacy_defaults() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock_path = temp.path().join("skill-lock.json");
        std::fs::write(
            &lock_path,
            br#"{
              "version": 3,
              "skills": {},
              "defaultTargetAgents": {"global": ["legacy"], "project": ["legacy"]},
              "lastSelectedAgents": ["codex"]
            }"#,
        )
        .expect("write lock");
        let target = LockTarget {
            primary: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: lock_path.to_string_lossy().to_string(),
            },
            legacy: None,
            schema: LockSchema::Global,
        };

        write_last_selected_agents_with_io(
            EnvironmentLockIo::Native,
            target,
            &["claude-code".to_string()],
        )
        .await
        .expect("write history");

        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(lock_path).expect("read lock"))
                .expect("parse lock");
        assert_eq!(
            value["lastSelectedAgents"],
            serde_json::json!(["claude-code"])
        );
        assert_eq!(
            value["defaultTargetAgents"],
            serde_json::json!({"global":["legacy"],"project":["legacy"]})
        );
    }

    #[tokio::test]
    async fn creates_the_global_lock_when_recording_the_first_selection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock_path = temp.path().join("skill-lock.json");
        let target = LockTarget {
            primary: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: lock_path.to_string_lossy().to_string(),
            },
            legacy: None,
            schema: LockSchema::Global,
        };

        write_last_selected_agents_with_io(
            EnvironmentLockIo::Native,
            target,
            &["codex".to_string()],
        )
        .await
        .expect("write first history");

        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(lock_path).expect("read lock"))
                .expect("parse lock");
        assert_eq!(value["version"], 3);
        assert_eq!(value["skills"], serde_json::json!({}));
        assert_eq!(value["lastSelectedAgents"], serde_json::json!(["codex"]));
    }
}
