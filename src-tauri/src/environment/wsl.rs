#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::Digest;
use specta::Type;
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio::time::timeout;
#[cfg(target_os = "windows")]
use tokio::time::Duration;

#[cfg(target_os = "windows")]
use crate::background_process::tokio_command;
use crate::environment::path_mapping::map_wsl_input_without_wslpath;
use crate::environment::types::{
    EnvironmentKey, EnvironmentRef, EnvironmentRuntimeEvent, EnvironmentStatus,
};
use crate::error::AppError;

pub mod operations;
pub(crate) mod protocol;
mod worker;

#[cfg(target_os = "windows")]
fn wsl_command() -> tokio::process::Command {
    let executable = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .map(|root| root.join("System32").join("wsl.exe"))
        .unwrap_or_else(|| std::path::PathBuf::from("wsl.exe"));
    tokio_command(executable)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub(crate) struct WslSession {
    pub(crate) distro_name: String,
    pub(crate) user: String,
    pub(crate) uid: u32,
    pub(crate) home: String,
    pub(crate) xdg_state_home: Option<String>,
    pub(crate) config_home: String,
    pub(crate) environment: BTreeMap<String, String>,
    #[serde(skip)]
    #[specta(skip)]
    pub(crate) runtime_generation: u64,
}

#[derive(Clone)]
struct CachedWslSession {
    generation: u64,
    session: WslSession,
    worker: Option<worker::WorkerSession>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WslCapabilityState {
    Unsupported,
    Disabled,
    Enabling,
    Enabled,
    Disabling,
}

struct WslRuntimeState {
    capability: WslCapabilityState,
    capability_revision: u64,
    active_wsl_permits: usize,
    active_source_owners: usize,
    next_generation: u64,
    sessions: HashMap<EnvironmentKey, CachedWslSession>,
    runtime: HashMap<EnvironmentKey, EnvironmentRuntimeStatus>,
}

impl WslRuntimeState {
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
            active_source_owners: 0,
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
pub(crate) struct WslRuntime {
    state: Arc<Mutex<WslRuntimeState>>,
    reconnect_locks: Arc<Mutex<HashMap<EnvironmentKey, Arc<AsyncMutex<()>>>>>,
    listener: Arc<Mutex<Option<EnvironmentRuntimeListener>>>,
    quiescence: Arc<Notify>,
    source_retirement: Arc<Notify>,
    worker_artifact_directory: Option<Arc<PathBuf>>,
}

#[derive(Clone)]
pub(crate) struct WslWorkspace {
    registry: WslRuntime,
    distro_name: String,
    capability_cycle: u64,
}

impl std::fmt::Debug for WslWorkspace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WslWorkspace")
            .field("distro_name", &self.distro_name)
            .field("capability_cycle", &self.capability_cycle)
            .finish_non_exhaustive()
    }
}

impl PartialEq for WslWorkspace {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.registry.state, &other.registry.state)
            && self.distro_name.eq_ignore_ascii_case(&other.distro_name)
            && self.capability_cycle == other.capability_cycle
    }
}

impl Eq for WslWorkspace {}

type EnvironmentRuntimeListener = Arc<dyn Fn(EnvironmentRuntimeEvent) + Send + Sync>;

impl Default for WslRuntime {
    fn default() -> Self {
        Self::new(true)
    }
}

impl WslRuntime {
    pub fn new(wsl_integration_enabled: bool) -> Self {
        Self::new_with_support(true, wsl_integration_enabled)
    }

    pub fn new_with_support(supported: bool, wsl_integration_enabled: bool) -> Self {
        Self::new_with_worker_artifact_directory(supported, wsl_integration_enabled, None)
    }

    pub fn new_with_worker_artifact_directory(
        supported: bool,
        wsl_integration_enabled: bool,
        worker_artifact_directory: Option<PathBuf>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(WslRuntimeState::new(
                supported,
                wsl_integration_enabled,
            ))),
            reconnect_locks: Arc::new(Mutex::new(HashMap::new())),
            listener: Arc::new(Mutex::new(None)),
            quiescence: Arc::new(Notify::new()),
            source_retirement: Arc::new(Notify::new()),
            worker_artifact_directory: worker_artifact_directory.map(Arc::new),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_wsl_test() -> Self {
        Self::new_with_worker_artifact_directory(
            true,
            true,
            Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/wsl-worker/current")),
        )
    }

    pub fn wsl_integration_enabled(&self) -> bool {
        matches!(
            self.state
                .lock()
                .expect("WSL runtime lock poisoned")
                .capability,
            WslCapabilityState::Enabled | WslCapabilityState::Disabling
        )
    }

