#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;
#[cfg(target_os = "windows")]
use tokio::process::Command;
use tokio::sync::Mutex as AsyncMutex;
#[cfg(target_os = "windows")]
use tokio::time::{timeout, Duration};

use crate::environment::types::{EnvironmentRef, EnvironmentRuntimeEvent, EnvironmentStatus};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct WslSession {
    pub distro_name: String,
    pub user: String,
    pub uid: u32,
    pub home: String,
    pub xdg_state_home: Option<String>,
    pub config_home: String,
    pub environment: BTreeMap<String, String>,
    pub git_available: bool,
}

#[derive(Clone)]
struct CachedWslSession {
    generation: u64,
    session: WslSession,
}

#[derive(Default)]
struct EnvironmentRegistryState {
    next_generation: u64,
    sessions: HashMap<String, CachedWslSession>,
}

#[derive(Default)]
pub struct EnvironmentRegistry {
    state: Mutex<EnvironmentRegistryState>,
    reconnect_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    listener: Mutex<Option<EnvironmentRuntimeListener>>,
}

type EnvironmentRuntimeListener = Arc<dyn Fn(EnvironmentRuntimeEvent) + Send + Sync>;

impl EnvironmentRegistry {
    pub fn insert(&self, session: WslSession) {
        let environment = EnvironmentRef::Wsl {
            distro_name: session.distro_name.clone(),
        };
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        state.next_generation = state.next_generation.wrapping_add(1);
        let generation = state.next_generation;
        state.sessions.insert(
            session.distro_name.clone(),
            CachedWslSession {
                generation,
                session,
            },
        );
        drop(state);
        self.publish(EnvironmentRuntimeEvent {
            environment,
            status: EnvironmentStatus::Available,
            error: None,
        });
    }

    #[cfg(test)]
    pub fn get(&self, distro_name: &str) -> Option<WslSession> {
        self.get_cached(distro_name).map(|cached| cached.session)
    }

    fn get_cached(&self, distro_name: &str) -> Option<CachedWslSession> {
        self.state
            .lock()
            .expect("environment registry lock poisoned")
            .sessions
            .get(distro_name)
            .cloned()
    }

    fn reconnect_lock(&self, distro_name: &str) -> Arc<AsyncMutex<()>> {
        let mut locks = self
            .reconnect_locks
            .lock()
            .expect("environment reconnect lock map poisoned");
        Arc::clone(
            locks
                .entry(distro_name.to_string())
                .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
        )
    }

