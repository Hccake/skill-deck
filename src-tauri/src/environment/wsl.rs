#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio::time::timeout;
#[cfg(target_os = "windows")]
use tokio::time::Duration;

#[cfg(target_os = "windows")]
use crate::background_process::tokio_command;
use crate::environment::types::{
    EnvironmentKey, EnvironmentRef, EnvironmentRuntimeEvent, EnvironmentStatus,
};
use crate::error::AppError;

pub mod operations;

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
    #[serde(skip)]
    #[specta(skip)]
    pub runtime_generation: u64,
}

#[derive(Clone)]
struct CachedWslSession {
    generation: u64,
    session: WslSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WslCapabilityState {
    Unsupported,
    Disabled,
    Enabling,
    Enabled,
    Disabling,
}

struct EnvironmentRegistryState {
    capability: WslCapabilityState,
    capability_revision: u64,
    active_wsl_permits: usize,
    next_generation: u64,
    sessions: HashMap<EnvironmentKey, CachedWslSession>,
    runtime: HashMap<EnvironmentKey, EnvironmentRuntimeStatus>,
}

impl EnvironmentRegistryState {
    fn new(supported: bool, enabled: bool) -> Self {
        Self {
            capability: if !supported {
                WslCapabilityState::Unsupported
            } else if enabled {
                WslCapabilityState::Enabled
            } else {
                WslCapabilityState::Disabled
            },
            capability_revision: 0,
            active_wsl_permits: 0,
            next_generation: 0,
            sessions: HashMap::new(),
            runtime: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentRuntimeStatus {
    pub revision: u64,
    pub status: EnvironmentStatus,
    pub error: Option<AppError>,
}

#[derive(Clone)]
pub struct EnvironmentRegistry {
    state: Arc<Mutex<EnvironmentRegistryState>>,
    reconnect_locks: Arc<Mutex<HashMap<EnvironmentKey, Arc<AsyncMutex<()>>>>>,
    listener: Arc<Mutex<Option<EnvironmentRuntimeListener>>>,
    quiescence: Arc<Notify>,
}

type EnvironmentRuntimeListener = Arc<dyn Fn(EnvironmentRuntimeEvent) + Send + Sync>;

impl Default for EnvironmentRegistry {
    fn default() -> Self {
        Self::new(true)
    }
}

impl EnvironmentRegistry {
    pub fn new(wsl_integration_enabled: bool) -> Self {
        Self::new_with_support(true, wsl_integration_enabled)
    }

    pub fn new_with_support(supported: bool, wsl_integration_enabled: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(EnvironmentRegistryState::new(
                supported,
                wsl_integration_enabled,
            ))),
            reconnect_locks: Arc::new(Mutex::new(HashMap::new())),
            listener: Arc::new(Mutex::new(None)),
            quiescence: Arc::new(Notify::new()),
        }
    }

    pub fn wsl_integration_enabled(&self) -> bool {
        matches!(
            self.state
                .lock()
                .expect("environment registry lock poisoned")
                .capability,
            WslCapabilityState::Enabled | WslCapabilityState::Disabling
        )
    }

    #[cfg(test)]
    pub fn set_wsl_integration_enabled(&self, enabled: bool) {
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        state.capability = if enabled {
            WslCapabilityState::Enabled
        } else {
            WslCapabilityState::Disabled
        };
        state.capability_revision = state.capability_revision.saturating_add(1);
        if !enabled {
            state.sessions.clear();
            state.runtime.clear();
        }
    }

    fn disabled_error(distro_name: &str) -> AppError {
        AppError::EnvironmentUnavailable {
            environment: EnvironmentRef::Wsl {
                distro_name: distro_name.to_string(),
            },
            message: "WSL integration is disabled".to_string(),
        }
    }

    pub fn capability_revision(&self) -> u64 {
        self.state
            .lock()
            .expect("environment registry lock poisoned")
            .capability_revision
    }

    fn acquire_wsl_access(&self, distro_name: &str) -> Result<WslAccessPermit, AppError> {
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        match state.capability {
            WslCapabilityState::Enabled => {
                state.active_wsl_permits = state.active_wsl_permits.saturating_add(1);
                Ok(WslAccessPermit {
                    state: Arc::clone(&self.state),
                    quiescence: Arc::clone(&self.quiescence),
                    capability_revision: state.capability_revision,
                })
            }
            WslCapabilityState::Unsupported => Err(AppError::CapabilityUnavailable {
                capability: "wslIntegration".to_string(),
                path: None,
            }),
            WslCapabilityState::Disabled
            | WslCapabilityState::Enabling
            | WslCapabilityState::Disabling => Err(Self::disabled_error(distro_name)),
        }
    }

    pub(crate) fn with_wsl_access<T>(
        &self,
        distro_name: &str,
        action: impl FnOnce() -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let _permit = self.acquire_wsl_access(distro_name)?;
        action()
    }

    pub(crate) async fn discover_using<Discover, DiscoveryFuture>(
        &self,
        discover: Discover,
    ) -> Result<Vec<String>, AppError>
    where
        Discover: FnOnce() -> DiscoveryFuture,
        DiscoveryFuture: Future<Output = Result<Vec<String>, AppError>>,
    {
        let _permit = self.acquire_wsl_access("discovery")?;
        discover().await
    }

    pub(crate) async fn connect_using<C, CFut>(
        &self,
        distro_name: &str,
        mut connector: C,
    ) -> Result<WslSession, AppError>
    where
        C: FnMut(String) -> CFut,
        CFut: Future<Output = Result<WslSession, AppError>>,
    {
        let reconnect_lock = self.reconnect_lock(distro_name);
        let _reconnect = reconnect_lock.lock().await;
        let permit = self.acquire_wsl_access(distro_name)?;
        let mut session = connector(distro_name.to_string()).await?;
        self.insert_with_permit(&mut session, &permit)?;
        Ok(session)
    }

    pub fn insert(&self, mut session: WslSession) {
        if let Ok(permit) = self.acquire_wsl_access(&session.distro_name) {
            let _ = self.insert_with_permit(&mut session, &permit);
        }
    }

    #[cfg(test)]
    pub(crate) fn insert_if_enabled(&self, session: &mut WslSession) -> Result<(), AppError> {
        let permit = self.acquire_wsl_access(&session.distro_name)?;
        self.insert_with_permit(session, &permit)
    }

    fn insert_with_permit(
        &self,
        session: &mut WslSession,
        permit: &WslAccessPermit,
    ) -> Result<(), AppError> {
        let distro_name = session.distro_name.clone();
        let key = EnvironmentKey::wsl(&distro_name);
        let environment = EnvironmentRef::Wsl {
            distro_name: distro_name.clone(),
        };
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        if state.capability_revision != permit.capability_revision
            || !matches!(
                state.capability,
                WslCapabilityState::Enabled | WslCapabilityState::Disabling
            )
        {
            return Err(Self::disabled_error(&distro_name));
        }
        state.next_generation = state.next_generation.saturating_add(1);
        let generation = state.next_generation;
        session.runtime_generation = generation;
        state.sessions.insert(
            key.clone(),
            CachedWslSession {
                generation,
                session: session.clone(),
            },
        );
        state.runtime.insert(
            key,
            EnvironmentRuntimeStatus {
                revision: generation,
                status: EnvironmentStatus::Available,
                error: None,
            },
        );
        drop(state);
        self.publish(EnvironmentRuntimeEvent {
            capability_revision: permit.capability_revision,
            revision: generation,
            environment,
            status: EnvironmentStatus::Available,
            error: None,
        });
        Ok(())
    }

    pub fn begin_disable(&self) -> Result<WslDisableTransition, AppError> {
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        if state.capability != WslCapabilityState::Enabled {
            return Err(Self::disabled_error(""));
        }
        state.capability = WslCapabilityState::Disabling;
        Ok(WslDisableTransition {
            registry: self.clone(),
            completed: false,
        })
    }

    pub fn begin_enable(&self) -> Result<WslEnableTransition, AppError> {
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        match state.capability {
            WslCapabilityState::Disabled => {
                state.capability = WslCapabilityState::Enabling;
                Ok(WslEnableTransition {
                    registry: self.clone(),
                    completed: false,
                })
            }
            WslCapabilityState::Unsupported => Err(AppError::CapabilityUnavailable {
                capability: "wslIntegration".to_string(),
                path: None,
            }),
            _ => Err(Self::disabled_error("")),
        }
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
            .get(&EnvironmentKey::wsl(distro_name))
            .cloned()
    }

    pub fn runtime_status(&self, distro_name: &str) -> Option<EnvironmentRuntimeStatus> {
        self.state
            .lock()
            .expect("environment registry lock poisoned")
            .runtime
            .get(&EnvironmentKey::wsl(distro_name))
            .cloned()
    }

    fn reconnect_lock(&self, distro_name: &str) -> Arc<AsyncMutex<()>> {
        let mut locks = self
            .reconnect_locks
            .lock()
            .expect("environment reconnect lock map poisoned");
        Arc::clone(
            locks
                .entry(EnvironmentKey::wsl(distro_name))
                .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
        )
    }

    fn invalidate_generation(&self, distro_name: &str, generation: u64) -> Option<u64> {
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        let key = EnvironmentKey::wsl(distro_name);
        if state
            .sessions
            .get(&key)
            .is_some_and(|cached| cached.generation == generation)
        {
            state.sessions.remove(&key);
            state.next_generation = state.next_generation.saturating_add(1);
            Some(state.next_generation)
        } else {
            None
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
        if let Some(revision) = self.invalidate_generation(distro_name, generation) {
            let mut state = self
                .state
                .lock()
                .expect("environment registry lock poisoned");
            state.runtime.insert(
                EnvironmentKey::wsl(distro_name),
                EnvironmentRuntimeStatus {
                    revision,
                    status: EnvironmentStatus::Unavailable,
                    error: Some(error.clone()),
                },
            );
            let capability_revision = state.capability_revision;
            drop(state);
            self.publish(EnvironmentRuntimeEvent {
                capability_revision,
                revision,
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
    ) -> Result<(CachedWslSession, WslAccessPermit), AppError>
    where
        C: FnMut(String) -> CFut,
        CFut: Future<Output = Result<WslSession, AppError>>,
    {
        if self.get_cached(distro_name).is_some() {
            let permit = self.acquire_wsl_access(distro_name)?;
            if let Some(cached) = self.get_cached(distro_name) {
                return Ok((cached, permit));
            }
        }
        let reconnect_lock = self.reconnect_lock(distro_name);
        let _reconnect = reconnect_lock.lock().await;
        let permit = self.acquire_wsl_access(distro_name)?;
        if let Some(cached) = self.get_cached(distro_name) {
            return Ok((cached, permit));
        }
        let mut session = connector(distro_name.to_string()).await?;
        self.insert_with_permit(&mut session, &permit)?;
        self.get_cached(distro_name)
            .map(|cached| (cached, permit))
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
        let (initial, initial_access) = self
            .get_or_connect_using(distro_name, &mut connector)
            .await?;
        match operation(initial.session.clone()).await {
            Ok(result) => Ok(result),
            Err(AppError::EnvironmentUnavailable { .. }) => {
                drop(initial_access);
                let reconnect_lock = self.reconnect_lock(distro_name);
                let _reconnect = reconnect_lock.lock().await;
                let access = self.acquire_wsl_access(distro_name)?;
                let refreshed = match self.get_cached(distro_name) {
                    Some(cached) if cached.generation != initial.generation => cached,
                    _ => {
                        let mut session = match connector(distro_name.to_string()).await {
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
                        self.insert_with_permit(&mut session, &access)?;
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
                let result = match operation(refreshed.session).await {
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
                };
                drop(access);
                result
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
        let (cached, _access) = self
            .get_or_connect_using(distro_name, &mut connector)
            .await?;
        let result = operation(cached.session).await;
        if let Err(error @ AppError::EnvironmentUnavailable { .. }) = &result {
            self.publish_unavailable_if_current(distro_name, cached.generation, error.clone());
        }
        result
    }
}

pub struct WslAccessPermit {
    state: Arc<Mutex<EnvironmentRegistryState>>,
    quiescence: Arc<Notify>,
    capability_revision: u64,
}

impl Drop for WslAccessPermit {
    fn drop(&mut self) {
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        state.active_wsl_permits = state.active_wsl_permits.saturating_sub(1);
        let quiescent = state.active_wsl_permits == 0;
        drop(state);
        if quiescent {
            self.quiescence.notify_waiters();
        }
    }
}

pub struct WslDisableTransition {
    registry: EnvironmentRegistry,
    completed: bool,
}

impl WslDisableTransition {
    pub async fn wait_for_quiescence(&self, limit: std::time::Duration) -> Result<(), ()> {
        let wait = async {
            loop {
                let notified = self.registry.quiescence.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self
                    .registry
                    .state
                    .lock()
                    .expect("environment registry lock poisoned")
                    .active_wsl_permits
                    == 0
                {
                    return;
                }
                notified.await;
            }
        };
        timeout(limit, wait).await.map_err(|_| ())
    }

    pub fn commit_disabled(mut self) {
        let mut state = self
            .registry
            .state
            .lock()
            .expect("environment registry lock poisoned");
        assert_eq!(state.capability, WslCapabilityState::Disabling);
        assert_eq!(state.active_wsl_permits, 0);
        state.sessions.clear();
        state.runtime.clear();
        state.capability = WslCapabilityState::Disabled;
        state.capability_revision = state.capability_revision.saturating_add(1);
        self.completed = true;
    }
}

impl Drop for WslDisableTransition {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let mut state = self
            .registry
            .state
            .lock()
            .expect("environment registry lock poisoned");
        if state.capability == WslCapabilityState::Disabling {
            state.capability = WslCapabilityState::Enabled;
        }
    }
}

pub struct WslEnableTransition {
    registry: EnvironmentRegistry,
    completed: bool,
}

impl WslEnableTransition {
    pub fn commit_enabled(mut self) {
        let mut state = self
            .registry
            .state
            .lock()
            .expect("environment registry lock poisoned");
        assert_eq!(state.capability, WslCapabilityState::Enabling);
        state.capability = WslCapabilityState::Enabled;
        state.capability_revision = state.capability_revision.saturating_add(1);
        self.completed = true;
    }
}

impl Drop for WslEnableTransition {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let mut state = self
            .registry
            .state
            .lock()
            .expect("environment registry lock poisoned");
        if state.capability == WslCapabilityState::Enabling {
            state.capability = WslCapabilityState::Disabled;
        }
    }
}

pub fn parse_wsl_list_output(bytes: &[u8]) -> Vec<String> {
    let decoded = if bytes.len() >= 2 && bytes.len().is_multiple_of(2) {
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
        .map(|field| {
            String::from_utf8(field.to_vec()).map_err(|_| AppError::Custom {
                message: "invalid UTF-8 in WSL session response".to_string(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if fields.last().is_some_and(String::is_empty) {
        fields.pop();
    }
    if fields.len() != 11 || fields.first().map(String::as_str) != Some("3") {
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
        runtime_generation: 0,
    })
}

#[cfg(target_os = "windows")]
pub async fn discover_wsl_distributions() -> Result<Vec<String>, AppError> {
    let mut command = tokio_command("wsl.exe");
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
    const SCRIPT: &str = include_str!("wsl/scripts/session.sh");
    let mut command = tokio_command("wsl.exe");
    command.args([
        "--distribution",
        distro_name,
        "--exec",
        "/bin/sh",
        "-c",
        SCRIPT,
        "--",
        "session",
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
#[allow(
    clippy::disallowed_methods,
    reason = "WSL 解析测试需要直接运行内置的 shell 脚本"
)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{
        interpret_wsl_discovery_outcome, parse_wsl_list_output, parse_wsl_session_output,
        EnvironmentRegistry, WslDiscoveryCommandOutcome, WslSession,
    };
    use crate::environment::types::{EnvironmentRef, EnvironmentRuntimeEvent, EnvironmentStatus};
    use crate::error::{AppError, LockConflictTarget};

    #[cfg(target_os = "linux")]
    fn command_output_with_timeout(
        command: &mut Command,
        timeout: std::time::Duration,
    ) -> std::io::Result<Output> {
        use std::os::unix::process::CommandExt;

        command
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if child.try_wait()?.is_some() {
                return child.wait_with_output();
            }
            if std::time::Instant::now() >= deadline {
                if let Ok(process_group_id) = i32::try_from(child.id()) {
                    // The shell contract can spawn probe commands, so terminate the whole group.
                    let _ = unsafe { libc::kill(-process_group_id, libc::SIGKILL) };
                }
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "shell contract exceeded its execution deadline",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn sample_session(distro_name: &str, user: &str) -> WslSession {
        WslSession {
            distro_name: distro_name.to_string(),
            user: user.to_string(),
            uid: 1000,
            home: format!("/home/{user}"),
            xdg_state_home: None,
            config_home: format!("/home/{user}/.config"),
            environment: std::collections::BTreeMap::new(),
            runtime_generation: 0,
        }
    }

    #[test]
    fn cloned_registry_handles_share_cached_sessions() {
        let registry = EnvironmentRegistry::default();
        let cloned = registry.clone();

        cloned.insert(sample_session("Ubuntu", "alice"));

        assert_eq!(
            registry.get("Ubuntu").expect("shared session").user,
            "alice"
        );
    }

    #[test]
    fn registry_uses_one_cached_session_for_case_insensitive_distro_aliases() {
        let registry = EnvironmentRegistry::default();
        registry.insert(sample_session("Ubuntu", "alice"));

        assert_eq!(
            registry
                .get("UBUNTU")
                .expect("case-insensitive cached session")
                .user,
            "alice"
        );
    }

    #[test]
    fn registry_uses_one_reconnect_lock_for_case_insensitive_distro_aliases() {
        let registry = EnvironmentRegistry::default();

        assert!(Arc::ptr_eq(
            &registry.reconnect_lock("Ubuntu"),
            &registry.reconnect_lock("ubuntu")
        ));
    }

    #[test]
    fn reinserting_identical_session_advances_runtime_generation() {
        let registry = EnvironmentRegistry::default();
        registry.insert(sample_session("Ubuntu", "alice"));
        let first = registry.get("Ubuntu").expect("first session");

        registry.insert(sample_session("Ubuntu", "alice"));
        let second = registry.get("Ubuntu").expect("second session");

        assert!(second.runtime_generation > first.runtime_generation);
    }

    #[test]
    fn environment_runtime_event_and_snapshot_share_monotonic_revision() {
        let registry = EnvironmentRegistry::default();
        let events = record_runtime_events(&registry);

        registry.insert(sample_session("Ubuntu", "alice"));
        registry.insert(sample_session("Ubuntu", "bob"));

        let events = events.lock().expect("runtime event recorder lock poisoned");
        assert_eq!(events.len(), 2);
        assert!(events[1].revision > events[0].revision);
        let runtime = registry
            .runtime_status("ubuntu")
            .expect("runtime status is case insensitive");
        assert_eq!(runtime.revision, events[1].revision);
        assert_eq!(runtime.status, EnvironmentStatus::Available);
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
    async fn disabled_registry_rejects_wsl_before_connecting() {
        let registry = EnvironmentRegistry::new(false);
        let connects = Arc::new(AtomicUsize::new(0));
        let connector_count = Arc::clone(&connects);

        let error = registry
            .with_session_retry_using(
                "Ubuntu",
                move |_| {
                    connector_count.fetch_add(1, Ordering::SeqCst);
                    async { Ok(sample_session("Ubuntu", "unexpected")) }
                },
                |session| async move { Ok(session.user) },
            )
            .await
            .expect_err("disabled WSL integration");

        assert!(matches!(
            error,
            AppError::EnvironmentUnavailable {
                environment: EnvironmentRef::Wsl { ref distro_name },
                ..
            } if distro_name == "Ubuntu"
        ));
        assert_eq!(connects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn waiting_reconnect_task_cannot_start_connector_after_disable_commits() {
        let registry = Arc::new(EnvironmentRegistry::default());
        let reconnect_lock = registry.reconnect_lock("Ubuntu");
        let held_reconnect = reconnect_lock.lock().await;
        let connects = Arc::new(AtomicUsize::new(0));
        let task_registry = Arc::clone(&registry);
        let task_connects = Arc::clone(&connects);
        let task = tokio::spawn(async move {
            task_registry
                .with_session_retry_using(
                    "Ubuntu",
                    move |_| {
                        task_connects.fetch_add(1, Ordering::SeqCst);
                        async { Ok(sample_session("Ubuntu", "unexpected")) }
                    },
                    |session| async move { Ok(session.user) },
                )
                .await
        });
        tokio::task::yield_now().await;

        let transition = registry.begin_disable().expect("begin disable");
        transition
            .wait_for_quiescence(std::time::Duration::from_secs(1))
            .await
            .expect("no access permit while reconnect lock is pending");
        transition.commit_disabled();
        drop(held_reconnect);

        let error = task
            .await
            .expect("waiting task")
            .expect_err("disabled task rejected");
        assert!(matches!(error, AppError::EnvironmentUnavailable { .. }));
        assert_eq!(connects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn disable_timeout_rolls_capability_back_to_enabled() {
        let registry = Arc::new(EnvironmentRegistry::default());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let signals = Arc::new(Mutex::new((Some(started_tx), Some(release_rx))));
        let task_registry = Arc::clone(&registry);
        let task_signals = Arc::clone(&signals);
        let operation = tokio::spawn(async move {
            task_registry
                .with_session_retry_using(
                    "Ubuntu",
                    move |_| async { Ok(sample_session("Ubuntu", "alice")) },
                    move |_| {
                        let (started_tx, release_rx) = {
                            let mut signals = task_signals.lock().expect("signals lock");
                            (
                                signals.0.take().expect("one operation start"),
                                signals.1.take().expect("one operation release"),
                            )
                        };
                        async move {
                            started_tx.send(()).expect("signal operation start");
                            release_rx.await.expect("release operation");
                            Ok(())
                        }
                    },
                )
                .await
        });
        started_rx.await.expect("operation started");

        let transition = registry.begin_disable().expect("begin disable");
        transition
            .wait_for_quiescence(std::time::Duration::ZERO)
            .await
            .expect_err("active permit prevents disable");
        drop(transition);

        assert!(registry.wsl_integration_enabled());
        release_tx.send(()).expect("release operation");
        operation
            .await
            .expect("operation task")
            .expect("operation result");
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
                capability_revision: 0,
                revision: 2,
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
        assert!(events[1].revision > events[0].revision);
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
                capability_revision: 0,
                revision: 3,
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
                capability_revision: 0,
                revision: 2,
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
                capability_revision: 0,
                revision: 2,
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
                capability_revision: 0,
                revision: 2,
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
            runtime_generation: 0,
        });

        assert_eq!(registry.get("Ubuntu").expect("session").user, "alice");
        assert!(registry.get("Debian").is_none());
    }

    #[test]
    fn parses_versioned_session_output() {
        let output = b"3\0alice\x001000\0/home/alice\0/home/alice/.state\0/home/alice/.config\0/opt/codex\0/opt/claude\0\0\0\0";
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
    }

    #[test]
    fn rejects_legacy_session_shapes() {
        for output in [
            b"1\0alice\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\x001\0".as_slice(),
            b"2\0alice\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\x001\x001\x001\x001\x001\x001\0".as_slice(),
        ] {
            assert!(parse_wsl_session_output("Ubuntu", output).is_err());
        }
    }

    #[test]
    fn rejects_invalid_current_session_payloads() {
        for output in [
            b"3\0alice\0not-a-uid\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\0".as_slice(),
            b"3\0alice\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\0unexpected\0"
                .as_slice(),
            b"3\0\xff\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\0".as_slice(),
        ] {
            assert!(parse_wsl_session_output("Ubuntu", output).is_err());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn shell_contract_runner_terminates_a_hung_process_group() {
        let started = std::time::Instant::now();
        let error = command_output_with_timeout(
            Command::new("/bin/sh").arg("-c").arg("sleep 30"),
            std::time::Duration::from_millis(100),
        )
        .expect_err("hung shell contract must time out");

        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bundled_session_script_reports_the_complete_session_baseline() {
        let output = command_output_with_timeout(
            Command::new("/bin/sh")
                .arg("-c")
                .arg(include_str!("wsl/scripts/session.sh"))
                .arg("--")
                .arg("session"),
            std::time::Duration::from_secs(10),
        )
        .expect("session script");

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.starts_with(b"3\0"));
        let session = parse_wsl_session_output("Ubuntu", &output.stdout)
            .expect("parse bundled session output");
        assert!(!session.user.is_empty());
        assert!(session.home.starts_with('/'));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bundled_session_script_rejects_each_missing_baseline_tool() {
        use std::os::unix::fs::PermissionsExt;

        for command in ["git", "xargs", "sort", "sha256sum", "readlink", "stat"] {
            let temp = tempfile::tempdir().expect("temporary command directory");
            let command_path = temp.path().join(command);
            std::fs::write(&command_path, "#!/bin/sh\nexit 1\n").expect("write failing command");
            std::fs::set_permissions(&command_path, std::fs::Permissions::from_mode(0o755))
                .expect("make failing command executable");
            let path = format!(
                "{}:{}",
                temp.path().display(),
                std::env::var("PATH").unwrap_or_default()
            );

            let output = command_output_with_timeout(
                Command::new("/bin/sh")
                    .arg("-c")
                    .arg(include_str!("wsl/scripts/session.sh"))
                    .arg("--")
                    .arg("session")
                    .env("PATH", path),
                std::time::Duration::from_secs(10),
            )
            .expect("session script");

            assert!(
                !output.status.success(),
                "unavailable {command} must reject the WSL session"
            );
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .to_ascii_lowercase()
                    .contains(command),
                "{command} failure returned stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bundled_session_script_rejects_incompatible_baseline_behavior() {
        use std::os::unix::fs::PermissionsExt;

        let incompatible_commands = [
            (
                "xargs",
                r#"#!/bin/sh
for argument in "$@"; do
  [ "$argument" = "-r" ] && exit 64
done
printf 'a\nb\n'
"#,
            ),
            (
                "sort",
                r#"#!/bin/sh
printf 'a\0b\0'
"#,
            ),
            (
                "sha256sum",
                r#"#!/bin/sh
[ "$#" -eq 0 ] || exit 64
printf '%s  -\n' 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855'
"#,
            ),
            (
                "readlink",
                r#"#!/bin/sh
for argument in "$@"; do
  [ "$argument" = "--" ] && exit 64
done
printf '/\n'
"#,
            ),
        ];
        let mut unexpected_successes = Vec::new();

        for (command, script) in incompatible_commands {
            let temp = tempfile::tempdir().expect("temporary command directory");
            let command_path = temp.path().join(command);
            std::fs::write(&command_path, script).expect("write incompatible command");
            std::fs::set_permissions(&command_path, std::fs::Permissions::from_mode(0o755))
                .expect("make incompatible command executable");
            let path = format!(
                "{}:{}",
                temp.path().display(),
                std::env::var("PATH").unwrap_or_default()
            );

            let output = command_output_with_timeout(
                Command::new("/bin/sh")
                    .arg("-c")
                    .arg(include_str!("wsl/scripts/session.sh"))
                    .arg("--")
                    .arg("session")
                    .env("PATH", path),
                std::time::Duration::from_secs(10),
            )
            .expect("session script");

            if output.status.success() {
                unexpected_successes.push(command);
                continue;
            }
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .to_ascii_lowercase()
                    .contains(command),
                "{command} incompatibility returned stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        assert!(
            unexpected_successes.is_empty(),
            "incompatible commands passed the WSL session baseline: {unexpected_successes:?}"
        );
    }

    #[test]
    fn session_script_cleans_the_probe_root_only_after_owning_its_creation() {
        let script = include_str!("wsl/scripts/session.sh");

        assert!(script.contains("probe_root_created=0"));
        assert!(script.contains("probe_root_created=1"));
        assert!(script.contains("if [ \"$probe_root_created\" = 1 ]; then"));
    }

    #[test]
    fn parses_empty_xdg_state_home_without_shifting_fields() {
        let output = b"3\0alice\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\0";
        let session = parse_wsl_session_output("Ubuntu", output).expect("parse session");

        assert_eq!(session.xdg_state_home, None);
    }
}