    fn worker_artifact_directory(&self, distro_name: &str) -> Result<&std::path::Path, AppError> {
        self.worker_artifact_directory
            .as_ref()
            .map(|directory| directory.as_path())
            .ok_or_else(|| AppError::EnvironmentUnavailable {
                environment: EnvironmentRef::Wsl {
                    distro_name: distro_name.to_string(),
                },
                message: "WSL worker artifact directory is not configured".to_string(),
            })
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

    fn enabled_cycle(&self, distro_name: &str) -> Result<u64, AppError> {
        let state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        match state.capability {
            WslCapabilityState::Enabled => Ok(state.capability_revision),
            WslCapabilityState::Unsupported => Err(AppError::CapabilityUnavailable {
                capability: "wslIntegration".to_string(),
                path: None,
            }),
            WslCapabilityState::Disabled
            | WslCapabilityState::Enabling
            | WslCapabilityState::Disabling => Err(Self::disabled_error(distro_name)),
        }
    }

    fn acquire_wsl_access(&self, distro_name: &str) -> Result<WslAccessPermit, AppError> {
        self.acquire_wsl_access_for_cycle(distro_name, None)
    }

    fn acquire_wsl_access_for_cycle(
        &self,
        distro_name: &str,
        expected_cycle: Option<u64>,
    ) -> Result<WslAccessPermit, AppError> {
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        match state.capability {
            WslCapabilityState::Enabled
                if expected_cycle.is_none_or(|cycle| cycle == state.capability_revision) =>
            {
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
            WslCapabilityState::Enabled
            | WslCapabilityState::Disabled
            | WslCapabilityState::Enabling
            | WslCapabilityState::Disabling => Err(Self::disabled_error(distro_name)),
        }
    }

    pub fn workspace(&self, distro_name: &str) -> Result<WslWorkspace, AppError> {
        let state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        match state.capability {
            WslCapabilityState::Enabled => Ok(WslWorkspace {
                registry: self.clone(),
                distro_name: distro_name.to_string(),
                capability_cycle: state.capability_revision,
            }),
            WslCapabilityState::Unsupported => Err(AppError::CapabilityUnavailable {
                capability: "wslIntegration".to_string(),
                path: None,
            }),
            WslCapabilityState::Disabled
            | WslCapabilityState::Enabling
            | WslCapabilityState::Disabling => Err(Self::disabled_error(distro_name)),
        }
    }

    pub fn map_input_without_process(
        &self,
        distro_name: &str,
        path: &str,
    ) -> Result<Option<String>, AppError> {
        let _permit = self.acquire_wsl_access(distro_name)?;
        map_wsl_input_without_wslpath(distro_name, path)
    }

    async fn discover_with<Discover, DiscoveryFuture>(
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

    pub async fn discover(&self) -> Result<Vec<String>, AppError> {
        self.discover_with(discover_wsl_distributions).await
    }

    #[cfg(test)]
    pub(crate) async fn discover_using<Discover, DiscoveryFuture>(
        &self,
        discover: Discover,
    ) -> Result<Vec<String>, AppError>
    where
        Discover: FnOnce() -> DiscoveryFuture,
        DiscoveryFuture: Future<Output = Result<Vec<String>, AppError>>,
    {
        self.discover_with(discover).await
    }

    pub async fn connect(&self, distro_name: &str) -> Result<WslSession, AppError> {
        let expected_cycle = self.enabled_cycle(distro_name)?;
        let reconnect_lock = self.reconnect_lock(distro_name);
        let _reconnect = reconnect_lock.lock().await;
        let permit = self.acquire_wsl_access_for_cycle(distro_name, Some(expected_cycle))?;
        #[cfg(target_os = "windows")]
        ensure_wsl2_candidate(
            distro_name,
            &discover_wsl_distributions().await.map_err(|error| {
                AppError::EnvironmentUnavailable {
                    environment: EnvironmentRef::Wsl {
                        distro_name: distro_name.to_string(),
                    },
                    message: error.to_string(),
                }
            })?,
        )?;
        let mut session = connect_wsl_environment(distro_name).await?;
        let worker =
            match worker::connect_worker(&session, self.worker_artifact_directory(distro_name)?)
                .await
            {
                Ok(worker) => worker,
                Err(error) => {
                    self.publish_connect_failure_if_cycle(
                        distro_name,
                        permit.capability_revision,
                        error.clone(),
                    );
                    return Err(error);
                }
            };
        let worker_closed = worker.closed_receiver();
        self.insert_with_permit(&mut session, &permit, Some(worker))?;
        self.monitor_worker(
            distro_name.to_string(),
            session.runtime_generation,
            worker_closed,
        );
        Ok(session)
    }

    #[cfg(test)]
    pub(crate) fn insert(&self, mut session: WslSession) {
        if let Ok(permit) = self.acquire_wsl_access(&session.distro_name) {
            let _ = self.insert_with_permit(&mut session, &permit, None);
        }
    }

    fn insert_with_permit(
        &self,
        session: &mut WslSession,
        permit: &WslAccessPermit,
        worker: Option<worker::WorkerSession>,
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
                worker,
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

    fn publish_connect_failure_if_cycle(
        &self,
        distro_name: &str,
        capability_revision: u64,
        error: AppError,
    ) {
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        if state.capability_revision != capability_revision
            || state.capability != WslCapabilityState::Enabled
        {
            return;
        }
        state.next_generation = state.next_generation.saturating_add(1);
        let revision = state.next_generation;
        state.runtime.insert(
            EnvironmentKey::wsl(distro_name),
            EnvironmentRuntimeStatus {
                revision,
                status: EnvironmentStatus::Unavailable,
                error: Some(error.clone()),
            },
        );
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

    fn monitor_worker(
        &self,
        distro_name: String,
        generation: u64,
        mut closed: tokio::sync::watch::Receiver<bool>,
    ) {
        let registry = self.clone();
        tokio::spawn(async move {
            if !*closed.borrow() && closed.changed().await.is_err() {
                return;
            }
            if *closed.borrow() {
                registry.publish_worker_closed_if_current(&distro_name, generation);
            }
        });
    }

    fn publish_worker_closed_if_current(&self, distro_name: &str, generation: u64) {
        let error = AppError::EnvironmentUnavailable {
            environment: EnvironmentRef::Wsl {
                distro_name: distro_name.to_string(),
            },
            message: "WSL worker session closed".to_string(),
        };
        let mut state = self
            .state
            .lock()
            .expect("environment registry lock poisoned");
        let key = EnvironmentKey::wsl(distro_name);
        if state.capability != WslCapabilityState::Enabled
            || state
                .sessions
                .get(&key)
                .is_none_or(|cached| cached.generation != generation)
        {
            return;
        }
        state.sessions.remove(&key);
        state.next_generation = state.next_generation.saturating_add(1);
        let revision = state.next_generation;
        state.runtime.insert(
            key,
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

    async fn get_or_connect_using<C, CFut>(
        &self,
        distro_name: &str,
        expected_cycle: Option<u64>,
        connector: &mut C,
    ) -> Result<(CachedWslSession, WslAccessPermit), AppError>
    where
        C: FnMut(String) -> CFut,
        CFut: Future<Output = Result<WslSession, AppError>>,
    {
        if self.get_cached(distro_name).is_some() {
            let permit = self.acquire_wsl_access_for_cycle(distro_name, expected_cycle)?;
            if let Some(cached) = self.get_cached(distro_name) {
                return Ok((cached, permit));
            }
        }
        let reconnect_lock = self.reconnect_lock(distro_name);
        let _reconnect = reconnect_lock.lock().await;
        let permit = self.acquire_wsl_access_for_cycle(distro_name, expected_cycle)?;
        if let Some(cached) = self.get_cached(distro_name) {
            return Ok((cached, permit));
        }
        let mut session = connector(distro_name.to_string()).await?;
        self.insert_with_permit(&mut session, &permit, None)?;
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
        connector: C,
        operation: O,
    ) -> Result<T, AppError>
    where
        C: FnMut(String) -> CFut,
        CFut: Future<Output = Result<WslSession, AppError>>,
        O: FnMut(WslSession) -> OFut,
        OFut: Future<Output = Result<T, AppError>>,
    {
        let expected_cycle = self.enabled_cycle(distro_name)?;
        self.with_session_retry_in_cycle(distro_name, Some(expected_cycle), connector, operation)
            .await
    }

    async fn with_session_retry_in_cycle<T, C, CFut, O, OFut>(
        &self,
        distro_name: &str,
        expected_cycle: Option<u64>,
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
            .get_or_connect_using(distro_name, expected_cycle, &mut connector)
            .await?;
        match operation(initial.session.clone()).await {
            Ok(result) => Ok(result),
            Err(AppError::EnvironmentUnavailable { .. }) => {
                drop(initial_access);
                let reconnect_lock = self.reconnect_lock(distro_name);
                let _reconnect = reconnect_lock.lock().await;
                let access = self.acquire_wsl_access_for_cycle(distro_name, expected_cycle)?;
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
                        self.insert_with_permit(&mut session, &access, None)?;
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
        let expected_cycle = self.enabled_cycle(distro_name)?;
        let mut connector =
            |distro_name: String| async move { connect_wsl_environment(&distro_name).await };
        let (cached, _access) = self
            .get_or_connect_using(distro_name, Some(expected_cycle), &mut connector)
            .await?;
        let result = operation(cached.session).await;
        if let Err(error @ AppError::EnvironmentUnavailable { .. }) = &result {
            self.publish_unavailable_if_current(distro_name, cached.generation, error.clone());
        }
        result
    }
}

impl WslWorkspace {
    pub fn distro_name(&self) -> &str {
        &self.distro_name
    }

    pub(crate) fn filesystem_inspector(
        &self,
    ) -> Arc<dyn crate::environment::inspection::FilesystemInspector> {
        Arc::new(operations::inspection::WslInspector::new(self.clone()))
    }

    pub(crate) fn payload_storage(
        &self,
    ) -> Arc<dyn crate::application::payload_session::PayloadSessionStorage> {
        Arc::new(operations::acquire::WslPayloadSessionStorage::new(
            self.clone(),
        ))
    }

    pub(crate) fn defer_worker_source_release(
        &self,
        handle: operations::source_acquisition::WorkerSourceHandle,
    ) {
        let workspace = self.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let Some(cached) = workspace.registry.get_cached(&workspace.distro_name) else {
                    return;
                };
                if cached.generation != handle.generation {
                    return;
                }
                let Some(worker) = cached.worker else {
                    return;
                };
                let _ = worker
                    .request_control_with_cancellation(
                        environment_protocol::Message::ReleaseSource {
                            source_id: handle.id,
                        },
                        std::time::Duration::from_secs(10),
                        None,
                    )
                    .await;
            });
        }
    }

    pub(crate) fn register_source_owner(&self) -> Result<(), AppError> {
        let mut state = self
            .registry
            .state
            .lock()
            .expect("environment registry lock poisoned");
        if state.capability_revision != self.capability_cycle
            || !matches!(
                state.capability,
                WslCapabilityState::Enabled | WslCapabilityState::Disabling
            )
        {
            return Err(WslRuntime::disabled_error(&self.distro_name));
        }
        state.active_source_owners = state.active_source_owners.saturating_add(1);
        Ok(())
    }

    pub(crate) fn release_source_owner(&self) {
        let mut state = self
            .registry
            .state
            .lock()
            .expect("environment registry lock poisoned");
        state.active_source_owners = state.active_source_owners.saturating_sub(1);
        let retired = state.active_source_owners == 0;
        drop(state);
        if retired {
            self.registry.source_retirement.notify_waiters();
        }
    }

    pub(crate) async fn with_access<T, O, OFut>(&self, operation: O) -> Result<T, AppError>
    where
        O: FnOnce() -> OFut,
        OFut: Future<Output = Result<T, AppError>>,
    {
        let _access = self
            .registry
            .acquire_wsl_access_for_cycle(&self.distro_name, Some(self.capability_cycle))?;
        operation().await
    }

    pub(crate) async fn inspect_filesystem(
        &self,
        request: environment_protocol::InspectionRequest,
    ) -> Result<environment_protocol::InspectionResponse, AppError> {
        self.request_worker_payload(environment_protocol::Message::InspectFilesystem { request })
            .await
    }

    pub(crate) async fn map_host_path(
        &self,
        path: String,
        cancellation: Option<crate::core::mutation::CancellationSignal>,
    ) -> Result<String, AppError> {
        if path.is_empty() || path.contains('\0') {
            return Err(AppError::Validation {
                field: Some("bridgePath".to_string()),
                message: "Host bridge path is invalid".to_string(),
            });
        }
        let message = environment_protocol::Message::MapHostPaths {
            request: environment_protocol::MapHostPathsRequest {
                paths: vec![path.clone()],
                deadline_millis: 10_000,
            },
        };
        let response: environment_protocol::MapHostPathsResponse = match cancellation {
            Some(cancellation) => {
                self.request_worker_payload_with_cancellation(message, cancellation)
                    .await
            }
            None => self.request_worker_payload(message).await,
        }
        .map_err(|error| match error {
            AppError::CapabilityUnavailable { capability, .. }
                if capability == "wslPathMapping" =>
            {
                AppError::StorageMappingUnsupported {
                    path: path.clone(),
                    environment: EnvironmentRef::Wsl {
                        distro_name: self.distro_name.clone(),
                    },
                }
            }
            error => error,
        })?;
        match response.mapped.as_slice() {
            [mapped] if mapped.starts_with('/') && !mapped.contains('\0') => Ok(mapped.clone()),
            _ => Err(AppError::ConfigurationCorrupted {
                message: "invalid WSL path mapping response".to_string(),
            }),
        }
    }

    pub(crate) async fn map_path_to_windows(
        &self,
        path: String,
    ) -> Result<Option<String>, AppError> {
        if !path.starts_with('/') || path.contains('\0') {
            return Err(AppError::Validation {
                field: Some("storagePath".to_string()),
                message: "WSL storage path must be absolute".to_string(),
            });
        }
        let response: environment_protocol::MapWindowsPathsResponse = self
            .request_worker_payload(environment_protocol::Message::MapPathsToWindows {
                request: environment_protocol::MapWindowsPathsRequest {
                    paths: vec![path],
                    deadline_millis: 10_000,
                },
            })
            .await?;
        match response.mapped.as_slice() {
            [mapped] => Ok(mapped.clone()),
            _ => Err(AppError::ConfigurationCorrupted {
                message: "invalid WSL Windows path mapping response".to_string(),
            }),
        }
    }

    pub(crate) async fn request_worker_payload<T>(
        &self,
        message: environment_protocol::Message,
    ) -> Result<T, AppError>
    where
        T: serde::de::DeserializeOwned,
    {
        let payload = self.request_worker_bytes(message, None).await?;
        environment_protocol::decode_payload(&payload).map_err(|error| {
            AppError::ConfigurationCorrupted {
                message: format!("invalid WSL Worker response payload: {error}"),
            }
        })
    }

    pub(crate) async fn request_worker_payload_with_cancellation<T>(
        &self,
        message: environment_protocol::Message,
        cancellation: crate::core::mutation::CancellationSignal,
    ) -> Result<T, AppError>
    where
        T: serde::de::DeserializeOwned,
    {
        let payload = self
            .request_worker_bytes(message, Some(cancellation))
            .await?;
        environment_protocol::decode_payload(&payload).map_err(|error| {
            AppError::ConfigurationCorrupted {
                message: format!("invalid WSL Worker response payload: {error}"),
            }
        })
    }

    pub(crate) async fn request_worker_control_once(
        &self,
        message: environment_protocol::Message,
        cancellation: Option<crate::core::mutation::CancellationSignal>,
        limit: std::time::Duration,
    ) -> Result<(u64, environment_protocol::Message), AppError> {
        let (worker, generation, _access) = self.worker_for_cycle().await?;
        let result = worker
            .request_control_with_cancellation(message, limit, cancellation)
            .await;
        if let Err(error @ AppError::EnvironmentUnavailable { .. }) = &result {
            self.registry.publish_unavailable_if_current(
                &self.distro_name,
                generation,
                error.clone(),
            );
        }
        result.map(|message| (generation, message))
    }

    pub(crate) async fn request_worker_control_for_generation(
        &self,
        generation: u64,
        message: environment_protocol::Message,
        cancellation: Option<crate::core::mutation::CancellationSignal>,
        limit: std::time::Duration,
    ) -> Result<environment_protocol::Message, AppError> {
        let (worker, current_generation, _access) = self.worker_for_cycle().await?;
        self.require_worker_generation(generation, current_generation)?;
        let result = worker
            .request_control_with_cancellation(message, limit, cancellation)
            .await;
        if let Err(error @ AppError::EnvironmentUnavailable { .. }) = &result {
            self.registry.publish_unavailable_if_current(
                &self.distro_name,
                current_generation,
                error.clone(),
            );
        }
        result
    }

    pub(crate) async fn request_worker_payload_for_generation<T>(
        &self,
        generation: u64,
        message: environment_protocol::Message,
        max_payload_bytes: usize,
        cancellation: Option<crate::core::mutation::CancellationSignal>,
        limit: std::time::Duration,
    ) -> Result<T, AppError>
    where
        T: serde::de::DeserializeOwned,
    {
        let (worker, current_generation, _access) = self.worker_for_cycle().await?;
        self.require_worker_generation(generation, current_generation)?;
        let result = worker
            .request_payload_with_limit(message, limit, max_payload_bytes, cancellation)
            .await;
        if let Err(error @ AppError::EnvironmentUnavailable { .. }) = &result {
            self.registry.publish_unavailable_if_current(
                &self.distro_name,
                current_generation,
                error.clone(),
            );
        }
        let payload = result?;
        environment_protocol::decode_payload(&payload).map_err(|error| {
            AppError::ConfigurationCorrupted {
                message: format!("invalid WSL Worker response payload: {error}"),
            }
        })
    }

    pub(crate) async fn request_worker_payload_once<T>(
        &self,
        message: environment_protocol::Message,
        max_payload_bytes: usize,
        cancellation: Option<crate::core::mutation::CancellationSignal>,
        limit: std::time::Duration,
    ) -> Result<(u64, T), AppError>
    where
        T: serde::de::DeserializeOwned,
    {
        let (worker, generation, _access) = self.worker_for_cycle().await?;
        let result = worker
            .request_payload_with_limit(message, limit, max_payload_bytes, cancellation)
            .await;
        if let Err(error @ AppError::EnvironmentUnavailable { .. }) = &result {
            self.registry.publish_unavailable_if_current(
                &self.distro_name,
                generation,
                error.clone(),
            );
        }
        let payload = result?;
        let decoded = environment_protocol::decode_payload(&payload).map_err(|error| {
            AppError::ConfigurationCorrupted {
                message: format!("invalid WSL Worker response payload: {error}"),
            }
        })?;
        Ok((generation, decoded))
    }

    pub(crate) async fn request_worker_bytes_for_generation(
        &self,
        generation: u64,
        message: environment_protocol::Message,
        max_payload_bytes: usize,
        limit: std::time::Duration,
    ) -> Result<Vec<u8>, AppError> {
        let (worker, current_generation, _access) = self.worker_for_cycle().await?;
        self.require_worker_generation(generation, current_generation)?;
        worker
            .request_payload_with_limit(message, limit, max_payload_bytes, None)
            .await
    }

    pub(crate) async fn send_worker_transfer_for_generation(
        &self,
        generation: u64,
        transfer_id: u64,
        payload: &[u8],
        max_payload_bytes: usize,
        limit: std::time::Duration,
    ) -> Result<environment_protocol::Message, AppError> {
        let (worker, current_generation, _access) = self.worker_for_cycle().await?;
        self.require_worker_generation(generation, current_generation)?;
        worker
            .send_prepared_transfer(transfer_id, payload, max_payload_bytes, limit)
            .await
    }

    pub(crate) async fn execute_worker_mutation(
        &self,
        generation: u64,
        resource_id: &str,
        request: &environment_protocol::MutationUnitRequest,
        cancellation: crate::core::mutation::CancellationSignal,
    ) -> Result<environment_protocol::MutationUnitOutcome, AppError> {
        let payload = environment_protocol::encode_payload(request).map_err(|error| {
            AppError::ConfigurationCorrupted {
                message: format!("failed to encode WSL Worker mutation request: {error}"),
            }
        })?;
        if payload.len() > environment_protocol::MAX_MUTATION_TRANSFER_BYTES {
            return Err(AppError::CapabilityUnavailable {
                capability: "wslMutationRequestSize".to_string(),
                path: None,
            });
        }
        let digest = format!("sha256:{:x}", sha2::Sha256::digest(&payload));
        let prepared = self
            .request_worker_control_for_generation(
                generation,
                environment_protocol::Message::PrepareMutationUnit {
                    resource_id: resource_id.to_string(),
                    total_bytes: payload.len() as u64,
                    sha256: digest,
                },
                Some(cancellation.clone()),
                std::time::Duration::from_secs(10),
            )
            .await?;
        let transfer_id = match prepared {
            environment_protocol::Message::TransferReady { transfer_id } => transfer_id,
            environment_protocol::Message::Error { code, phase, .. } => {
                return Err(AppError::ExecutionFailed {
                    message: format!(
                        "WSL Worker mutation preparation failed during {phase}: {code}"
                    ),
                });
            }
            _ => {
                return Err(AppError::ConfigurationCorrupted {
                    message: "invalid WSL Worker mutation preparation response".to_string(),
                });
            }
        };
        let (worker, current_generation, _access) = self.worker_for_cycle().await?;
        self.require_worker_generation(generation, current_generation)?;
        match worker
            .send_prepared_mutation(
                transfer_id,
                &payload,
                cancellation,
                std::time::Duration::from_secs(125),
            )
            .await
        {
            Ok(outcome) => Ok(outcome),
            Err(worker::MutationSessionError {
                accepted_resource_id: Some(accepted),
                error,
            }) => Err(AppError::RecoveryRequired {
                recovery_resource_id: crate::error::RecoveryResourceId::parse(accepted)
                    .unwrap_or_else(|_| {
                        crate::error::RecoveryResourceId::parse(resource_id.to_string())
                            .expect("validated mutation resource ID")
                    }),
                message: error.to_string(),
            }),
            Err(worker::MutationSessionError { error, .. }) => Err(error),
        }
    }

    fn require_worker_generation(&self, expected: u64, actual: u64) -> Result<(), AppError> {
        if expected == actual {
            Ok(())
        } else {
            Err(AppError::EnvironmentUnavailable {
                environment: EnvironmentRef::Wsl {
                    distro_name: self.distro_name.clone(),
                },
                message: "WSL Worker handle belongs to an expired session".to_string(),
            })
        }
    }

    async fn request_worker_bytes(
        &self,
        message: environment_protocol::Message,
        cancellation: Option<crate::core::mutation::CancellationSignal>,
    ) -> Result<Vec<u8>, AppError> {
        for attempt in 0..=1 {
            let (worker, generation, _access) = self.worker_for_cycle().await?;
            let result = match &cancellation {
                Some(cancellation) => {
                    worker
                        .request_payload_with_cancellation(
                            message.clone(),
                            std::time::Duration::from_secs(35),
                            cancellation.clone(),
                        )
                        .await
                }
                None => {
                    worker
                        .request_payload(message.clone(), std::time::Duration::from_secs(35))
                        .await
                }
            };
            match result {
                Ok(payload) => return Ok(payload),
                Err(error @ AppError::EnvironmentUnavailable { .. }) if attempt == 0 => {
                    self.registry.publish_unavailable_if_current(
                        &self.distro_name,
                        generation,
                        error,
                    );
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("WSL Worker read retry has a fixed attempt count")
    }

    async fn worker_for_cycle(
        &self,
    ) -> Result<(worker::WorkerSession, u64, WslAccessPermit), AppError> {
        let access = self
            .registry
            .acquire_wsl_access_for_cycle(&self.distro_name, Some(self.capability_cycle))?;
        if let Some(cached) = self.registry.get_cached(&self.distro_name) {
            if let Some(worker) = cached.worker {
                return Ok((worker, cached.generation, access));
            }
        }
        drop(access);

        let reconnect_lock = self.registry.reconnect_lock(&self.distro_name);
        let _reconnect = reconnect_lock.lock().await;
        let access = self
            .registry
            .acquire_wsl_access_for_cycle(&self.distro_name, Some(self.capability_cycle))?;
        if let Some(cached) = self.registry.get_cached(&self.distro_name) {
            if let Some(worker) = cached.worker {
                return Ok((worker, cached.generation, access));
            }
            self.ensure_wsl2_candidate().await?;
            let worker = match worker::connect_worker(
                &cached.session,
                self.registry.worker_artifact_directory(&self.distro_name)?,
            )
            .await
            {
                Ok(worker) => worker,
                Err(error) => {
                    self.registry.publish_unavailable_if_current(
                        &self.distro_name,
                        cached.generation,
                        error.clone(),
                    );
                    return Err(error);
                }
            };
            let closed = worker.closed_receiver();
            {
                let mut state = self
                    .registry
                    .state
                    .lock()
                    .expect("environment registry lock poisoned");
                let key = EnvironmentKey::wsl(&self.distro_name);
                if state.capability_revision != self.capability_cycle
                    || state.capability != WslCapabilityState::Enabled
                {
                    return Err(WslRuntime::disabled_error(&self.distro_name));
                }
                let current = state.sessions.get_mut(&key).ok_or_else(|| {
                    AppError::EnvironmentUnavailable {
                        environment: EnvironmentRef::Wsl {
                            distro_name: self.distro_name.clone(),
                        },
                        message: "WSL session changed while connecting its Worker".to_string(),
                    }
                })?;
                if current.generation != cached.generation {
                    return Err(WslRuntime::disabled_error(&self.distro_name));
                }
                current.worker = Some(worker.clone());
            }
            self.registry
                .monitor_worker(self.distro_name.clone(), cached.generation, closed);
            return Ok((worker, cached.generation, access));
        }

        self.ensure_wsl2_candidate().await?;
        let mut session = connect_wsl_environment(&self.distro_name).await?;
        let worker = match worker::connect_worker(
            &session,
            self.registry.worker_artifact_directory(&self.distro_name)?,
        )
        .await
        {
            Ok(worker) => worker,
            Err(error) => {
                self.registry.publish_connect_failure_if_cycle(
                    &self.distro_name,
                    self.capability_cycle,
                    error.clone(),
                );
                return Err(error);
            }
        };
        let closed = worker.closed_receiver();
        self.registry
            .insert_with_permit(&mut session, &access, Some(worker.clone()))?;
        let generation = session.runtime_generation;
        self.registry
            .monitor_worker(self.distro_name.clone(), generation, closed);
        Ok((worker, generation, access))
    }

    async fn ensure_wsl2_candidate(&self) -> Result<(), AppError> {
        #[cfg(target_os = "windows")]
        ensure_wsl2_candidate(
            &self.distro_name,
            &discover_wsl_distributions().await.map_err(|error| {
                AppError::EnvironmentUnavailable {
                    environment: EnvironmentRef::Wsl {
                        distro_name: self.distro_name.clone(),
                    },
                    message: error.to_string(),
                }
            })?,
        )?;
        Ok(())
    }

    #[cfg(test)]
    async fn with_session_retry_using<T, C, CFut, O, OFut>(
        &self,
        connector: C,
        operation: O,
    ) -> Result<T, AppError>
    where
        C: FnMut(String) -> CFut,
        CFut: Future<Output = Result<WslSession, AppError>>,
        O: FnMut(WslSession) -> OFut,
        OFut: Future<Output = Result<T, AppError>>,
    {
        self.registry
            .with_session_retry_in_cycle(
                &self.distro_name,
                Some(self.capability_cycle),
                connector,
                operation,
            )
            .await
    }
}

pub struct WslAccessPermit {
    state: Arc<Mutex<WslRuntimeState>>,
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
    registry: WslRuntime,
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

    pub(crate) async fn wait_for_source_retirement(
        &self,
        limit: std::time::Duration,
    ) -> Result<(), ()> {
        let wait = async {
            loop {
                let notified = self.registry.source_retirement.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self
                    .registry
                    .state
                    .lock()
                    .expect("environment registry lock poisoned")
                    .active_source_owners
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
        assert_eq!(state.active_source_owners, 0);
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
    registry: WslRuntime,
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
    let looks_utf16 = bytes.len() >= 2
        && bytes.len().is_multiple_of(2)
        && bytes
            .iter()
            .skip(1)
            .step_by(2)
            .filter(|byte| **byte == 0)
            .count()
            > bytes.len() / 8;
    let decoded = if looks_utf16 {
        let (pairs, remainder) = bytes.as_chunks::<2>();
        debug_assert!(remainder.is_empty());
        let mut utf16 = pairs
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect::<Vec<_>>();
        if utf16.first() == Some(&0xfeff) {
            utf16.remove(0);
        }
        String::from_utf16(&utf16).unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    decoded
        .lines()
        .map(|line| line.trim_matches(['\0', '\r', ' ', '\t']))
        .filter(|line| !line.is_empty())
        .filter_map(parse_wsl_verbose_line)
        .filter_map(|(name, version)| (version == 2).then_some(name))
        .collect()
}

fn parse_wsl_verbose_line(line: &str) -> Option<(String, u8)> {
    let line = line.strip_prefix('*').unwrap_or(line).trim_start();
    let columns = line
        .split('\t')
        .flat_map(|column| column.split("  "))
        .map(str::trim)
        .filter(|column| !column.is_empty())
        .collect::<Vec<_>>();
    if columns.len() < 3 {
        return None;
    }
    let version = columns.last()?.parse().ok()?;
    Some((columns[0].to_string(), version))
}

fn ensure_wsl2_candidate(distro_name: &str, candidates: &[String]) -> Result<(), AppError> {
    if candidates
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(distro_name))
    {
        return Ok(());
    }
    Err(AppError::EnvironmentUnavailable {
        environment: EnvironmentRef::Wsl {
            distro_name: distro_name.to_string(),
        },
        message: "the distribution is not an available WSL 2 environment".to_string(),
    })
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
            message: "wsl.exe --list --verbose timed out".to_string(),
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
                    "wsl.exe --list --verbose exited unsuccessfully".to_string()
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
    if fields.len() != 12 || fields.first().map(String::as_str) != Some("4") {
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
        ("GROK_HOME", 11),
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
async fn discover_wsl_distributions() -> Result<Vec<String>, AppError> {
    let mut command = wsl_command();
    command.args(["--list", "--verbose"]);
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
async fn discover_wsl_distributions() -> Result<Vec<String>, AppError> {
    Ok(Vec::new())
}

#[cfg(target_os = "windows")]
async fn connect_wsl_environment(distro_name: &str) -> Result<WslSession, AppError> {
    const SCRIPT: &str = include_str!("wsl/scripts/session.sh");
    let mut command = wsl_command();
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
    command.kill_on_drop(true);
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
async fn connect_wsl_environment(_distro_name: &str) -> Result<WslSession, AppError> {
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
    use std::future::Future;
    #[cfg(target_os = "linux")]
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{
        interpret_wsl_discovery_outcome, parse_wsl_list_output, parse_wsl_session_output,
        WslDiscoveryCommandOutcome, WslRuntime, WslSession,
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
        let registry = WslRuntime::default();
        let cloned = registry.clone();

        cloned.insert(sample_session("Ubuntu", "alice"));

        assert_eq!(
            registry.get("Ubuntu").expect("shared session").user,
            "alice"
        );
    }

    #[test]
    fn registry_uses_one_cached_session_for_case_insensitive_distro_aliases() {
        let registry = WslRuntime::default();
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
        let registry = WslRuntime::default();

        assert!(Arc::ptr_eq(
            &registry.reconnect_lock("Ubuntu"),
            &registry.reconnect_lock("ubuntu")
        ));
    }

    #[test]
    fn reinserting_identical_session_advances_runtime_generation() {
        let registry = WslRuntime::default();
        registry.insert(sample_session("Ubuntu", "alice"));
        let first = registry.get("Ubuntu").expect("first session");

        registry.insert(sample_session("Ubuntu", "alice"));
        let second = registry.get("Ubuntu").expect("second session");

        assert!(second.runtime_generation > first.runtime_generation);
    }

    #[test]
    fn environment_runtime_event_and_snapshot_share_monotonic_revision() {
        let registry = WslRuntime::default();
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

    fn record_runtime_events(registry: &WslRuntime) -> Arc<Mutex<Vec<EnvironmentRuntimeEvent>>> {
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
        let registry = WslRuntime::default();
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
        let registry = WslRuntime::new(false);
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
    async fn workspace_from_an_old_capability_cycle_cannot_connect_after_reenable() {
        let registry = WslRuntime::default();
        let workspace = registry.workspace("Ubuntu").expect("enabled workspace");

        let disable = registry.begin_disable().expect("begin disable");
        disable
            .wait_for_quiescence(std::time::Duration::from_secs(1))
            .await
            .expect("quiescent runtime");
        disable.commit_disabled();
        registry
            .begin_enable()
            .expect("begin enable")
            .commit_enabled();

        let connects = Arc::new(AtomicUsize::new(0));
        let connector_count = Arc::clone(&connects);
        let error = workspace
            .with_session_retry_using(
                move |_| {
                    connector_count.fetch_add(1, Ordering::SeqCst);
                    async { Ok(sample_session("Ubuntu", "unexpected")) }
                },
                |session| async move { Ok(session.user) },
            )
            .await
            .expect_err("stale workspace");

        assert!(matches!(error, AppError::EnvironmentUnavailable { .. }));
        assert_eq!(connects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn waiting_reconnect_task_cannot_cross_disable_and_reenable_cycle() {
        let registry = WslRuntime::default();
        let reconnect_lock = registry.reconnect_lock("Ubuntu");
        let held_reconnect = reconnect_lock.lock().await;
        let connects = Arc::new(AtomicUsize::new(0));
        let task_connects = Arc::clone(&connects);
        let operation = registry.with_session_retry_using(
            "Ubuntu",
            move |_| {
                task_connects.fetch_add(1, Ordering::SeqCst);
                async { Ok(sample_session("Ubuntu", "unexpected")) }
            },
            |session| async move { Ok(session.user) },
        );
        tokio::pin!(operation);
        std::future::poll_fn(|context| {
            assert!(matches!(
                operation.as_mut().poll(context),
                std::task::Poll::Pending
            ));
            std::task::Poll::Ready(())
        })
        .await;

        let transition = registry.begin_disable().expect("begin disable");
        transition
            .wait_for_quiescence(std::time::Duration::from_secs(1))
            .await
            .expect("no access permit while reconnect lock is pending");
        transition.commit_disabled();
        registry
            .begin_enable()
            .expect("begin enable")
            .commit_enabled();
        drop(held_reconnect);

        let error = operation.await.expect_err("disabled task rejected");
        assert!(matches!(error, AppError::EnvironmentUnavailable { .. }));
        assert_eq!(connects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn disable_timeout_rolls_capability_back_to_enabled() {
        let registry = Arc::new(WslRuntime::default());
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
    async fn disable_waits_for_managed_source_owners_before_committing() {
        let registry = WslRuntime::default();
        let workspace = registry.workspace("Ubuntu").expect("enabled workspace");
        let active_operation = registry
            .acquire_wsl_access("Ubuntu")
            .expect("active WSL operation");
        let transition = registry.begin_disable().expect("begin disable");
        workspace
            .register_source_owner()
            .expect("active operation registers source owner while disabling");
        drop(active_operation);
        transition
            .wait_for_quiescence(std::time::Duration::from_secs(1))
            .await
            .expect("no active command permits");

        assert!(transition
            .wait_for_source_retirement(std::time::Duration::ZERO)
            .await
            .is_err());
        workspace.release_source_owner();
        transition
            .wait_for_source_retirement(std::time::Duration::from_secs(1))
            .await
            .expect("source owner retired");
        transition.commit_disabled();
    }

    #[tokio::test]
    async fn session_retry_reconnects_once_after_environment_unavailable() {
        let registry = WslRuntime::default();
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
        let registry = WslRuntime::default();
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
        let registry = WslRuntime::default();
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
        let registry = WslRuntime::default();
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
        let registry = WslRuntime::default();
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
        let registry = Arc::new(WslRuntime::default());
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
    async fn current_worker_closure_invalidates_the_environment() {
        let registry = WslRuntime::default();
        registry.insert(sample_session("Ubuntu", "alice"));
        let generation = registry.get("Ubuntu").unwrap().runtime_generation;
        let (closed_tx, closed_rx) = tokio::sync::watch::channel(false);
        registry.monitor_worker("Ubuntu".to_string(), generation, closed_rx);

        closed_tx.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if registry
                    .runtime_status("Ubuntu")
                    .is_some_and(|runtime| runtime.status == EnvironmentStatus::Unavailable)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        assert!(registry.get("Ubuntu").is_none());
        assert!(matches!(
            registry.runtime_status("Ubuntu").unwrap().error,
            Some(AppError::EnvironmentUnavailable { .. })
        ));
    }

    #[tokio::test]
    async fn concurrent_session_failures_share_one_reconnect() {
        let registry = Arc::new(WslRuntime::default());
        registry.insert(sample_session("Ubuntu", "old-user"));
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let connects = Arc::new(AtomicUsize::new(0));

        let run = |registry: Arc<WslRuntime>| {
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
            .expect("missing WSL is a normal Native-only result");

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
    fn parses_utf8_and_utf16_verbose_wsl_lists() {
        let text = "  NAME              STATE           VERSION\r\n* Ubuntu-24.04      Running         2\r\n  Legacy            Stopped         1\r\n  Imported Distro   Stopped         2\r\n\r\n";
        let utf16 = text
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let expected = vec!["Ubuntu-24.04", "Imported Distro"];

        assert_eq!(parse_wsl_list_output(text.as_bytes()), expected);
        assert_eq!(parse_wsl_list_output(&utf16), expected);
    }

    #[test]
    fn connection_requires_a_discovered_wsl2_candidate() {
        let candidates = vec!["Ubuntu".to_string(), "Debian".to_string()];

        super::ensure_wsl2_candidate("ubuntu", &candidates).unwrap();
        assert!(matches!(
            super::ensure_wsl2_candidate("Legacy", &candidates),
            Err(AppError::EnvironmentUnavailable {
                environment: EnvironmentRef::Wsl { ref distro_name },
                ..
            }) if distro_name == "Legacy"
        ));
    }

    #[test]
    fn registry_keeps_successful_sessions_by_distro() {
        let registry = WslRuntime::default();
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
        let output = b"4\0alice\x001000\0/home/alice\0/home/alice/.state\0/home/alice/.config\0/opt/codex\0/opt/claude\0\0\0\0/opt/grok\0";
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
        assert_eq!(session.environment["GROK_HOME"], "/opt/grok");
    }

    #[test]
    fn rejects_legacy_session_shapes() {
        for output in [
            b"1\0alice\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\x001\0".as_slice(),
            b"2\0alice\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\x001\x001\x001\x001\x001\x001\0".as_slice(),
            b"3\0alice\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\0".as_slice(),
        ] {
            assert!(parse_wsl_session_output("Ubuntu", output).is_err());
        }
    }

    #[test]
    fn rejects_invalid_current_session_payloads() {
        for output in [
            b"4\0alice\0not-a-uid\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\0\0".as_slice(),
            b"4\0alice\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\0\0unexpected\0"
                .as_slice(),
            b"4\0\xff\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\0\0".as_slice(),
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
    fn bundled_session_script_reports_identity_without_business_tool_probes() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("session fixture");
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("fixture bin");
        for command in [
            "git",
            "timeout",
            "xargs",
            "sort",
            "sha256sum",
            "readlink",
            "stat",
        ] {
            let command_path = bin.join(command);
            std::fs::write(&command_path, "#!/bin/sh\nexit 99\n")
                .expect("failing business tool fixture");
            std::fs::set_permissions(&command_path, std::fs::Permissions::from_mode(0o755))
                .expect("make business tool fixture executable");
        }
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = command_output_with_timeout(
            Command::new("/bin/sh")
                .arg("-c")
                .arg(include_str!("wsl/scripts/session.sh"))
                .arg("--")
                .arg("session")
                .env("GROK_HOME", "/opt/grok")
                .env("PATH", path),
            std::time::Duration::from_secs(10),
        )
        .expect("session script");

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.starts_with(b"4\0"));
        let session = parse_wsl_session_output("Ubuntu", &output.stdout)
            .expect("parse bundled session output");
        assert!(!session.user.is_empty());
        assert!(session.home.starts_with('/'));
        assert_eq!(session.environment["GROK_HOME"], "/opt/grok");
    }

    #[test]
    fn parses_empty_xdg_state_home_without_shifting_fields() {
        let output = b"4\0alice\x001000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\0\0";
        let session = parse_wsl_session_output("Ubuntu", output).expect("parse session");

        assert_eq!(session.xdg_state_home, None);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    #[ignore = "requires Windows with a WSL 2 distribution"]
    async fn real_wsl2_worker_maps_a_windows_directory_round_trip() {
        let distro =
            std::env::var("SKILL_DECK_TEST_WSL_DISTRO").unwrap_or_else(|_| "Ubuntu".to_string());
        let root = tempfile::tempdir().expect("Windows path fixture");
        let directory = root.path().join("Skill Deck 项目");
        std::fs::create_dir(&directory).expect("create Windows path fixture");
        let runtime = WslRuntime::for_wsl_test();
        let workspace = runtime.workspace(&distro).expect("WSL workspace");

        let mapped = workspace
            .map_host_path(directory.to_string_lossy().into_owned(), None)
            .await
            .expect("map Windows path into WSL");
        assert!(mapped.starts_with('/'));
        let round_trip = workspace
            .map_path_to_windows(mapped)
            .await
            .expect("map WSL path into Windows")
            .expect("Windows projection");

        assert_eq!(
            std::fs::canonicalize(round_trip).expect("canonical mapped path"),
            std::fs::canonicalize(directory).expect("canonical fixture path")
        );
    }
}