    fn invalidate_generation(&self, distro_name: &str, generation: u64) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        if state
            .sessions
            .get(distro_name)
            .is_some_and(|cached| cached.generation == generation)
        {
            state.sessions.remove(distro_name);
            true
        } else {
            false
        }
    }

    pub fn set_listener(&self, listener: impl Fn(EnvironmentRuntimeEvent) + Send + Sync + 'static) {
        *self
            .listener
            .lock()
            .expect("environment runtime listener lock poisoned") = Some(Arc::new(listener));
    }

    fn publish(&self, event: EnvironmentRuntimeEvent) {
        let listener = self
            .listener
            .lock()
            .expect("environment runtime listener lock poisoned")
            .clone();
        if let Some(listener) = listener {
            listener(event);
        }
    }

    fn publish_unavailable_if_current(&self, distro_name: &str, generation: u64, error: AppError) {
        if self.invalidate_generation(distro_name, generation) {
            self.publish(EnvironmentRuntimeEvent {
                environment: EnvironmentRef::Wsl {
                    distro_name: distro_name.to_string(),
                },
                status: EnvironmentStatus::Unavailable,
                error: Some(error),
            });
        }
    }

    async fn get_or_connect_using<C, CFut>(
        &self,
        distro_name: &str,
        connector: &mut C,
    ) -> Result<CachedWslSession, AppError>
    where
        C: FnMut(String) -> CFut,
        CFut: Future<Output = Result<WslSession, AppError>>,
    {
        if let Some(cached) = self.get_cached(distro_name) {
            return Ok(cached);
        }
        let reconnect_lock = self.reconnect_lock(distro_name);
        let _reconnect = reconnect_lock.lock().await;
        if let Some(cached) = self.get_cached(distro_name) {
            return Ok(cached);
        }
        let session = connector(distro_name.to_string()).await?;
        self.insert(session);
        self.get_cached(distro_name)
            .ok_or_else(|| AppError::EnvironmentUnavailable {
                environment: EnvironmentRef::Wsl {
                    distro_name: distro_name.to_string(),
                },
                message: "connected WSL session was not cached".to_string(),
            })
    }

    pub(crate) async fn with_session_retry_using<T, C, CFut, O, OFut>(
        &self,
        distro_name: &str,
        mut connector: C,
        mut operation: O,
    ) -> Result<T, AppError>
    where
        C: FnMut(String) -> CFut,
        CFut: Future<Output = Result<WslSession, AppError>>,
        O: FnMut(WslSession) -> OFut,
        OFut: Future<Output = Result<T, AppError>>,
    {
        let initial = self
            .get_or_connect_using(distro_name, &mut connector)
            .await?;
        match operation(initial.session.clone()).await {
            Ok(result) => Ok(result),
            Err(AppError::EnvironmentUnavailable { .. }) => {
                let reconnect_lock = self.reconnect_lock(distro_name);
                let _reconnect = reconnect_lock.lock().await;
                let refreshed = match self.get_cached(distro_name) {
                    Some(cached) if cached.generation != initial.generation => cached,
                    _ => {
                        let session = match connector(distro_name.to_string()).await {
                            Ok(session) => session,
                            Err(error @ AppError::EnvironmentUnavailable { .. }) => {
                                self.publish_unavailable_if_current(
                                    distro_name,
                                    initial.generation,
                                    error.clone(),
                                );
                                return Err(error);
                            }
                            Err(error) => return Err(error),
                        };
                        self.insert(session);
                        self.get_cached(distro_name).ok_or_else(|| {
                            AppError::EnvironmentUnavailable {
                                environment: EnvironmentRef::Wsl {
                                    distro_name: distro_name.to_string(),
                                },
                                message: "reconnected WSL session was not cached".to_string(),
                            }
                        })?
                    }
                };
                match operation(refreshed.session).await {
                    Ok(result) => Ok(result),
                    Err(error @ AppError::EnvironmentUnavailable { .. }) => {
                        self.publish_unavailable_if_current(
                            distro_name,
                            refreshed.generation,
                            error.clone(),
                        );
                        Err(error)
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    pub async fn with_session_retry<T, O, OFut>(
        &self,
        distro_name: &str,
        operation: O,
    ) -> Result<T, AppError>
    where
        O: FnMut(WslSession) -> OFut,
        OFut: Future<Output = Result<T, AppError>>,
    {
        self.with_session_retry_using(
            distro_name,
            |distro_name| async move { connect_wsl_environment(&distro_name).await },
            operation,
        )
        .await
    }

    pub async fn with_session<T, O, OFut>(
        &self,
        distro_name: &str,
        mut operation: O,
    ) -> Result<T, AppError>
    where
        O: FnMut(WslSession) -> OFut,
        OFut: Future<Output = Result<T, AppError>>,
    {
        let mut connector =
            |distro_name: String| async move { connect_wsl_environment(&distro_name).await };
        let cached = self
            .get_or_connect_using(distro_name, &mut connector)
            .await?;
        let result = operation(cached.session).await;
        if let Err(error @ AppError::EnvironmentUnavailable { .. }) = &result {
            self.publish_unavailable_if_current(distro_name, cached.generation, error.clone());
        }
        result
    }
}

pub fn parse_wsl_list_output(bytes: &[u8]) -> Vec<String> {
    let decoded = if bytes.len() >= 2 && bytes.len() % 2 == 0 {
        let utf16 = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&utf16).unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    decoded
        .lines()
        .map(|line| line.trim_matches(['\0', '\r', ' ', '\t']))
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

enum WslDiscoveryCommandOutcome {
    TimedOut,
    SpawnFailed(std::io::Error),
    Completed {
        success: bool,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
}

fn interpret_wsl_discovery_outcome(
    outcome: WslDiscoveryCommandOutcome,
) -> Result<Vec<String>, AppError> {
    match outcome {
        WslDiscoveryCommandOutcome::SpawnFailed(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(Vec::new())
        }
        WslDiscoveryCommandOutcome::TimedOut => Err(AppError::EnvironmentDiscoveryFailed {
            message: "wsl.exe --list --quiet timed out".to_string(),
        }),
        WslDiscoveryCommandOutcome::SpawnFailed(error) => {
            Err(AppError::EnvironmentDiscoveryFailed {
                message: error.to_string(),
            })
        }
        WslDiscoveryCommandOutcome::Completed {
            success: true,
            stdout,
            ..
        } => Ok(parse_wsl_list_output(&stdout)),
        WslDiscoveryCommandOutcome::Completed { stderr, .. } => {
            let message = String::from_utf8_lossy(&stderr).trim().to_string();
            Err(AppError::EnvironmentDiscoveryFailed {
                message: if message.is_empty() {
                    "wsl.exe --list --quiet exited unsuccessfully".to_string()
                } else {
                    message
                },
            })
        }
    }
}

pub fn parse_wsl_session_output(distro_name: &str, bytes: &[u8]) -> Result<WslSession, AppError> {
    let mut fields = bytes
        .split(|byte| *byte == 0)
        .map(|field| String::from_utf8_lossy(field).into_owned())
        .collect::<Vec<_>>();
    if fields.last().is_some_and(String::is_empty) {
        fields.pop();
    }
    if fields.len() != 12 || fields[0] != "1" {
        return Err(AppError::Custom {
            message: "invalid WSL session response".to_string(),
        });
    }
    let environment = [
        ("CODEX_HOME", 6usize),
        ("CLAUDE_CONFIG_DIR", 7),
        ("VIBE_HOME", 8),
        ("HERMES_HOME", 9),
        ("AUTOHAND_HOME", 10),
    ]
    .into_iter()
    .filter(|(_, index)| !fields[*index].is_empty())
    .map(|(name, index)| (name.to_string(), fields[index].clone()))
    .collect();
    Ok(WslSession {
        distro_name: distro_name.to_string(),
        user: fields[1].clone(),
        uid: fields[2].parse().map_err(|_| AppError::Custom {
            message: "invalid WSL uid".to_string(),
        })?,
        home: fields[3].clone(),
        xdg_state_home: (!fields[4].is_empty()).then(|| fields[4].clone()),
        config_home: fields[5].clone(),
        environment,
        git_available: fields[11] == "1",
    })
}

#[cfg(target_os = "windows")]
pub async fn discover_wsl_distributions() -> Result<Vec<String>, AppError> {
    let mut command = Command::new("wsl.exe");
    command.args(["--list", "--quiet"]);
    let outcome = match timeout(Duration::from_secs(10), command.output()).await {
        Err(_) => WslDiscoveryCommandOutcome::TimedOut,
        Ok(Err(error)) => WslDiscoveryCommandOutcome::SpawnFailed(error),
        Ok(Ok(output)) => WslDiscoveryCommandOutcome::Completed {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: output.stderr,
        },
    };
    interpret_wsl_discovery_outcome(outcome)
}

#[cfg(not(target_os = "windows"))]
pub async fn discover_wsl_distributions() -> Result<Vec<String>, AppError> {
    Ok(Vec::new())
}

#[cfg(target_os = "windows")]
pub async fn connect_wsl_environment(distro_name: &str) -> Result<WslSession, AppError> {
    const SCRIPT: &str = r#"printf '1\0'; id -un | tr -d '\n'; printf '\0'; id -u | tr -d '\n'; printf '\0'; printf '%s\0' "$HOME" "${XDG_STATE_HOME:-}" "${XDG_CONFIG_HOME:-$HOME/.config}" "${CODEX_HOME:-}" "${CLAUDE_CONFIG_DIR:-}" "${VIBE_HOME:-}" "${HERMES_HOME:-}" "${AUTOHAND_HOME:-}"; if command -v git >/dev/null 2>&1; then printf '1\0'; else printf '0\0'; fi"#;
    let mut command = Command::new("wsl.exe");
    command.args([
        "--distribution",
        distro_name,
        "--exec",
        "/bin/sh",
        "-c",
        SCRIPT,
    ]);
    let environment = EnvironmentRef::Wsl {
        distro_name: distro_name.to_string(),
    };
    let output = timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| AppError::EnvironmentUnavailable {
            environment: environment.clone(),
            message: format!("connecting to WSL distro '{distro_name}' timed out"),
        })?
        .map_err(|error| AppError::EnvironmentUnavailable {
            environment: environment.clone(),
            message: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(AppError::EnvironmentUnavailable {
            environment,
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    parse_wsl_session_output(distro_name, &output.stdout).map_err(|error| {
        AppError::EnvironmentUnavailable {
            environment: EnvironmentRef::Wsl {
                distro_name: distro_name.to_string(),
            },
            message: error.to_string(),
        }
    })
}

#[cfg(not(target_os = "windows"))]
pub async fn connect_wsl_environment(_distro_name: &str) -> Result<WslSession, AppError> {
    Err(AppError::EnvironmentUnavailable {
        environment: EnvironmentRef::Wsl {
            distro_name: _distro_name.to_string(),
        },
        message: "WSL is only available on Windows".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{
        interpret_wsl_discovery_outcome, parse_wsl_list_output, parse_wsl_session_output,
        EnvironmentRegistry, WslDiscoveryCommandOutcome, WslSession,
    };
    use crate::environment::types::{EnvironmentRef, EnvironmentRuntimeEvent, EnvironmentStatus};
    use crate::error::{AppError, LockConflictTarget};

    fn sample_session(distro_name: &str, user: &str) -> WslSession {
        WslSession {
            distro_name: distro_name.to_string(),
            user: user.to_string(),
            uid: 1000,
            home: format!("/home/{user}"),
            xdg_state_home: None,
            config_home: format!("/home/{user}/.config"),
            environment: std::collections::BTreeMap::new(),
            git_available: true,
        }
    }

    fn unavailable(distro_name: &str) -> AppError {
        AppError::EnvironmentUnavailable {
            environment: EnvironmentRef::Wsl {
                distro_name: distro_name.to_string(),
            },
            message: "session expired".to_string(),
        }
    }

    fn record_runtime_events(
        registry: &EnvironmentRegistry,
    ) -> Arc<Mutex<Vec<EnvironmentRuntimeEvent>>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&events);
        registry.set_listener(move |event| {
            recorded
                .lock()
                .expect("runtime event recorder lock poisoned")
                .push(event);
        });
        events
    }

    #[tokio::test]
    async fn session_retry_reuses_a_cached_session_without_connecting() {
        let registry = EnvironmentRegistry::default();
        registry.insert(sample_session("Ubuntu", "alice"));
        let connects = Arc::new(AtomicUsize::new(0));
        let connector_count = Arc::clone(&connects);

        let user = registry
            .with_session_retry_using(
                "Ubuntu",
                move |_| {
                    connector_count.fetch_add(1, Ordering::SeqCst);
                    async { Ok(sample_session("Ubuntu", "unexpected")) }
                },
                |session| async move { Ok(session.user) },
            )
            .await
            .expect("cached operation");

        assert_eq!(user, "alice");
        assert_eq!(connects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn session_retry_reconnects_once_after_environment_unavailable() {
        let registry = EnvironmentRegistry::default();
        registry.insert(sample_session("Ubuntu", "old-user"));
        let events = record_runtime_events(&registry);
        let connects = Arc::new(AtomicUsize::new(0));
        let operations = Arc::new(AtomicUsize::new(0));
        let connector_count = Arc::clone(&connects);
        let operation_count = Arc::clone(&operations);

        let user = registry
            .with_session_retry_using(
                "Ubuntu",
                move |_| {
                    connector_count.fetch_add(1, Ordering::SeqCst);
                    async { Ok(sample_session("Ubuntu", "new-user")) }
                },
                move |session| {
                    let attempt = operation_count.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if attempt == 0 {
                            Err(unavailable("Ubuntu"))
                        } else {
                            Ok(session.user)
                        }
                    }
                },
            )
            .await
            .expect("retried operation");

        assert_eq!(user, "new-user");
        assert_eq!(connects.load(Ordering::SeqCst), 1);
        assert_eq!(operations.load(Ordering::SeqCst), 2);
        assert_eq!(
            registry.get("Ubuntu").expect("refreshed session").user,
            "new-user"
        );
        assert_eq!(
            *events.lock().expect("runtime event recorder lock poisoned"),
            vec![EnvironmentRuntimeEvent {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                status: EnvironmentStatus::Available,
                error: None,
            }]
        );
    }

    #[tokio::test]
    async fn session_retry_does_not_repeat_a_started_business_failure() {
        let registry = EnvironmentRegistry::default();
        registry.insert(sample_session("Ubuntu", "alice"));
        let events = record_runtime_events(&registry);
        let connects = Arc::new(AtomicUsize::new(0));
        let operations = Arc::new(AtomicUsize::new(0));
        let connector_count = Arc::clone(&connects);
        let operation_count = Arc::clone(&operations);

        let error = registry
            .with_session_retry_using(
                "Ubuntu",
                move |_| {
                    connector_count.fetch_add(1, Ordering::SeqCst);
                    async { Ok(sample_session("Ubuntu", "new-user")) }
                },
                move |_| {
                    operation_count.fetch_add(1, Ordering::SeqCst);
                    async {
                        Err::<(), _>(AppError::WslCommandFailed {
                            exit_code: Some(13),
                            stderr: "permission denied".to_string(),
                        })
                    }
                },
            )
            .await
            .expect_err("business error");

        assert!(matches!(error, AppError::WslCommandFailed { .. }));
        assert_eq!(connects.load(Ordering::SeqCst), 0);
        assert_eq!(operations.load(Ordering::SeqCst), 1);
        assert!(events
            .lock()
            .expect("runtime event recorder lock poisoned")
            .is_empty());
        assert_eq!(
            registry.get("Ubuntu").expect("cached session").user,
            "alice"
        );

        let error = registry
            .with_session_retry_using(
                "Ubuntu",
                |_| async { Ok(sample_session("Ubuntu", "unexpected")) },
                |_| async {
                    Err::<(), _>(AppError::LockConflict {
                        target: LockConflictTarget::Skill {
                            skill_name: "toolkit".to_string(),
                        },
                    })
                },
            )
            .await
            .expect_err("lock conflict");

        assert!(matches!(error, AppError::LockConflict { .. }));
        assert!(events
            .lock()
            .expect("runtime event recorder lock poisoned")
            .is_empty());
        assert_eq!(
            registry.get("Ubuntu").expect("cached session").user,
            "alice"
        );
    }

    #[tokio::test]
    async fn session_retry_retries_at_most_once() {
        let registry = EnvironmentRegistry::default();
        registry.insert(sample_session("Ubuntu", "alice"));
        let events = record_runtime_events(&registry);
        let connects = Arc::new(AtomicUsize::new(0));
        let operations = Arc::new(AtomicUsize::new(0));
        let connector_count = Arc::clone(&connects);
        let operation_count = Arc::clone(&operations);

        let error = registry
            .with_session_retry_using(
                "Ubuntu",
                move |_| {
                    connector_count.fetch_add(1, Ordering::SeqCst);
                    async { Ok(sample_session("Ubuntu", "new-user")) }
                },
                move |_| {
                    operation_count.fetch_add(1, Ordering::SeqCst);
                    async { Err::<(), _>(unavailable("Ubuntu")) }
                },
            )
            .await
            .expect_err("second unavailable result is final");

        assert!(matches!(error, AppError::EnvironmentUnavailable { .. }));
        assert_eq!(connects.load(Ordering::SeqCst), 1);
        assert_eq!(operations.load(Ordering::SeqCst), 2);
        let events = events.lock().expect("runtime event recorder lock poisoned");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.status == EnvironmentStatus::Unavailable)
                .count(),
            1
        );
        assert_eq!(
            events.last(),
            Some(&EnvironmentRuntimeEvent {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                status: EnvironmentStatus::Unavailable,
                error: Some(unavailable("Ubuntu")),
            })
        );
        assert!(registry.get("Ubuntu").is_none());
    }

    #[tokio::test]
    async fn session_retry_connector_failure_invalidates_and_publishes_unavailable() {
        let registry = EnvironmentRegistry::default();
        registry.insert(sample_session("Ubuntu", "old-user"));
        let events = record_runtime_events(&registry);
        let operations = Arc::new(AtomicUsize::new(0));
        let operation_count = Arc::clone(&operations);

        let error = registry
            .with_session_retry_using(
                "Ubuntu",
                |_| async { Err(unavailable("Ubuntu")) },
                move |_| {
                    operation_count.fetch_add(1, Ordering::SeqCst);
                    async { Err::<(), _>(unavailable("Ubuntu")) }
                },
            )
            .await
            .expect_err("failed reconnect is final");

        assert!(matches!(error, AppError::EnvironmentUnavailable { .. }));
        assert_eq!(operations.load(Ordering::SeqCst), 1);
        assert!(registry.get("Ubuntu").is_none());
        assert_eq!(
            *events.lock().expect("runtime event recorder lock poisoned"),
            vec![EnvironmentRuntimeEvent {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                status: EnvironmentStatus::Unavailable,
                error: Some(unavailable("Ubuntu")),
            }]
        );
    }

    #[tokio::test]
    async fn session_operation_invalidates_without_replaying_a_mutation() {
        let registry = EnvironmentRegistry::default();
        registry.insert(sample_session("Ubuntu", "old-user"));
        let events = record_runtime_events(&registry);
        let operations = Arc::new(AtomicUsize::new(0));
        let operation_count = Arc::clone(&operations);

        let error = registry
            .with_session("Ubuntu", move |_| {
                operation_count.fetch_add(1, Ordering::SeqCst);
                async { Err::<(), _>(unavailable("Ubuntu")) }
            })
            .await
            .expect_err("mutation must not be replayed");

        assert!(matches!(error, AppError::EnvironmentUnavailable { .. }));
        assert_eq!(operations.load(Ordering::SeqCst), 1);
        assert!(registry.get("Ubuntu").is_none());
        assert_eq!(
            *events.lock().expect("runtime event recorder lock poisoned"),
            vec![EnvironmentRuntimeEvent {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                status: EnvironmentStatus::Unavailable,
                error: Some(unavailable("Ubuntu")),
            }]
        );
    }

    #[tokio::test]
    async fn old_generation_failure_does_not_invalidate_or_publish_over_a_new_session() {
        let registry = Arc::new(EnvironmentRegistry::default());
        registry.insert(sample_session("Ubuntu", "old-user"));
        let events = record_runtime_events(&registry);
        let replacement_registry = Arc::clone(&registry);

        let error = registry
            .with_session("Ubuntu", move |_| {
                let replacement_registry = Arc::clone(&replacement_registry);
                async move {
                    replacement_registry.insert(sample_session("Ubuntu", "new-user"));
                    Err::<(), _>(unavailable("Ubuntu"))
                }
            })
            .await
            .expect_err("old session operation must still fail");

        assert!(matches!(error, AppError::EnvironmentUnavailable { .. }));
        assert_eq!(
            registry.get("Ubuntu").expect("new session remains").user,
            "new-user"
        );
        let events = events.lock().expect("runtime event recorder lock poisoned");
        assert_eq!(
            events.as_slice(),
            &[EnvironmentRuntimeEvent {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                status: EnvironmentStatus::Available,
                error: None,
            }]
        );
    }

    #[tokio::test]
    async fn concurrent_session_failures_share_one_reconnect() {
        let registry = Arc::new(EnvironmentRegistry::default());
        registry.insert(sample_session("Ubuntu", "old-user"));
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let connects = Arc::new(AtomicUsize::new(0));

        let run = |registry: Arc<EnvironmentRegistry>| {
            let barrier = Arc::clone(&barrier);
            let connects = Arc::clone(&connects);
            async move {
                let first_attempt = Arc::new(AtomicUsize::new(0));
                let attempts = Arc::clone(&first_attempt);
                registry
                    .with_session_retry_using(
                        "Ubuntu",
                        move |_| {
                            connects.fetch_add(1, Ordering::SeqCst);
                            async { Ok(sample_session("Ubuntu", "new-user")) }
                        },
                        move |session| {
                            let barrier = Arc::clone(&barrier);
                            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                            async move {
                                if attempt == 0 {
                                    barrier.wait().await;
                                    Err(unavailable("Ubuntu"))
                                } else {
                                    Ok(session.user)
                                }
                            }
                        },
                    )
                    .await
            }
        };

        let (first, second) = tokio::join!(run(Arc::clone(&registry)), run(registry));

        assert_eq!(first.expect("first operation"), "new-user");
        assert_eq!(second.expect("second operation"), "new-user");
        assert_eq!(connects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn missing_wsl_executable_is_normal_wsl_discovery() {
        let distributions =
            interpret_wsl_discovery_outcome(WslDiscoveryCommandOutcome::SpawnFailed(
                std::io::Error::from(std::io::ErrorKind::NotFound),
            ))
            .expect("missing WSL is a normal Host-only result");

        assert!(distributions.is_empty());
    }

    #[test]
    fn wsl_discovery_timeout_is_typed() {
        assert!(matches!(
            interpret_wsl_discovery_outcome(WslDiscoveryCommandOutcome::TimedOut),
            Err(AppError::EnvironmentDiscoveryFailed { .. })
        ));
    }

    #[test]
    fn wsl_discovery_spawn_and_exit_failures_are_typed() {
        for outcome in [
            WslDiscoveryCommandOutcome::SpawnFailed(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "blocked",
            )),
            WslDiscoveryCommandOutcome::Completed {
                success: false,
                stdout: Vec::new(),
                stderr: b"failed".to_vec(),
            },
        ] {
            assert!(matches!(
                interpret_wsl_discovery_outcome(outcome),
                Err(AppError::EnvironmentDiscoveryFailed { .. })
            ));
        }
    }

    #[test]
    fn empty_successful_wsl_discovery_is_normal() {
        let distributions =
            interpret_wsl_discovery_outcome(WslDiscoveryCommandOutcome::Completed {
                success: true,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
            .expect("empty distribution list");

        assert!(distributions.is_empty());
    }

    #[test]
    fn parses_utf16_wsl_list_and_removes_nul_and_blank_lines() {
        let text = "Ubuntu-24.04\0\r\nDebian\0\r\n\r\n";
        let bytes = text
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();

        assert_eq!(
            parse_wsl_list_output(&bytes),
            vec!["Ubuntu-24.04", "Debian"]
        );
    }

    #[test]
    fn registry_keeps_successful_sessions_by_distro() {
        let registry = EnvironmentRegistry::default();
        registry.insert(WslSession {
            distro_name: "Ubuntu".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: std::collections::BTreeMap::new(),
            git_available: true,
        });

        assert_eq!(registry.get("Ubuntu").expect("session").user, "alice");
        assert!(registry.get("Debian").is_none());
    }

    #[test]
    fn parses_versioned_session_output() {
        let output = b"1\0alice\x001000\0/home/alice\0/home/alice/.state\0/home/alice/.config\0/opt/codex\0/opt/claude\0\0\0\x001\0";
        let session = parse_wsl_session_output("Ubuntu", output).expect("parse session");

        assert_eq!(session.user, "alice");
        assert_eq!(session.uid, 1000);
        assert_eq!(session.home, "/home/alice");
        assert_eq!(
            session.xdg_state_home.as_deref(),
            Some("/home/alice/.state")
        );
        assert_eq!(session.config_home, "/home/alice/.config");
        assert_eq!(session.environment["CODEX_HOME"], "/opt/codex");
        assert_eq!(session.environment["CLAUDE_CONFIG_DIR"], "/opt/claude");
        assert!(session.git_available);
    }

    #[test]
    fn parses_empty_xdg_state_home_without_shifting_fields() {
        let output = b"1\0alice\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\x001\0";
        let session = parse_wsl_session_output("Ubuntu", output).expect("parse session");

        assert_eq!(session.xdg_state_home, None);
        assert!(session.git_available);
    }
}
