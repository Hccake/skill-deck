use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::time::Duration;

use crate::core::agent_availability::AgentAvailabilityKind;
use crate::core::agent_definition::{
    AgentDefinition, AgentId, DetectionSpec, LegacyPath, LegacyPathScope, PathSpec, ScopeDefinition,
};
use crate::core::agent_registry::AgentRegistrySnapshot;
use crate::core::agents::{AgentScopeTarget, AgentType};
use crate::core::paths::PATHS;
use crate::environment::types::{EnvironmentRef, EnvironmentStatus};
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{decode_nul_records, run_wsl_script};
use crate::error::AppError;

const WSL_PATH_METADATA_SCRIPT: &str = r#"
missing_kind() {
  probe=${1%/*}
  [ -n "$probe" ] || probe=/
  while [ "$probe" != / ] && [ ! -e "$probe" ] && [ ! -L "$probe" ]; do
    next=${probe%/*}
    [ -n "$next" ] || next=/
    [ "$next" != "$probe" ] || break
    probe=$next
  done
  if [ -d "$probe" ] && [ ! -x "$probe" ]; then
    printf inaccessible
  else
    printf missing
  fi
}

printf '1\0'
while [ "$#" -ge 2 ]; do
  path=$1
  inspect_eve=$2
  shift 2
  if [ -L "$path" ]; then
    if [ ! -e "$path" ]; then
      kind=broken-link
    elif [ -d "$path" ]; then
      kind=symlink-directory
    else
      kind=symlink-other
    fi
  elif [ -d "$path" ]; then
    kind=directory
  elif [ -e "$path" ]; then
    kind=other
  else
    kind=$(missing_kind "$path")
  fi
  printf 'path\0%s\0%s\0' "$path" "$kind"
  if [ "$inspect_eve" = 1 ] && [ -f "$path" ]; then
    if payload=$(dd if="$path" bs=1048576 count=1 2>/dev/null); then
      if [ -n "$payload" ]; then
        printf 'eve\0%s\0' "$payload"
      else
        printf 'eve-empty\0-\0'
      fi
    else
      printf 'eve-unreadable\0-\0'
    fi
  else
    printf 'none\0-\0'
  fi
done
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEnvironmentContext {
    pub home: String,
    pub config_home: String,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentContext {
    pub environment: EnvironmentRef,
    pub home: String,
    pub config_home: String,
    pub environment_variables: BTreeMap<String, String>,
    pub availability: EnvironmentStatus,
    pub revision: String,
    pub wsl_session: Option<WslSession>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum DetectionState {
    Detected,
    NotDetected,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum DirectoryPresenceState {
    Present,
    Missing,
    LegacyPath,
    BrokenLink,
    ConflictingEntry,
    UnsafePath,
    EnvironmentUnavailable,
    ProjectNotSelected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ResolvedAgentScope {
    pub enabled: bool,
    pub reads_shared: bool,
    pub shared_path: Option<String>,
    pub private_path: Option<String>,
    pub read_paths: Vec<String>,
    pub shared_presence: Option<DirectoryPresenceState>,
    pub private_presence: Option<DirectoryPresenceState>,
    pub legacy_paths: Vec<ResolvedPathPresence>,
}

impl ResolvedAgentScope {
    fn disabled() -> Self {
        Self {
            enabled: false,
            reads_shared: false,
            shared_path: None,
            private_path: None,
            read_paths: Vec::new(),
            shared_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ResolvedPathPresence {
    pub path: Option<String>,
    pub presence: DirectoryPresenceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ResolvedAgent {
    pub definition: AgentDefinition,
    pub detection: DetectionState,
    pub global: ResolvedAgentScope,
    pub project: ResolvedAgentScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentRuntimeSnapshot {
    pub registry_revision: String,
    pub environment_revision: String,
    pub environment: EnvironmentRef,
    pub availability: EnvironmentStatus,
    pub project_path: Option<String>,
    pub agents: BTreeMap<AgentId, ResolvedAgent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathEntryKind {
    Missing,
    Directory,
    SymlinkDirectory,
    SymlinkOther,
    Other,
    BrokenLink,
    Inaccessible,
}

impl PathEntryKind {
    fn exists(self) -> bool {
        matches!(
            self,
            Self::Directory | Self::SymlinkDirectory | Self::SymlinkOther | Self::Other
        )
    }

    fn directory_exists(self) -> bool {
        matches!(self, Self::Directory | Self::SymlinkDirectory)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathMetadata {
    entry_kind: PathEntryKind,
    eve_package: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathQuery {
    path: String,
    inspect_eve_package: bool,
}

type MetadataQuery =
    Arc<dyn Fn(&[PathQuery]) -> Result<BTreeMap<String, PathMetadata>, AppError> + Send + Sync>;

enum MetadataBackend {
    Host,
    Wsl(WslSession),
    Unavailable,
    Custom(MetadataQuery),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RuntimeCacheKey {
    registry_revision: String,
    environment_revision: String,
    project_path: Option<String>,
}

pub struct AgentEnvironmentResolver {
    context: AgentEnvironmentContext,
    environment_context: EnvironmentContext,
    metadata_backend: MetadataBackend,
    cache: Mutex<BTreeMap<RuntimeCacheKey, AgentRuntimeSnapshot>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEnvironmentTarget {
    pub agent: AgentType,
    pub display_name: String,
    pub shared_path: String,
    pub private_path: Option<String>,
    pub availability: AgentAvailabilityKind,
    pub default_available: bool,
    pub detection_paths: Vec<String>,
}

impl AgentEnvironmentTarget {
    pub fn scope_target(&self, is_global: bool) -> AgentScopeTarget {
        let supported = self.availability != AgentAvailabilityKind::Unsupported;
        let configured_path = self
            .private_path
            .clone()
            .unwrap_or_else(|| self.shared_path.clone());
        let read_paths = match self.availability {
            AgentAvailabilityKind::SharedOnly => vec![self.shared_path.clone()],
            AgentAvailabilityKind::SharedCompatible => {
                let mut paths = vec![self.shared_path.clone()];
                if let Some(private_path) = &self.private_path {
                    paths.push(private_path.clone());
                }
                paths
            }
            AgentAvailabilityKind::PrivateRequired | AgentAvailabilityKind::Unknown => {
                self.private_path.clone().into_iter().collect()
            }
            AgentAvailabilityKind::Unsupported => Vec::new(),
        };

        AgentScopeTarget {
            supported,
            automatic: self.default_available,
            path: if !supported {
                String::new()
            } else if is_global {
                configured_path.clone()
            } else {
                self.agent.config().skills_dir.to_string()
            },
            availability: self.availability,
            default_available: self.default_available,
            shared_path: self.shared_path.clone(),
            install_path: if self.default_available {
                self.shared_path.clone()
            } else {
                configured_path
            },
            read_paths,
            private_path: self.private_path.clone(),
        }
    }
}

impl AgentEnvironmentResolver {
    pub fn new(context: AgentEnvironmentContext) -> Self {
        Self::from_environment(EnvironmentContext {
            environment: EnvironmentRef::Host,
            home: context.home,
            config_home: context.config_home,
            environment_variables: context.env,
            availability: EnvironmentStatus::Available,
            revision: "compatibility-host".to_string(),
            wsl_session: None,
        })
    }

    pub fn from_environment(context: EnvironmentContext) -> Self {
        let compatibility_context = AgentEnvironmentContext {
            home: context.home.clone(),
            config_home: context.config_home.clone(),
            env: context.environment_variables.clone(),
        };
        let metadata_backend = metadata_backend(&context);
        Self {
            context: compatibility_context,
            environment_context: context,
            metadata_backend,
            cache: Mutex::new(BTreeMap::new()),
        }
    }

    #[cfg(test)]
    fn with_metadata_query(
        context: EnvironmentContext,
        query: impl Fn(&[PathQuery]) -> Result<BTreeMap<String, PathMetadata>, AppError>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        let mut resolver = Self::from_environment(context);
        resolver.metadata_backend = MetadataBackend::Custom(Arc::new(query));
        resolver
    }

    #[cfg(test)]
    fn replace_environment_context(&mut self, context: EnvironmentContext) {
        self.context = AgentEnvironmentContext {
            home: context.home.clone(),
            config_home: context.config_home.clone(),
            env: context.environment_variables.clone(),
        };
        if !matches!(self.metadata_backend, MetadataBackend::Custom(_)) {
            self.metadata_backend = metadata_backend(&context);
        }
        self.environment_context = context;
    }

    pub async fn resolve_registry(
        &self,
        snapshot: &AgentRegistrySnapshot,
        project_path: Option<&str>,
    ) -> Result<AgentRuntimeSnapshot, AppError> {
        let project_path = project_path.map(|path| normalize_path(path, &self.environment_context));
        let cache_key = RuntimeCacheKey {
            registry_revision: snapshot.revision.clone(),
            environment_revision: self.environment_context.revision.clone(),
            project_path: project_path.clone(),
        };
        if let Some(cached) = self
            .cache
            .lock()
            .expect("agent runtime cache lock poisoned")
            .get(&cache_key)
            .cloned()
        {
            return Ok(cached);
        }

        let backend_available = !matches!(self.metadata_backend, MetadataBackend::Unavailable);
        let mut effective_availability = self.environment_context.availability;
        if effective_availability == EnvironmentStatus::Available && !backend_available {
            effective_availability = EnvironmentStatus::Unavailable;
        }
        let (metadata, environment_available) =
            if effective_availability == EnvironmentStatus::Available {
                let queries = self.collect_queries(snapshot, project_path.as_deref());
                match self.query_metadata(&queries).await {
                    Ok(metadata) => (metadata, true),
                    Err(AppError::EnvironmentUnavailable { .. }) => {
                        effective_availability = EnvironmentStatus::Unavailable;
                        (BTreeMap::new(), false)
                    }
                    Err(error) => return Err(error),
                }
            } else {
                (BTreeMap::new(), false)
            };

        let agents = snapshot
            .active_definitions
            .iter()
            .map(|(id, definition)| {
                let detection = self.resolve_detection(
                    definition,
                    project_path.as_deref(),
                    &metadata,
                    environment_available,
                );
                let global = self.resolve_scope(
                    &definition.global,
                    &definition.legacy_paths,
                    LegacyPathScope::Global,
                    true,
                    project_path.as_deref(),
                    &metadata,
                    environment_available,
                );
                let project = self.resolve_scope(
                    &definition.project,
                    &definition.legacy_paths,
                    LegacyPathScope::Project,
                    false,
                    project_path.as_deref(),
                    &metadata,
                    environment_available,
                );
                (
                    id.clone(),
                    ResolvedAgent {
                        definition: definition.clone(),
                        detection,
                        global,
                        project,
                    },
                )
            })
            .collect();
        let resolved = AgentRuntimeSnapshot {
            registry_revision: snapshot.revision.clone(),
            environment_revision: self.environment_context.revision.clone(),
            environment: self.environment_context.environment.clone(),
            availability: effective_availability,
            project_path,
            agents,
        };
        self.cache
            .lock()
            .expect("agent runtime cache lock poisoned")
            .insert(cache_key, resolved.clone());
        Ok(resolved)
    }

    pub fn invalidate_cache(&self) {
        self.cache
            .lock()
            .expect("agent runtime cache lock poisoned")
            .clear();
    }

    fn collect_queries(
        &self,
        snapshot: &AgentRegistrySnapshot,
        project_path: Option<&str>,
    ) -> Vec<PathQuery> {
        let mut queries = BTreeMap::new();
        for definition in snapshot.active_definitions.values() {
            self.collect_detection_queries(definition, project_path, &mut queries);
            self.collect_scope_queries(&definition.global, true, project_path, &mut queries);
            self.collect_scope_queries(&definition.project, false, project_path, &mut queries);
            for legacy in &definition.legacy_paths {
                let matches_scope = matches!(legacy.scope, LegacyPathScope::Global)
                    || project_path.is_some() && matches!(legacy.scope, LegacyPathScope::Project);
                if matches_scope {
                    self.collect_path_spec_queries(&legacy.path, project_path, false, &mut queries);
                }
            }
        }
        queries.into_values().collect()
    }

    fn collect_detection_queries(
        &self,
        definition: &AgentDefinition,
        project_path: Option<&str>,
        queries: &mut BTreeMap<String, PathQuery>,
    ) {
        match &definition.detection {
            DetectionSpec::AnyPathExists { paths } => {
                for path in paths {
                    self.collect_path_spec_queries(path, project_path, false, queries);
                }
            }
            DetectionSpec::Eve => {
                if let Some(project_path) = project_path {
                    insert_query(
                        queries,
                        join_resolved(project_path, "agent", &self.environment_context),
                        false,
                        &self.environment_context,
                    );
                    insert_query(
                        queries,
                        join_resolved(project_path, "package.json", &self.environment_context),
                        true,
                        &self.environment_context,
                    );
                }
            }
        }
    }

    fn collect_scope_queries(
        &self,
        scope: &ScopeDefinition,
        is_global: bool,
        project_path: Option<&str>,
        queries: &mut BTreeMap<String, PathQuery>,
    ) {
        if !scope.enabled {
            return;
        }
        if let Some(shared_path) = self.shared_path(is_global, project_path) {
            insert_query(queries, shared_path, false, &self.environment_context);
        }
        if let Some(private_path) = &scope.private_path {
            self.collect_path_spec_queries(private_path, project_path, false, queries);
        }
    }

    fn collect_path_spec_queries(
        &self,
        spec: &PathSpec,
        project_path: Option<&str>,
        inspect_eve_package: bool,
        queries: &mut BTreeMap<String, PathQuery>,
    ) {
        match spec {
            PathSpec::FirstExisting {
                candidates,
                fallback,
            } => {
                for candidate in candidates {
                    self.collect_path_spec_queries(
                        candidate,
                        project_path,
                        inspect_eve_package,
                        queries,
                    );
                }
                self.collect_path_spec_queries(
                    fallback,
                    project_path,
                    inspect_eve_package,
                    queries,
                );
            }
            PathSpec::EnvironmentVariable {
                name,
                relative_path,
                fallback,
            } => {
                if let Some(base) = self
                    .environment_context
                    .environment_variables
                    .get(name)
                    .filter(|value| !value.trim().is_empty())
                {
                    let path = join_resolved(base, relative_path, &self.environment_context);
                    if absolute_path_is_compatible(&path, &self.environment_context) {
                        insert_query(
                            queries,
                            path,
                            inspect_eve_package,
                            &self.environment_context,
                        );
                    }
                } else {
                    self.collect_path_spec_queries(
                        fallback,
                        project_path,
                        inspect_eve_package,
                        queries,
                    );
                }
            }
            _ => {
                if let PathResolution::Resolved(path) = self.resolve_simple_path(spec, project_path)
                {
                    insert_query(
                        queries,
                        path,
                        inspect_eve_package,
                        &self.environment_context,
                    );
                }
            }
        }
    }

    async fn query_metadata(
        &self,
        queries: &[PathQuery],
    ) -> Result<BTreeMap<String, PathMetadata>, AppError> {
        if queries.is_empty() {
            return Ok(BTreeMap::new());
        }
        let metadata = match &self.metadata_backend {
            MetadataBackend::Host => query_host_metadata(queries),
            MetadataBackend::Wsl(session) => query_wsl_metadata(session, queries).await,
            MetadataBackend::Custom(query) => query(queries),
            MetadataBackend::Unavailable => Ok(BTreeMap::new()),
        }?;
        Ok(metadata
            .into_iter()
            .map(|(path, metadata)| (path_key(&path, &self.environment_context), metadata))
            .collect())
    }

    fn resolve_detection(
        &self,
        definition: &AgentDefinition,
        project_path: Option<&str>,
        metadata: &BTreeMap<String, PathMetadata>,
        environment_available: bool,
    ) -> DetectionState {
        if !environment_available {
            return DetectionState::Indeterminate;
        }
        match &definition.detection {
            DetectionSpec::AnyPathExists { paths } => {
                let mut indeterminate = false;
                for path in paths {
                    let PathResolution::Resolved(path) =
                        self.resolve_path(path, project_path, metadata)
                    else {
                        indeterminate = true;
                        continue;
                    };
                    let Some(entry) = metadata.get(&path_key(&path, &self.environment_context))
                    else {
                        indeterminate = true;
                        continue;
                    };
                    if entry.entry_kind == PathEntryKind::Inaccessible {
                        indeterminate = true;
                        continue;
                    }
                    if entry.entry_kind.exists() {
                        return DetectionState::Detected;
                    }
                }
                if indeterminate {
                    DetectionState::Indeterminate
                } else {
                    DetectionState::NotDetected
                }
            }
            DetectionSpec::Eve => {
                let Some(project_path) = project_path else {
                    return DetectionState::Indeterminate;
                };
                let agent_path = join_resolved(project_path, "agent", &self.environment_context);
                let package_path =
                    join_resolved(project_path, "package.json", &self.environment_context);
                let agent_metadata =
                    metadata.get(&path_key(&agent_path, &self.environment_context));
                let package_metadata =
                    metadata.get(&path_key(&package_path, &self.environment_context));
                let (Some(agent_metadata), Some(package_metadata)) =
                    (agent_metadata, package_metadata)
                else {
                    return DetectionState::Indeterminate;
                };
                if agent_metadata.entry_kind == PathEntryKind::Inaccessible
                    || package_metadata.entry_kind == PathEntryKind::Inaccessible
                {
                    return DetectionState::Indeterminate;
                }
                if !agent_metadata.entry_kind.directory_exists() {
                    return DetectionState::NotDetected;
                }
                match package_metadata.eve_package {
                    Some(true) => DetectionState::Detected,
                    Some(false) => DetectionState::NotDetected,
                    None if matches!(
                        package_metadata.entry_kind,
                        PathEntryKind::Other | PathEntryKind::SymlinkOther
                    ) =>
                    {
                        DetectionState::Indeterminate
                    }
                    None => DetectionState::NotDetected,
                }
            }
        }
    }

    fn resolve_scope(
        &self,
        scope: &ScopeDefinition,
        legacy_paths: &[LegacyPath],
        legacy_scope: LegacyPathScope,
        is_global: bool,
        project_path: Option<&str>,
        metadata: &BTreeMap<String, PathMetadata>,
        environment_available: bool,
    ) -> ResolvedAgentScope {
        if !scope.enabled {
            return ResolvedAgentScope::disabled();
        }
        let shared_resolution = match self.shared_path(is_global, project_path) {
            Some(path) => PathResolution::Resolved(path),
            None => PathResolution::ProjectNotSelected,
        };
        let private_resolution = scope
            .private_path
            .as_ref()
            .map(|path| self.resolve_path(path, project_path, metadata));
        let shared_path = shared_resolution.path().map(ToString::to_string);
        let private_path = private_resolution
            .as_ref()
            .and_then(PathResolution::path)
            .map(ToString::to_string);
        let mut read_paths = Vec::new();
        if scope.reads_shared {
            if let Some(path) = &shared_path {
                read_paths.push(path.clone());
            }
        }
        if let Some(path) = &private_path {
            read_paths.push(path.clone());
        }
        let legacy_paths = legacy_paths
            .iter()
            .filter(|legacy| legacy.scope == legacy_scope)
            .map(|legacy| {
                let resolution = self.resolve_path(&legacy.path, project_path, metadata);
                let presence = match path_presence(
                    &resolution,
                    metadata,
                    environment_available,
                    &self.environment_context,
                ) {
                    DirectoryPresenceState::Present => DirectoryPresenceState::LegacyPath,
                    other => other,
                };
                ResolvedPathPresence {
                    path: resolution.path().map(ToString::to_string),
                    presence,
                }
            })
            .collect();
        ResolvedAgentScope {
            enabled: true,
            reads_shared: scope.reads_shared,
            shared_path,
            private_path,
            read_paths,
            shared_presence: Some(path_presence(
                &shared_resolution,
                metadata,
                environment_available,
                &self.environment_context,
            )),
            private_presence: private_resolution.as_ref().map(|resolution| {
                path_presence(
                    resolution,
                    metadata,
                    environment_available,
                    &self.environment_context,
                )
            }),
            legacy_paths,
        }
    }

    fn shared_path(&self, is_global: bool, project_path: Option<&str>) -> Option<String> {
        if is_global {
            Some(join_resolved(
                &self.environment_context.home,
                ".agents/skills",
                &self.environment_context,
            ))
        } else {
            project_path.map(|project_path| {
                join_resolved(project_path, ".agents/skills", &self.environment_context)
            })
        }
    }

    fn resolve_path(
        &self,
        spec: &PathSpec,
        project_path: Option<&str>,
        metadata: &BTreeMap<String, PathMetadata>,
    ) -> PathResolution {
        match spec {
            PathSpec::FirstExisting {
                candidates,
                fallback,
            } => {
                for candidate in candidates {
                    let resolved = self.resolve_path(candidate, project_path, metadata);
                    let Some(path) = resolved.path() else {
                        return resolved;
                    };
                    match metadata
                        .get(&path_key(path, &self.environment_context))
                        .map(|entry| entry.entry_kind)
                    {
                        Some(kind) if kind.directory_exists() => return resolved,
                        Some(PathEntryKind::Inaccessible) | None => {
                            return PathResolution::Indeterminate;
                        }
                        Some(_) => {}
                    }
                }
                self.resolve_path(fallback, project_path, metadata)
            }
            PathSpec::EnvironmentVariable {
                name,
                relative_path,
                fallback,
            } => self
                .environment_context
                .environment_variables
                .get(name)
                .filter(|value| !value.trim().is_empty())
                .map(|base| {
                    let path = join_resolved(base, relative_path, &self.environment_context);
                    if absolute_path_is_compatible(&path, &self.environment_context) {
                        PathResolution::Resolved(path)
                    } else {
                        PathResolution::Unsafe
                    }
                })
                .unwrap_or_else(|| self.resolve_path(fallback, project_path, metadata)),
            _ => self.resolve_simple_path(spec, project_path),
        }
    }

    fn resolve_simple_path(&self, spec: &PathSpec, project_path: Option<&str>) -> PathResolution {
        match spec {
            PathSpec::Home { relative_path } => PathResolution::Resolved(join_resolved(
                &self.environment_context.home,
                relative_path,
                &self.environment_context,
            )),
            PathSpec::ConfigHome { relative_path } => PathResolution::Resolved(join_resolved(
                &self.environment_context.config_home,
                relative_path,
                &self.environment_context,
            )),
            PathSpec::Project { relative_path } => project_path
                .map(|project_path| {
                    PathResolution::Resolved(join_resolved(
                        project_path,
                        relative_path,
                        &self.environment_context,
                    ))
                })
                .unwrap_or(PathResolution::ProjectNotSelected),
            PathSpec::EnvironmentVariable { .. } => {
                unreachable!("handled by resolve_path and collect_path_spec_queries")
            }
            PathSpec::Absolute { path } => {
                if absolute_path_is_compatible(path, &self.environment_context) {
                    PathResolution::Resolved(normalize_path(path, &self.environment_context))
                } else {
                    PathResolution::Unsafe
                }
            }
            PathSpec::FirstExisting { .. } => unreachable!("handled by resolve_path"),
        }
    }

    pub fn project_skills_dir(&self, agent: AgentType, project_path: &str) -> String {
        join_posix(project_path, agent.config().skills_dir)
    }

    pub fn global_skills_dir(&self, agent: AgentType) -> Option<String> {
        let override_home = match agent {
            AgentType::Codex => self.env_home("CODEX_HOME", ".codex"),
            AgentType::ClaudeCode => self.env_home("CLAUDE_CONFIG_DIR", ".claude"),
            AgentType::MistralVibe => self.env_home("VIBE_HOME", ".vibe"),
            AgentType::HermesAgent => self.env_home("HERMES_HOME", ".hermes"),
            AgentType::AutohandCode => self.env_home("AUTOHAND_HOME", ".autohand"),
            AgentType::Openclaw => Some(join_posix(&self.context.home, ".openclaw")),
            _ => None,
        };
        if let Some(home) = override_home {
            return Some(join_posix(&home, "skills"));
        }

        let configured = agent.config().global_skills_dir?;
        if let Ok(relative) = configured.strip_prefix(&PATHS.config_home) {
            return Some(join_posix(
                &self.context.config_home,
                &path_to_posix(relative),
            ));
        }
        if let Ok(relative) = configured.strip_prefix(&PATHS.home) {
            return Some(join_posix(&self.context.home, &path_to_posix(relative)));
        }
        None
    }

    pub fn target(
        &self,
        agent: AgentType,
        is_global: bool,
        project_path: &str,
    ) -> AgentEnvironmentTarget {
        let shared_path = if is_global {
            join_posix(&self.context.home, ".agents/skills")
        } else {
            join_posix(project_path, ".agents/skills")
        };
        let configured_private = if is_global {
            self.global_skills_dir(agent)
        } else if agent.config().skills_dir.trim().is_empty() {
            None
        } else {
            Some(self.project_skills_dir(agent, project_path))
        };

        let (supported, default_available, availability) = if is_global {
            match configured_private.as_deref() {
                None => (false, false, AgentAvailabilityKind::Unsupported),
                Some(_) if matches!(global_official_support(agent), OfficialSharedSupport::No) => {
                    (true, false, AgentAvailabilityKind::PrivateRequired)
                }
                Some(private) if same_posix_path(private, &shared_path) => {
                    (true, true, AgentAvailabilityKind::SharedOnly)
                }
                Some(_) if matches!(global_official_support(agent), OfficialSharedSupport::Yes) => {
                    (true, true, AgentAvailabilityKind::SharedCompatible)
                }
                Some(_) => (true, false, AgentAvailabilityKind::Unknown),
            }
        } else {
            match configured_private.as_deref() {
                None => (false, false, AgentAvailabilityKind::Unsupported),
                Some(private) if same_posix_path(private, &shared_path) => {
                    (true, true, AgentAvailabilityKind::SharedOnly)
                }
                Some(_) => (true, false, AgentAvailabilityKind::PrivateRequired),
            }
        };
        let private_path = configured_private.filter(|path| !same_posix_path(path, &shared_path));

        AgentEnvironmentTarget {
            agent,
            display_name: agent.config().display_name.to_string(),
            shared_path,
            private_path: supported.then_some(private_path).flatten(),
            availability,
            default_available,
            detection_paths: self.detection_paths(agent, project_path),
        }
    }

    fn env_home(&self, key: &str, fallback: &str) -> Option<String> {
        Some(
            self.context
                .env
                .get(key)
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| join_posix(&self.context.home, fallback)),
        )
    }

    fn detection_paths(&self, agent: AgentType, project_path: &str) -> Vec<String> {
        let home = &self.context.home;
        let config = &self.context.config_home;
        let paths = match agent {
            AgentType::Amp => vec![join_posix(config, "amp")],
            AgentType::Antigravity => vec![join_posix(home, ".gemini/antigravity")],
            AgentType::AntigravityCli => vec![join_posix(home, ".gemini/antigravity-cli")],
            AgentType::Cline => vec![join_posix(home, ".cline")],
            AgentType::Codex => vec![
                self.env_home("CODEX_HOME", ".codex").expect("codex home"),
                "/etc/codex".to_string(),
            ],
            AgentType::Cursor => vec![join_posix(home, ".cursor")],
            AgentType::Deepagents => vec![join_posix(home, ".deepagents")],
            AgentType::Dexto => vec![join_posix(home, ".dexto")],
            AgentType::Eve => vec![
                join_posix(project_path, "agent"),
                join_posix(project_path, "package.json"),
            ],
            AgentType::Firebender => vec![join_posix(home, ".firebender")],
            AgentType::GeminiCli => vec![join_posix(home, ".gemini")],
            AgentType::GithubCopilot => vec![join_posix(home, ".copilot")],
            AgentType::KimiCodeCli => {
                vec![join_posix(home, ".kimi-code"), join_posix(home, ".kimi")]
            }
            AgentType::Loaf => vec![join_posix(home, ".loaf")],
            AgentType::Opencode => vec![join_posix(config, "opencode")],
            AgentType::Promptscript => vec![
                join_posix(project_path, ".promptscript"),
                join_posix(project_path, "promptscript.yaml"),
            ],
            AgentType::Replit => vec![join_posix(project_path, ".replit")],
            AgentType::Warp => vec![join_posix(home, ".warp")],
            AgentType::Zed => vec![join_posix(config, "zed")],
            _ => self
                .global_skills_dir(agent)
                .and_then(|path| parent_posix(&path))
                .into_iter()
                .collect(),
        };
        paths
            .into_iter()
            .filter(|path| !path.trim().is_empty())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PathResolution {
    Resolved(String),
    Indeterminate,
    Unsafe,
    ProjectNotSelected,
}

impl PathResolution {
    fn path(&self) -> Option<&str> {
        match self {
            Self::Resolved(path) => Some(path),
            Self::Indeterminate | Self::Unsafe | Self::ProjectNotSelected => None,
        }
    }
}

fn metadata_backend(context: &EnvironmentContext) -> MetadataBackend {
    if context.availability != EnvironmentStatus::Available {
        return MetadataBackend::Unavailable;
    }
    match &context.environment {
        EnvironmentRef::Host => MetadataBackend::Host,
        EnvironmentRef::Wsl { distro_name } => context
            .wsl_session
            .as_ref()
            .filter(|session| session.distro_name == *distro_name)
            .cloned()
            .map(MetadataBackend::Wsl)
            .unwrap_or(MetadataBackend::Unavailable),
    }
}

fn insert_query(
    queries: &mut BTreeMap<String, PathQuery>,
    path: String,
    inspect_eve_package: bool,
    context: &EnvironmentContext,
) {
    let key = path_key(&path, context);
    queries
        .entry(key)
        .and_modify(|query| query.inspect_eve_package |= inspect_eve_package)
        .or_insert(PathQuery {
            path,
            inspect_eve_package,
        });
}

fn query_host_metadata(queries: &[PathQuery]) -> Result<BTreeMap<String, PathMetadata>, AppError> {
    Ok(queries
        .iter()
        .map(|query| {
            let path = Path::new(&query.path);
            let entry_kind = match fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_symlink() => match fs::metadata(path) {
                    Ok(target) if target.is_dir() => PathEntryKind::SymlinkDirectory,
                    Ok(_) => PathEntryKind::SymlinkOther,
                    Err(_) => PathEntryKind::BrokenLink,
                },
                Ok(metadata) if metadata.is_dir() => PathEntryKind::Directory,
                Ok(_) => PathEntryKind::Other,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    PathEntryKind::Missing
                }
                Err(_) => PathEntryKind::Inaccessible,
            };
            let eve_package = query
                .inspect_eve_package
                .then(|| read_eve_package(path))
                .flatten();
            (
                query.path.clone(),
                PathMetadata {
                    entry_kind,
                    eve_package,
                },
            )
        })
        .collect())
}

fn read_eve_package(path: &Path) -> Option<bool> {
    let file = fs::File::open(path).ok()?;
    let mut content = String::new();
    file.take(1024 * 1024).read_to_string(&mut content).ok()?;
    Some(is_eve_package(&content))
}

async fn query_wsl_metadata(
    session: &WslSession,
    queries: &[PathQuery],
) -> Result<BTreeMap<String, PathMetadata>, AppError> {
    let mut args = Vec::with_capacity(queries.len() * 2);
    for query in queries {
        args.push(query.path.clone());
        args.push(if query.inspect_eve_package { "1" } else { "0" }.to_string());
    }
    let output = run_wsl_script(
        session,
        WSL_PATH_METADATA_SCRIPT,
        &args,
        Vec::new(),
        Duration::from_secs(20),
    )
    .await?;
    parse_wsl_path_metadata(&output)
}

fn parse_wsl_path_metadata(bytes: &[u8]) -> Result<BTreeMap<String, PathMetadata>, AppError> {
    let records = decode_nul_records(bytes);
    if records.first().map(String::as_str) != Some("1") {
        return Err(AppError::Custom {
            message: "invalid WSL path metadata response".to_string(),
        });
    }
    let mut metadata = BTreeMap::new();
    let mut index = 1;
    while index < records.len() {
        if records.get(index).map(String::as_str) != Some("path") || index + 4 >= records.len() {
            return Err(AppError::Custom {
                message: "invalid WSL path metadata record".to_string(),
            });
        }
        let path = records[index + 1].clone();
        let entry_kind = match records[index + 2].as_str() {
            "missing" => PathEntryKind::Missing,
            "directory" => PathEntryKind::Directory,
            "symlink-directory" => PathEntryKind::SymlinkDirectory,
            "symlink-other" => PathEntryKind::SymlinkOther,
            "other" => PathEntryKind::Other,
            "broken-link" => PathEntryKind::BrokenLink,
            "inaccessible" => PathEntryKind::Inaccessible,
            _ => {
                return Err(AppError::Custom {
                    message: "invalid WSL path metadata kind".to_string(),
                })
            }
        };
        let eve_package = match records[index + 3].as_str() {
            "none" | "eve-unreadable" => None,
            "eve-empty" => Some(false),
            "eve" => Some(is_eve_package(&records[index + 4])),
            _ => {
                return Err(AppError::Custom {
                    message: "invalid WSL path metadata payload".to_string(),
                })
            }
        };
        metadata.insert(
            path,
            PathMetadata {
                entry_kind,
                eve_package,
            },
        );
        index += 5;
    }
    Ok(metadata)
}

fn is_eve_package(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .is_some_and(|package| {
            ["dependencies", "devDependencies"]
                .into_iter()
                .any(|section| {
                    package
                        .get(section)
                        .and_then(serde_json::Value::as_object)
                        .is_some_and(|entries| entries.contains_key("eve"))
                })
        })
}

fn path_presence(
    resolution: &PathResolution,
    metadata: &BTreeMap<String, PathMetadata>,
    environment_available: bool,
    context: &EnvironmentContext,
) -> DirectoryPresenceState {
    match resolution {
        PathResolution::Indeterminate => DirectoryPresenceState::EnvironmentUnavailable,
        PathResolution::Unsafe => DirectoryPresenceState::UnsafePath,
        PathResolution::ProjectNotSelected => DirectoryPresenceState::ProjectNotSelected,
        PathResolution::Resolved(_) if !environment_available => {
            DirectoryPresenceState::EnvironmentUnavailable
        }
        PathResolution::Resolved(path) => metadata
            .get(&path_key(path, context))
            .map(|entry| match entry.entry_kind {
                PathEntryKind::Directory | PathEntryKind::SymlinkDirectory => {
                    DirectoryPresenceState::Present
                }
                PathEntryKind::Missing => DirectoryPresenceState::Missing,
                PathEntryKind::BrokenLink => DirectoryPresenceState::BrokenLink,
                PathEntryKind::Other | PathEntryKind::SymlinkOther => {
                    DirectoryPresenceState::ConflictingEntry
                }
                PathEntryKind::Inaccessible => DirectoryPresenceState::EnvironmentUnavailable,
            })
            .unwrap_or(DirectoryPresenceState::EnvironmentUnavailable),
    }
}

fn join_resolved(base: &str, child: &str, context: &EnvironmentContext) -> String {
    if child.trim_matches(['/', '\\']).is_empty() {
        normalize_path(base, context)
    } else {
        normalize_path(&join_posix(base, child), context)
    }
}

fn normalize_path(path: &str, _context: &EnvironmentContext) -> String {
    let normalized = path.replace('\\', "/");
    if normalized == "/" || is_windows_drive_root(&normalized) {
        return normalized;
    }
    let trimmed = normalized.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn path_key(path: &str, context: &EnvironmentContext) -> String {
    let normalized = normalize_path(path, context);
    if matches!(context.environment, EnvironmentRef::Host) && cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

fn absolute_path_is_compatible(path: &str, context: &EnvironmentContext) -> bool {
    match context.environment {
        EnvironmentRef::Wsl { .. } => is_posix_absolute(path),
        EnvironmentRef::Host if cfg!(windows) => is_windows_absolute(path),
        EnvironmentRef::Host => is_posix_absolute(path),
    }
}

fn is_posix_absolute(path: &str) -> bool {
    path.starts_with('/') && !path.starts_with("//")
}

fn is_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || path.starts_with("//")
        || path.starts_with("\\\\")
}

fn is_windows_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OfficialSharedSupport {
    Yes,
    No,
    Unknown,
}

fn global_official_support(agent: AgentType) -> OfficialSharedSupport {
    match agent {
        AgentType::Codex
        | AgentType::GithubCopilot
        | AgentType::GeminiCli
        | AgentType::Opencode
        | AgentType::Warp
        | AgentType::Zed
        | AgentType::Firebender
        | AgentType::KimiCodeCli => OfficialSharedSupport::Yes,
        AgentType::Amp | AgentType::Antigravity | AgentType::Cline | AgentType::Deepagents => {
            OfficialSharedSupport::No
        }
        _ => OfficialSharedSupport::Unknown,
    }
}

fn join_posix(base: &str, child: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        child.trim_matches(['/', '\\']).replace('\\', "/")
    )
}

fn path_to_posix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn same_posix_path(left: &str, right: &str) -> bool {
    left.trim_end_matches('/') == right.trim_end_matches('/')
}

fn parent_posix(path: &str) -> Option<String> {
    path.trim_end_matches('/')
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Arc, Mutex};

    use super::{
        parse_wsl_path_metadata, AgentEnvironmentContext, AgentEnvironmentResolver,
        DirectoryPresenceState, EnvironmentContext, PathEntryKind, PathMetadata,
        WSL_PATH_METADATA_SCRIPT,
    };
    use crate::core::agent_availability::AgentAvailabilityKind;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, LegacyMigrationTarget,
        LegacyPath, LegacyPathBehavior, LegacyPathScope, PathSpec, ScopeDefinition,
    };
    use crate::core::agent_registry::AgentRegistrySnapshot;
    use crate::core::agents::AgentType;
    use crate::environment::types::{EnvironmentRef, EnvironmentStatus};

    fn linux_context() -> AgentEnvironmentContext {
        AgentEnvironmentContext {
            home: "/home/alice".to_string(),
            config_home: "/home/alice/.config".to_string(),
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn maps_home_and_config_based_global_paths_into_linux_environment() {
        let resolver = AgentEnvironmentResolver::new(linux_context());

        assert_eq!(
            resolver.global_skills_dir(AgentType::AiderDesk).as_deref(),
            Some("/home/alice/.aider-desk/skills")
        );
        assert_eq!(
            resolver.global_skills_dir(AgentType::Amp).as_deref(),
            Some("/home/alice/.config/agents/skills")
        );
    }

    #[test]
    fn honors_environment_specific_codex_and_claude_homes() {
        let mut context = linux_context();
        context
            .env
            .insert("CODEX_HOME".to_string(), "/opt/codex-profile".to_string());
        context.env.insert(
            "CLAUDE_CONFIG_DIR".to_string(),
            "/opt/claude-profile".to_string(),
        );
        let resolver = AgentEnvironmentResolver::new(context);

        assert_eq!(
            resolver.global_skills_dir(AgentType::Codex).as_deref(),
            Some("/opt/codex-profile/skills")
        );
        assert_eq!(
            resolver.global_skills_dir(AgentType::ClaudeCode).as_deref(),
            Some("/opt/claude-profile/skills")
        );
    }

    #[test]
    fn resolves_project_path_without_using_host_home() {
        let resolver = AgentEnvironmentResolver::new(linux_context());

        assert_eq!(
            resolver.project_skills_dir(AgentType::Codex, "/work/app"),
            "/work/app/.agents/skills"
        );
    }

    #[test]
    fn resolves_environment_specific_agent_targets_without_host_detection() {
        let resolver = AgentEnvironmentResolver::new(linux_context());

        let codex = resolver.target(AgentType::Codex, false, "/work/app");
        assert_eq!(codex.shared_path, "/work/app/.agents/skills");
        assert_eq!(codex.private_path, None);
        assert_eq!(codex.availability, AgentAvailabilityKind::SharedOnly);
        assert!(codex.default_available);
        assert_eq!(codex.detection_paths[0], "/home/alice/.codex");
        assert!(codex.detection_paths.contains(&"/etc/codex".to_string()));

        let claude = resolver.target(AgentType::ClaudeCode, false, "/work/app");
        assert_eq!(
            claude.private_path.as_deref(),
            Some("/work/app/.claude/skills")
        );
        assert_eq!(claude.availability, AgentAvailabilityKind::PrivateRequired);
        assert!(!claude.default_available);

        let amp = resolver.target(AgentType::Amp, true, "/work/app");
        assert_eq!(
            amp.private_path.as_deref(),
            Some("/home/alice/.config/agents/skills")
        );
        assert_eq!(amp.detection_paths, vec!["/home/alice/.config/amp"]);

        let target = codex.scope_target(false);
        assert!(target.supported);
        assert!(target.automatic);
        assert_eq!(target.path, ".agents/skills");
        assert_eq!(target.install_path, "/work/app/.agents/skills");
        assert_eq!(target.read_paths, vec!["/work/app/.agents/skills"]);
    }

    fn scope(enabled: bool, reads_shared: bool, private_path: Option<PathSpec>) -> ScopeDefinition {
        ScopeDefinition {
            enabled,
            reads_shared,
            private_path,
        }
    }

    fn definition(
        id: &str,
        global: ScopeDefinition,
        project: ScopeDefinition,
        detection_paths: Vec<PathSpec>,
    ) -> AgentDefinition {
        AgentDefinition {
            id: AgentId::parse(id).expect("agent ID"),
            display_name: id.to_string(),
            source: AgentSource::Custom,
            aliases: Vec::new(),
            global,
            project,
            detection: DetectionSpec::AnyPathExists {
                paths: detection_paths,
            },
            legacy_paths: Vec::new(),
            adapter: AgentAdapter::Standard,
        }
    }

    fn registry(revision: &str, definitions: Vec<AgentDefinition>) -> AgentRegistrySnapshot {
        AgentRegistrySnapshot {
            revision: revision.to_string(),
            active_definitions: definitions
                .into_iter()
                .map(|definition| (definition.id.clone(), definition))
                .collect(),
        }
    }

    fn runtime_context(
        environment: EnvironmentRef,
        availability: EnvironmentStatus,
        revision: &str,
    ) -> EnvironmentContext {
        EnvironmentContext {
            environment,
            home: "/home/alice".to_string(),
            config_home: "/home/alice/.config".to_string(),
            environment_variables: BTreeMap::new(),
            availability,
            revision: revision.to_string(),
            wsl_session: None,
        }
    }

    fn resolver_with_present_paths(
        context: EnvironmentContext,
        present_paths: impl IntoIterator<Item = &'static str>,
    ) -> (AgentEnvironmentResolver, Arc<Mutex<Vec<Vec<String>>>>) {
        let present_paths: BTreeSet<String> =
            present_paths.into_iter().map(ToString::to_string).collect();
        let calls = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
        let recorded_calls = Arc::clone(&calls);
        let resolver = AgentEnvironmentResolver::with_metadata_query(context, move |queries| {
            recorded_calls
                .lock()
                .expect("calls lock")
                .push(queries.iter().map(|query| query.path.clone()).collect());
            Ok(queries
                .iter()
                .map(|query| {
                    let entry_kind = if present_paths.contains(&query.path) {
                        PathEntryKind::Directory
                    } else {
                        PathEntryKind::Missing
                    };
                    (
                        query.path.clone(),
                        PathMetadata {
                            entry_kind,
                            eve_package: None,
                        },
                    )
                })
                .collect())
        });
        (resolver, calls)
    }

    async fn resolve_detection_with_metadata(
        detection_paths: Vec<PathSpec>,
        entry_kinds: impl IntoIterator<Item = (&'static str, PathEntryKind)>,
    ) -> super::DetectionState {
        let snapshot = registry(
            "registry-1",
            vec![definition(
                "detection-agent",
                scope(false, false, None),
                scope(false, false, None),
                detection_paths,
            )],
        );
        let entry_kinds: BTreeMap<String, PathEntryKind> = entry_kinds
            .into_iter()
            .map(|(path, entry_kind)| (path.to_string(), entry_kind))
            .collect();
        let resolver = AgentEnvironmentResolver::with_metadata_query(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            move |queries| {
                Ok(queries
                    .iter()
                    .map(|query| {
                        (
                            query.path.clone(),
                            PathMetadata {
                                entry_kind: entry_kinds
                                    .get(&query.path)
                                    .copied()
                                    .unwrap_or(PathEntryKind::Missing),
                                eve_package: None,
                            },
                        )
                    })
                    .collect())
            },
        );

        resolver
            .resolve_registry(&snapshot, None)
            .await
            .unwrap()
            .agents[&AgentId::parse("detection-agent").unwrap()]
            .detection
    }

    #[tokio::test]
    async fn resolves_scope_locations_independently_for_each_definition() {
        let definitions = vec![
            definition(
                "global-shared-project-private",
                scope(true, true, None),
                scope(true, false, Some(PathSpec::project(".private/skills"))),
                vec![PathSpec::home(".one")],
            ),
            definition(
                "global-private-project-both",
                scope(true, false, Some(PathSpec::home(".two/skills"))),
                scope(true, true, Some(PathSpec::project(".two/skills"))),
                vec![PathSpec::home(".two")],
            ),
            definition(
                "global-both-project-shared",
                scope(true, true, Some(PathSpec::home(".three/skills"))),
                scope(true, true, None),
                vec![PathSpec::home(".three")],
            ),
        ];
        let snapshot = registry("registry-1", definitions);
        let (resolver, _) = resolver_with_present_paths(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            [],
        );

        let resolved = resolver
            .resolve_registry(&snapshot, Some("/work/app"))
            .await
            .expect("resolve registry");

        let first = resolved
            .agents
            .get(&AgentId::parse("global-shared-project-private").unwrap())
            .unwrap();
        assert_eq!(first.global.read_paths, vec!["/home/alice/.agents/skills"]);
        assert_eq!(first.project.read_paths, vec!["/work/app/.private/skills"]);

        let second = resolved
            .agents
            .get(&AgentId::parse("global-private-project-both").unwrap())
            .unwrap();
        assert_eq!(second.global.read_paths, vec!["/home/alice/.two/skills"]);
        assert_eq!(
            second.project.read_paths,
            vec!["/work/app/.agents/skills", "/work/app/.two/skills"]
        );

        let third = resolved
            .agents
            .get(&AgentId::parse("global-both-project-shared").unwrap())
            .unwrap();
        assert_eq!(
            third.global.read_paths,
            vec!["/home/alice/.agents/skills", "/home/alice/.three/skills"]
        );
        assert_eq!(third.project.read_paths, vec!["/work/app/.agents/skills"]);
    }

    #[tokio::test]
    async fn disabled_scope_is_not_resolved_or_queried() {
        let snapshot = registry(
            "registry-1",
            vec![definition(
                "disabled-global",
                scope(false, true, Some(PathSpec::home("disabled/skills"))),
                scope(true, true, None),
                vec![PathSpec::home(".detected")],
            )],
        );
        let (resolver, calls) = resolver_with_present_paths(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            [],
        );

        let resolved = resolver
            .resolve_registry(&snapshot, Some("/work/app"))
            .await
            .expect("resolve registry");
        let agent = resolved.agents.values().next().unwrap();

        assert!(!agent.global.enabled);
        assert!(agent.global.shared_path.is_none());
        assert!(agent.global.private_path.is_none());
        assert!(agent.global.read_paths.is_empty());
        assert!(!calls.lock().unwrap()[0]
            .iter()
            .any(|path| path.contains("disabled")));
    }

    #[tokio::test]
    async fn any_path_exists_is_shared_by_builtin_and_custom_definitions() {
        let snapshot = registry(
            "registry-1",
            vec![
                definition(
                    "detected",
                    scope(true, true, None),
                    scope(false, false, None),
                    vec![PathSpec::home(".missing"), PathSpec::home(".present")],
                ),
                definition(
                    "not-detected",
                    scope(true, true, None),
                    scope(false, false, None),
                    vec![PathSpec::home(".absent")],
                ),
            ],
        );
        let (resolver, _) = resolver_with_present_paths(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            ["/home/alice/.present"],
        );

        let resolved = resolver
            .resolve_registry(&snapshot, None)
            .await
            .expect("resolve registry");

        assert_eq!(
            resolved.agents[&AgentId::parse("detected").unwrap()].detection,
            super::DetectionState::Detected
        );
        assert_eq!(
            resolved.agents[&AgentId::parse("not-detected").unwrap()].detection,
            super::DetectionState::NotDetected
        );
    }

    #[tokio::test]
    async fn any_path_exists_is_indeterminate_for_inaccessible_and_missing_paths() {
        let detection = resolve_detection_with_metadata(
            vec![PathSpec::home(".inaccessible"), PathSpec::home(".missing")],
            [("/home/alice/.inaccessible", PathEntryKind::Inaccessible)],
        )
        .await;

        assert_eq!(detection, super::DetectionState::Indeterminate);
    }

    #[tokio::test]
    async fn any_path_exists_is_indeterminate_for_fail_closed_first_existing_and_missing_path() {
        let detection = resolve_detection_with_metadata(
            vec![
                PathSpec::FirstExisting {
                    candidates: vec![PathSpec::home(".inaccessible-candidate")],
                    fallback: Box::new(PathSpec::home(".present-fallback")),
                },
                PathSpec::home(".missing"),
            ],
            [
                (
                    "/home/alice/.inaccessible-candidate",
                    PathEntryKind::Inaccessible,
                ),
                ("/home/alice/.present-fallback", PathEntryKind::Directory),
            ],
        )
        .await;

        assert_eq!(detection, super::DetectionState::Indeterminate);
    }

    #[tokio::test]
    async fn any_path_exists_is_not_detected_when_every_path_is_missing() {
        let detection = resolve_detection_with_metadata(
            vec![
                PathSpec::home(".missing-one"),
                PathSpec::home(".missing-two"),
            ],
            [],
        )
        .await;

        assert_eq!(detection, super::DetectionState::NotDetected);
    }

    #[tokio::test]
    async fn any_path_exists_is_detected_for_inaccessible_and_present_paths() {
        let detection = resolve_detection_with_metadata(
            vec![PathSpec::home(".inaccessible"), PathSpec::home(".present")],
            [
                ("/home/alice/.inaccessible", PathEntryKind::Inaccessible),
                ("/home/alice/.present", PathEntryKind::Directory),
            ],
        )
        .await;

        assert_eq!(detection, super::DetectionState::Detected);
    }

    #[tokio::test]
    async fn matching_absolute_paths_resolve_for_wsl_detection_and_global_private_scope() {
        let snapshot = registry(
            "registry-1",
            vec![definition(
                "absolute-agent",
                scope(
                    true,
                    false,
                    Some(PathSpec::absolute("/opt/absolute-agent/skills")),
                ),
                scope(false, false, None),
                vec![PathSpec::absolute("/opt/absolute-agent")],
            )],
        );
        let (resolver, _) = resolver_with_present_paths(
            runtime_context(
                EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                EnvironmentStatus::Available,
                "environment-1",
            ),
            ["/opt/absolute-agent", "/opt/absolute-agent/skills"],
        );

        let resolved = resolver
            .resolve_registry(&snapshot, None)
            .await
            .expect("resolve registry");
        let agent = resolved.agents.values().next().unwrap();

        assert_eq!(agent.detection, super::DetectionState::Detected);
        assert_eq!(
            agent.global.private_path.as_deref(),
            Some("/opt/absolute-agent/skills")
        );
        assert_eq!(
            agent.global.private_presence,
            Some(DirectoryPresenceState::Present)
        );
    }

    #[tokio::test]
    async fn incompatible_absolute_paths_fail_closed_per_environment() {
        let snapshot = registry(
            "registry-1",
            vec![
                definition(
                    "windows-only-agent",
                    scope(
                        true,
                        false,
                        Some(PathSpec::absolute("C:/Users/alice/.agent/skills")),
                    ),
                    scope(false, false, None),
                    vec![PathSpec::absolute("C:/Users/alice/.agent")],
                ),
                definition(
                    "unc-only-agent",
                    scope(
                        true,
                        false,
                        Some(PathSpec::absolute("//server/share/agent/skills")),
                    ),
                    scope(false, false, None),
                    vec![PathSpec::absolute("//server/share/agent")],
                ),
            ],
        );
        let (resolver, calls) = resolver_with_present_paths(
            runtime_context(
                EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                EnvironmentStatus::Available,
                "environment-1",
            ),
            [],
        );

        let resolved = resolver
            .resolve_registry(&snapshot, None)
            .await
            .expect("resolve registry");
        let agent = &resolved.agents[&AgentId::parse("windows-only-agent").unwrap()];
        let unc_agent = &resolved.agents[&AgentId::parse("unc-only-agent").unwrap()];

        assert_eq!(agent.detection, super::DetectionState::Indeterminate);
        assert!(agent.global.private_path.is_none());
        assert_eq!(
            agent.global.private_presence,
            Some(DirectoryPresenceState::UnsafePath)
        );
        assert!(!calls.lock().unwrap()[0]
            .iter()
            .any(|path| path.starts_with("C:")));
        assert_eq!(unc_agent.detection, super::DetectionState::Indeterminate);
        assert!(unc_agent.global.private_path.is_none());
        assert!(!calls.lock().unwrap()[0]
            .iter()
            .any(|path| path.starts_with("//server")));
    }

    #[tokio::test]
    async fn unavailable_environment_returns_indeterminate_states_without_querying() {
        let snapshot = registry(
            "registry-1",
            vec![definition(
                "offline-agent",
                scope(true, true, Some(PathSpec::home(".offline/skills"))),
                scope(true, true, None),
                vec![PathSpec::home(".offline")],
            )],
        );
        let (resolver, calls) = resolver_with_present_paths(
            runtime_context(
                EnvironmentRef::Wsl {
                    distro_name: "Offline".to_string(),
                },
                EnvironmentStatus::Unavailable,
                "environment-1",
            ),
            [],
        );

        let resolved = resolver
            .resolve_registry(&snapshot, Some("/work/app"))
            .await
            .expect("resolve unavailable registry");
        let agent = resolved.agents.values().next().unwrap();

        assert_eq!(agent.detection, super::DetectionState::Indeterminate);
        assert_eq!(
            agent.global.shared_presence,
            Some(DirectoryPresenceState::EnvironmentUnavailable)
        );
        assert_eq!(
            agent.project.shared_presence,
            Some(DirectoryPresenceState::EnvironmentUnavailable)
        );
        assert!(calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn batch_query_deduplicates_paths_across_agents_and_scopes() {
        let definitions = ["one", "two"]
            .into_iter()
            .map(|id| {
                definition(
                    id,
                    scope(true, true, None),
                    scope(true, true, None),
                    vec![PathSpec::home(".shared-detection")],
                )
            })
            .collect();
        let snapshot = registry("registry-1", definitions);
        let (resolver, calls) = resolver_with_present_paths(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            [],
        );

        resolver
            .resolve_registry(&snapshot, Some("/work/app"))
            .await
            .expect("resolve registry");

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let paths = &calls[0];
        assert_eq!(paths.len(), paths.iter().collect::<BTreeSet<_>>().len());
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.as_str() == "/home/alice/.agents/skills")
                .count(),
            1
        );
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.as_str() == "/work/app/.agents/skills")
                .count(),
            1
        );
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.as_str() == "/home/alice/.shared-detection")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn cache_is_reused_until_registry_or_environment_revision_changes() {
        let definition = definition(
            "cached-agent",
            scope(true, true, None),
            scope(false, false, None),
            vec![PathSpec::home(".cached")],
        );
        let first = registry("registry-1", vec![definition.clone()]);
        let second = registry("registry-2", vec![definition.clone()]);
        let (mut resolver, calls) = resolver_with_present_paths(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            [],
        );

        resolver.resolve_registry(&first, None).await.unwrap();
        resolver.resolve_registry(&first, None).await.unwrap();
        assert_eq!(calls.lock().unwrap().len(), 1);

        resolver.resolve_registry(&second, None).await.unwrap();
        assert_eq!(calls.lock().unwrap().len(), 2);

        resolver.invalidate_cache();
        resolver.resolve_registry(&second, None).await.unwrap();
        assert_eq!(calls.lock().unwrap().len(), 3);

        resolver.replace_environment_context(runtime_context(
            EnvironmentRef::Host,
            EnvironmentStatus::Available,
            "environment-2",
        ));
        resolver.resolve_registry(&second, None).await.unwrap();
        assert_eq!(calls.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn first_existing_uses_the_first_present_candidate_and_stable_fallback() {
        let snapshot = registry(
            "registry-1",
            vec![
                definition(
                    "candidate-agent",
                    scope(
                        true,
                        false,
                        Some(PathSpec::FirstExisting {
                            candidates: vec![
                                PathSpec::home(".old-one/skills"),
                                PathSpec::home(".old-two/skills"),
                            ],
                            fallback: Box::new(PathSpec::home(".new/skills")),
                        }),
                    ),
                    scope(false, false, None),
                    vec![PathSpec::home(".candidate")],
                ),
                definition(
                    "fallback-agent",
                    scope(
                        true,
                        false,
                        Some(PathSpec::FirstExisting {
                            candidates: vec![PathSpec::home(".missing/skills")],
                            fallback: Box::new(PathSpec::home(".fallback/skills")),
                        }),
                    ),
                    scope(false, false, None),
                    vec![PathSpec::home(".fallback-agent")],
                ),
                definition(
                    "environment-fallback-agent",
                    scope(
                        true,
                        false,
                        Some(PathSpec::EnvironmentVariable {
                            name: "MISSING_AGENT_HOME".to_string(),
                            relative_path: "skills".to_string(),
                            fallback: Box::new(PathSpec::FirstExisting {
                                candidates: vec![PathSpec::home(".env-old/skills")],
                                fallback: Box::new(PathSpec::home(".env-new/skills")),
                            }),
                        }),
                    ),
                    scope(false, false, None),
                    vec![PathSpec::home(".environment-fallback-agent")],
                ),
            ],
        );
        let (resolver, _) = resolver_with_present_paths(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            ["/home/alice/.old-two/skills", "/home/alice/.env-old/skills"],
        );

        let resolved = resolver.resolve_registry(&snapshot, None).await.unwrap();

        assert_eq!(
            resolved.agents[&AgentId::parse("candidate-agent").unwrap()]
                .global
                .private_path
                .as_deref(),
            Some("/home/alice/.old-two/skills")
        );
        assert_eq!(
            resolved.agents[&AgentId::parse("fallback-agent").unwrap()]
                .global
                .private_path
                .as_deref(),
            Some("/home/alice/.fallback/skills")
        );
        assert_eq!(
            resolved.agents[&AgentId::parse("environment-fallback-agent").unwrap()]
                .global
                .private_path
                .as_deref(),
            Some("/home/alice/.env-old/skills")
        );
    }

    #[tokio::test]
    async fn reports_present_legacy_paths_without_using_them_as_current_read_paths() {
        let mut legacy_agent = definition(
            "legacy-agent",
            scope(true, true, None),
            scope(false, false, None),
            vec![PathSpec::home(".legacy-agent")],
        );
        legacy_agent.legacy_paths.push(LegacyPath {
            scope: LegacyPathScope::Global,
            path: PathSpec::home(".legacy-agent/old-skills"),
            behavior: LegacyPathBehavior::OfferMigration,
            migration_target: LegacyMigrationTarget::SharedCanonical,
        });
        let snapshot = registry("registry-1", vec![legacy_agent]);
        let (resolver, _) = resolver_with_present_paths(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            ["/home/alice/.legacy-agent/old-skills"],
        );

        let resolved = resolver.resolve_registry(&snapshot, None).await.unwrap();
        let global = &resolved.agents[&AgentId::parse("legacy-agent").unwrap()].global;

        assert_eq!(global.read_paths, vec!["/home/alice/.agents/skills"]);
        assert_eq!(global.legacy_paths.len(), 1);
        assert_eq!(
            global.legacy_paths[0].path.as_deref(),
            Some("/home/alice/.legacy-agent/old-skills")
        );
        assert_eq!(
            global.legacy_paths[0].presence,
            DirectoryPresenceState::LegacyPath
        );
    }

    #[tokio::test]
    async fn first_existing_skips_a_conflicting_file_candidate() {
        let snapshot = registry(
            "registry-1",
            vec![definition(
                "first-directory-agent",
                scope(
                    true,
                    false,
                    Some(PathSpec::FirstExisting {
                        candidates: vec![
                            PathSpec::home(".file-candidate"),
                            PathSpec::home(".directory-candidate"),
                        ],
                        fallback: Box::new(PathSpec::home(".fallback-candidate")),
                    }),
                ),
                scope(false, false, None),
                vec![PathSpec::home(".first-directory-agent")],
            )],
        );
        let resolver = AgentEnvironmentResolver::with_metadata_query(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            |queries| {
                Ok(queries
                    .iter()
                    .map(|query| {
                        let entry_kind = match query.path.as_str() {
                            "/home/alice/.file-candidate" => PathEntryKind::Other,
                            "/home/alice/.directory-candidate" => PathEntryKind::Directory,
                            _ => PathEntryKind::Missing,
                        };
                        (
                            query.path.clone(),
                            PathMetadata {
                                entry_kind,
                                eve_package: None,
                            },
                        )
                    })
                    .collect())
            },
        );

        let resolved = resolver.resolve_registry(&snapshot, None).await.unwrap();

        assert_eq!(
            resolved.agents[&AgentId::parse("first-directory-agent").unwrap()]
                .global
                .private_path
                .as_deref(),
            Some("/home/alice/.directory-candidate")
        );
    }

    #[tokio::test]
    async fn first_existing_stops_on_inaccessible_or_unknown_candidates() {
        let snapshot = registry(
            "registry-1",
            vec![
                definition(
                    "inaccessible-candidate-agent",
                    scope(
                        true,
                        false,
                        Some(PathSpec::FirstExisting {
                            candidates: vec![
                                PathSpec::home(".inaccessible-candidate"),
                                PathSpec::home(".later-directory"),
                            ],
                            fallback: Box::new(PathSpec::home(".fallback-directory")),
                        }),
                    ),
                    scope(false, false, None),
                    vec![PathSpec::home(".inaccessible-agent")],
                ),
                definition(
                    "unknown-candidate-agent",
                    scope(
                        true,
                        false,
                        Some(PathSpec::FirstExisting {
                            candidates: vec![
                                PathSpec::home(".unknown-candidate"),
                                PathSpec::home(".later-directory"),
                            ],
                            fallback: Box::new(PathSpec::home(".fallback-directory")),
                        }),
                    ),
                    scope(false, false, None),
                    vec![PathSpec::home(".unknown-agent")],
                ),
            ],
        );
        let resolver = AgentEnvironmentResolver::with_metadata_query(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            |queries| {
                Ok(queries
                    .iter()
                    .filter_map(|query| {
                        if query.path == "/home/alice/.unknown-candidate" {
                            return None;
                        }
                        let entry_kind = match query.path.as_str() {
                            "/home/alice/.inaccessible-candidate" => PathEntryKind::Inaccessible,
                            "/home/alice/.later-directory" | "/home/alice/.fallback-directory" => {
                                PathEntryKind::Directory
                            }
                            _ => PathEntryKind::Missing,
                        };
                        Some((
                            query.path.clone(),
                            PathMetadata {
                                entry_kind,
                                eve_package: None,
                            },
                        ))
                    })
                    .collect())
            },
        );

        let resolved = resolver.resolve_registry(&snapshot, None).await.unwrap();
        let inaccessible =
            &resolved.agents[&AgentId::parse("inaccessible-candidate-agent").unwrap()].global;
        let unknown = &resolved.agents[&AgentId::parse("unknown-candidate-agent").unwrap()].global;

        assert!(inaccessible.private_path.is_none());
        assert!(inaccessible.read_paths.is_empty());
        assert_eq!(
            inaccessible.private_presence,
            Some(DirectoryPresenceState::EnvironmentUnavailable)
        );
        assert!(unknown.private_path.is_none());
        assert!(unknown.read_paths.is_empty());
        assert_eq!(
            unknown.private_presence,
            Some(DirectoryPresenceState::EnvironmentUnavailable)
        );
    }

    #[tokio::test]
    async fn environment_unavailable_during_batch_becomes_indeterminate_snapshot() {
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let snapshot = registry(
            "registry-1",
            vec![definition(
                "disconnecting-agent",
                scope(true, true, None),
                scope(false, false, None),
                vec![PathSpec::home(".disconnecting")],
            )],
        );
        let resolver = AgentEnvironmentResolver::with_metadata_query(
            runtime_context(
                environment.clone(),
                EnvironmentStatus::Available,
                "environment-1",
            ),
            move |_| {
                Err(crate::error::AppError::EnvironmentUnavailable {
                    environment: environment.clone(),
                    message: "disconnected".to_string(),
                })
            },
        );

        let resolved = resolver.resolve_registry(&snapshot, None).await.unwrap();
        let agent = &resolved.agents[&AgentId::parse("disconnecting-agent").unwrap()];

        assert_eq!(resolved.availability, EnvironmentStatus::Unavailable);
        assert_eq!(agent.detection, super::DetectionState::Indeterminate);
        assert_eq!(
            agent.global.shared_presence,
            Some(DirectoryPresenceState::EnvironmentUnavailable)
        );
    }

    #[tokio::test]
    async fn eve_adapter_reuses_the_batch_for_project_package_detection() {
        let mut eve = definition(
            "eve",
            scope(false, false, None),
            scope(true, false, Some(PathSpec::project("agent/skills"))),
            vec![PathSpec::home("unused")],
        );
        eve.source = AgentSource::Builtin;
        eve.detection = DetectionSpec::Eve;
        eve.adapter = AgentAdapter::Eve;
        let snapshot = registry("registry-1", vec![eve]);
        let calls = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
        let recorded_calls = Arc::clone(&calls);
        let resolver = AgentEnvironmentResolver::with_metadata_query(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            move |queries| {
                recorded_calls
                    .lock()
                    .unwrap()
                    .push(queries.iter().map(|query| query.path.clone()).collect());
                Ok(queries
                    .iter()
                    .map(|query| {
                        let metadata = if query.path == "/work/app/package.json" {
                            PathMetadata {
                                entry_kind: PathEntryKind::Other,
                                eve_package: Some(true),
                            }
                        } else if query.path == "/work/app/agent" {
                            PathMetadata {
                                entry_kind: PathEntryKind::Directory,
                                eve_package: None,
                            }
                        } else {
                            PathMetadata {
                                entry_kind: PathEntryKind::Missing,
                                eve_package: None,
                            }
                        };
                        (query.path.clone(), metadata)
                    })
                    .collect())
            },
        );

        let resolved = resolver
            .resolve_registry(&snapshot, Some("/work/app"))
            .await
            .unwrap();

        assert_eq!(
            resolved.agents[&AgentId::parse("eve").unwrap()].detection,
            super::DetectionState::Detected
        );
        assert_eq!(calls.lock().unwrap().len(), 1);
        assert!(calls.lock().unwrap()[0]
            .iter()
            .any(|path| path == "/work/app/package.json"));
    }

    #[tokio::test]
    async fn eve_requires_agent_directory_or_directory_symlink() {
        let mut eve = definition(
            "eve",
            scope(false, false, None),
            scope(true, false, Some(PathSpec::project("agent/skills"))),
            vec![PathSpec::home("unused")],
        );
        eve.source = AgentSource::Builtin;
        eve.detection = DetectionSpec::Eve;
        eve.adapter = AgentAdapter::Eve;
        let snapshot = registry("registry-1", vec![eve]);
        let resolver = AgentEnvironmentResolver::with_metadata_query(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            |queries| {
                Ok(queries
                    .iter()
                    .map(|query| {
                        let metadata = if query.path.ends_with("/package.json") {
                            PathMetadata {
                                entry_kind: PathEntryKind::Other,
                                eve_package: Some(true),
                            }
                        } else if query.path == "/work/file/agent" {
                            PathMetadata {
                                entry_kind: PathEntryKind::Other,
                                eve_package: None,
                            }
                        } else if query.path == "/work/file-link/agent" {
                            PathMetadata {
                                entry_kind: PathEntryKind::SymlinkOther,
                                eve_package: None,
                            }
                        } else if query.path == "/work/directory-link/agent" {
                            PathMetadata {
                                entry_kind: PathEntryKind::SymlinkDirectory,
                                eve_package: None,
                            }
                        } else {
                            PathMetadata {
                                entry_kind: PathEntryKind::Missing,
                                eve_package: None,
                            }
                        };
                        (query.path.clone(), metadata)
                    })
                    .collect())
            },
        );

        let file = resolver
            .resolve_registry(&snapshot, Some("/work/file"))
            .await
            .unwrap();
        let file_link = resolver
            .resolve_registry(&snapshot, Some("/work/file-link"))
            .await
            .unwrap();
        let directory_link = resolver
            .resolve_registry(&snapshot, Some("/work/directory-link"))
            .await
            .unwrap();

        assert_eq!(
            file.agents[&AgentId::parse("eve").unwrap()].detection,
            super::DetectionState::NotDetected
        );
        assert_eq!(
            file_link.agents[&AgentId::parse("eve").unwrap()].detection,
            super::DetectionState::NotDetected
        );
        assert_eq!(
            directory_link.agents[&AgentId::parse("eve").unwrap()].detection,
            super::DetectionState::Detected
        );
    }

    #[tokio::test]
    async fn eve_package_inspection_distinguishes_empty_from_unreadable() {
        let mut eve = definition(
            "eve",
            scope(false, false, None),
            scope(true, false, Some(PathSpec::project("agent/skills"))),
            vec![PathSpec::home("unused")],
        );
        eve.source = AgentSource::Builtin;
        eve.detection = DetectionSpec::Eve;
        eve.adapter = AgentAdapter::Eve;
        let other = definition(
            "other-agent",
            scope(true, true, None),
            scope(false, false, None),
            vec![PathSpec::home(".other-agent")],
        );
        let snapshot = registry("registry-1", vec![eve, other]);
        let resolver = AgentEnvironmentResolver::with_metadata_query(
            runtime_context(
                EnvironmentRef::Host,
                EnvironmentStatus::Available,
                "environment-1",
            ),
            |queries| {
                Ok(queries
                    .iter()
                    .map(|query| {
                        let metadata = if query.path.ends_with("/agent")
                            || query.path == "/home/alice/.other-agent"
                        {
                            PathMetadata {
                                entry_kind: PathEntryKind::Directory,
                                eve_package: None,
                            }
                        } else if query.path == "/work/empty/package.json" {
                            PathMetadata {
                                entry_kind: PathEntryKind::Other,
                                eve_package: Some(false),
                            }
                        } else if query.path == "/work/unreadable/package.json" {
                            PathMetadata {
                                entry_kind: PathEntryKind::Other,
                                eve_package: None,
                            }
                        } else {
                            PathMetadata {
                                entry_kind: PathEntryKind::Missing,
                                eve_package: None,
                            }
                        };
                        (query.path.clone(), metadata)
                    })
                    .collect())
            },
        );

        let empty = resolver
            .resolve_registry(&snapshot, Some("/work/empty"))
            .await
            .unwrap();
        let unreadable = resolver
            .resolve_registry(&snapshot, Some("/work/unreadable"))
            .await
            .unwrap();

        assert_eq!(
            empty.agents[&AgentId::parse("eve").unwrap()].detection,
            super::DetectionState::NotDetected
        );
        assert_eq!(
            unreadable.agents[&AgentId::parse("eve").unwrap()].detection,
            super::DetectionState::Indeterminate
        );
        assert_eq!(
            empty.agents[&AgentId::parse("other-agent").unwrap()].detection,
            super::DetectionState::Detected
        );
        assert_eq!(
            unreadable.agents[&AgentId::parse("other-agent").unwrap()].detection,
            super::DetectionState::Detected
        );
    }

    #[cfg(unix)]
    #[test]
    fn wsl_metadata_script_keeps_empty_eve_payload_aligned_with_later_agent() {
        let temp = tempfile::tempdir().unwrap();
        let package_path = temp.path().join("package.json");
        let other_agent_path = temp.path().join("other-agent");
        std::fs::write(&package_path, []).unwrap();
        std::fs::create_dir(&other_agent_path).unwrap();
        let package_path = package_path.to_string_lossy().into_owned();
        let other_agent_path = other_agent_path.to_string_lossy().into_owned();
        let output = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                WSL_PATH_METADATA_SCRIPT,
                "--",
                package_path.as_str(),
                "1",
                other_agent_path.as_str(),
                "0",
            ])
            .output()
            .unwrap();

        let metadata = parse_wsl_path_metadata(&output.stdout).expect("parse metadata frame");

        assert_eq!(metadata[&package_path].eve_package, Some(false));
        assert_eq!(
            metadata[&other_agent_path].entry_kind,
            PathEntryKind::Directory
        );
    }

    #[test]
    fn wsl_metadata_parser_keeps_unreadable_eve_and_inaccessible_path_records_distinct() {
        let bytes = b"1\0path\0/work/package.json\0other\0eve-unreadable\0-\0path\0/home/alice/.other-agent\0directory\0none\0-\0path\0/home/alice/.blocked\0inaccessible\0none\0-\0";

        let metadata = parse_wsl_path_metadata(bytes).expect("parse metadata frame");

        assert_eq!(metadata["/work/package.json"].eve_package, None);
        assert_eq!(
            metadata["/home/alice/.other-agent"].entry_kind,
            PathEntryKind::Directory
        );
        assert_eq!(
            metadata["/home/alice/.blocked"].entry_kind,
            PathEntryKind::Inaccessible
        );
    }

    #[cfg(unix)]
    #[test]
    fn wsl_metadata_script_preserves_the_no_payload_record() {
        let missing_path = tempfile::tempdir()
            .unwrap()
            .path()
            .join("missing")
            .to_string_lossy()
            .into_owned();
        let output = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                WSL_PATH_METADATA_SCRIPT,
                "--",
                missing_path.as_str(),
                "0",
            ])
            .output()
            .unwrap();

        let metadata = parse_wsl_path_metadata(&output.stdout).expect("parse metadata frame");

        assert_eq!(metadata[&missing_path].entry_kind, PathEntryKind::Missing);
    }
}
