use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use uuid::Uuid;

use crate::environment::types::ProjectBinding;
use crate::error::AppError;

const PROJECTS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectPathSemantics {
    WindowsHost,
    Posix,
}

impl ProjectPathSemantics {
    pub(crate) fn host() -> Self {
        if cfg!(target_os = "windows") {
            Self::WindowsHost
        } else {
            Self::Posix
        }
    }
}

struct NormalizedProjectPath {
    native_path: String,
    key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "camelCase")]
#[specta(tag = "status", rename_all = "camelCase")]
pub enum ProjectMigrationState {
    NotNeeded,
    Succeeded,
    Failed { error: AppError },
}

pub struct ProjectMigrationRegistry {
    state: Mutex<ProjectMigrationState>,
}

impl ProjectMigrationRegistry {
    pub fn new(state: ProjectMigrationState) -> Self {
        Self {
            state: Mutex::new(state),
        }
    }

    pub fn state(&self) -> ProjectMigrationState {
        self.state
            .lock()
            .expect("project migration state lock poisoned")
            .clone()
    }

    pub fn set(&self, state: ProjectMigrationState) {
        *self
            .state
            .lock()
            .expect("project migration state lock poisoned") = state;
    }

    pub fn ensure_ready(&self) -> Result<(), AppError> {
        match self.state() {
            ProjectMigrationState::Failed { error } => Err(AppError::ProjectMigrationFailed {
                message: error.to_string(),
            }),
            ProjectMigrationState::NotNeeded | ProjectMigrationState::Succeeded => Ok(()),
        }
    }
}

pub(crate) struct ProjectBindingAddResult {
    pub(crate) projects: Vec<ProjectBinding>,
    pub(crate) project: ProjectBinding,
    pub(crate) created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectsFile {
    pub(crate) schema_version: u32,
    pub(crate) projects: Vec<ProjectBinding>,
}

impl ProjectsFile {
    pub(crate) fn new(projects: Vec<ProjectBinding>, semantics: ProjectPathSemantics) -> Self {
        Self {
            schema_version: PROJECTS_SCHEMA_VERSION,
            projects: deduplicate_projects(&projects, semantics),
        }
    }
}

pub(crate) fn add_project_binding(
    mut projects: Vec<ProjectBinding>,
    native_path: String,
    semantics: ProjectPathSemantics,
) -> ProjectBindingAddResult {
    let normalized = normalize_project_path(&native_path, semantics);
    if let Some(project) = projects
        .iter()
        .find(|project| {
            normalize_project_path(&project.native_path, semantics).key == normalized.key
        })
        .cloned()
    {
        return ProjectBindingAddResult {
            projects,
            project,
            created: false,
        };
    }
    let project = ProjectBinding {
        id: Uuid::new_v4().to_string(),
        native_path: normalized.native_path,
        display_name: None,
        order: None,
        suppress_cross_storage_warning: false,
    };
    projects.push(project.clone());
    ProjectBindingAddResult {
        projects: deduplicate_projects(&projects, semantics),
        project,
        created: true,
    }
}

pub(crate) fn remove_project_binding(
    mut projects: Vec<ProjectBinding>,
    project_id: &str,
) -> Vec<ProjectBinding> {
    projects.retain(|project| project.id != project_id);
    projects
}

pub(crate) fn set_project_cross_storage_warning_suppressed(
    mut projects: Vec<ProjectBinding>,
    project_id: &str,
    suppressed: bool,
) -> Vec<ProjectBinding> {
    if let Some(project) = projects.iter_mut().find(|project| project.id == project_id) {
        project.suppress_cross_storage_warning = suppressed;
    }
    projects
}

pub struct ProjectsStore {
    path: PathBuf,
    semantics: ProjectPathSemantics,
}

impl ProjectsStore {
    pub fn new(path: PathBuf) -> Self {
        Self::new_with_semantics(path, ProjectPathSemantics::host())
    }

    pub(crate) fn new_with_semantics(path: PathBuf, semantics: ProjectPathSemantics) -> Self {
        Self { path, semantics }
    }

    pub fn read(&self) -> Result<Vec<ProjectBinding>, AppError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read(&self.path)?;
        let file: ProjectsFile = serde_json::from_slice(&content)?;
        Ok(file.projects)
    }

    pub fn write(&self, projects: &[ProjectBinding]) -> Result<(), AppError> {
        let projects = deduplicate_projects(projects, self.semantics);
        let file = ProjectsFile {
            schema_version: PROJECTS_SCHEMA_VERSION,
            projects,
        };
        atomic_write_json(&self.path, &file)
    }

    pub fn add(&self, native_path: String) -> Result<ProjectBindingAddResult, AppError> {
        let result = add_project_binding(self.read()?, native_path, self.semantics);
        if result.created {
            self.write(&result.projects)?;
        }
        Ok(result)
    }

    pub fn remove(&self, project_id: &str) -> Result<Vec<ProjectBinding>, AppError> {
        let projects = remove_project_binding(self.read()?, project_id);
        self.write(&projects)?;
        self.read()
    }

    pub fn set_cross_storage_warning_suppressed(
        &self,
        project_id: &str,
        suppressed: bool,
    ) -> Result<Vec<ProjectBinding>, AppError> {
        let projects =
            set_project_cross_storage_warning_suppressed(self.read()?, project_id, suppressed);
        self.write(&projects)?;
        self.read()
    }
}

pub fn migrate_legacy_projects(
    config_path: &Path,
    projects_path: &Path,
) -> Result<ProjectMigrationState, AppError> {
    let store = ProjectsStore::new(projects_path.to_path_buf());
    if projects_path.is_file() {
        return Ok(ProjectMigrationState::NotNeeded);
    }
    if !config_path.exists() {
        return Ok(ProjectMigrationState::NotNeeded);
    }

    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(config_path)?)?;
    let Some(root) = config.as_object_mut() else {
        return Err(AppError::Json {
            message: "config root must be a JSON object".to_string(),
        });
    };
    let legacy_paths = root
        .get("projects")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if legacy_paths.is_empty() {
        return Ok(ProjectMigrationState::NotNeeded);
    }

    let projects = legacy_paths
        .into_iter()
        .map(|native_path| ProjectBinding {
            id: Uuid::new_v4().to_string(),
            native_path: normalize_native_path(&native_path, store.semantics),
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        })
        .collect::<Vec<_>>();

    store.write(&projects)?;

    let backup_path = config_backup_path(config_path);
    fs::copy(config_path, backup_path)?;
    root.remove("projects");
    atomic_write_json(config_path, &config)?;

    Ok(ProjectMigrationState::Succeeded)
}

fn deduplicate_projects(
    projects: &[ProjectBinding],
    semantics: ProjectPathSemantics,
) -> Vec<ProjectBinding> {
    let mut seen = HashSet::new();
    projects
        .iter()
        .filter_map(|project| {
            let normalized = normalize_project_path(&project.native_path, semantics);
            if !seen.insert(normalized.key) {
                return None;
            }
            let mut project = project.clone();
            project.native_path = normalized.native_path;
            Some(project)
        })
        .collect()
}

fn normalize_native_path(path: &str, semantics: ProjectPathSemantics) -> String {
    normalize_project_path(path, semantics).native_path
}

pub(crate) fn normalize_project_native_path(path: &str, semantics: ProjectPathSemantics) -> String {
    normalize_native_path(path, semantics)
}

fn normalize_project_path(path: &str, semantics: ProjectPathSemantics) -> NormalizedProjectPath {
    match semantics {
        ProjectPathSemantics::WindowsHost => normalize_windows_project_path(path),
        ProjectPathSemantics::Posix => normalize_posix_project_path(path),
    }
}

fn normalize_components<'a>(
    components: impl Iterator<Item = &'a str>,
    rooted: bool,
) -> Vec<String> {
    let mut normalized = Vec::new();
    for component in components {
        match component {
            "" | "." => {}
            ".." if normalized.last().is_some_and(|last| last != "..") => {
                normalized.pop();
            }
            ".." if !rooted => normalized.push(component.to_string()),
            ".." => {}
            _ => normalized.push(component.to_string()),
        }
    }
    normalized
}

fn normalize_posix_project_path(path: &str) -> NormalizedProjectPath {
    let trimmed = path.trim();
    let rooted = trimmed.starts_with('/');
    let components = normalize_components(trimmed.split('/'), rooted);
    let joined = components.join("/");
    let native_path = if rooted {
        if joined.is_empty() {
            "/".to_string()
        } else {
            format!("/{joined}")
        }
    } else {
        joined
    };
    NormalizedProjectPath {
        key: native_path.clone(),
        native_path,
    }
}

fn normalize_windows_project_path(path: &str) -> NormalizedProjectPath {
    let unified = path.trim().replace('/', "\\");
    if let Some(without_prefix) = unified.strip_prefix("\\\\") {
        let mut parts = without_prefix.split('\\').filter(|part| !part.is_empty());
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        let remainder = normalize_components(parts, true);
        if server.eq_ignore_ascii_case("wsl$") || server.eq_ignore_ascii_case("wsl.localhost") {
            let tail = remainder.join("\\");
            let native_path = if tail.is_empty() {
                format!("\\\\wsl.localhost\\{share}")
            } else {
                format!("\\\\wsl.localhost\\{share}\\{tail}")
            };
            let key = format!("wsl:{}\\{tail}", share.to_ascii_lowercase());
            return NormalizedProjectPath { native_path, key };
        }
        let mut components = Vec::new();
        if !server.is_empty() {
            components.push(server.to_string());
        }
        if !share.is_empty() {
            components.push(share.to_string());
        }
        components.extend(remainder);
        let native_path = format!("\\\\{}", components.join("\\"));
        return NormalizedProjectPath {
            key: native_path.to_ascii_lowercase(),
            native_path,
        };
    }

    let bytes = unified.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let drive = (bytes[0] as char).to_ascii_uppercase();
        let tail = &unified[2..];
        let rooted = tail.starts_with('\\');
        let components = normalize_components(tail.split('\\'), rooted);
        let joined = components.join("\\");
        let native_path = if rooted {
            if joined.is_empty() {
                format!("{drive}:\\")
            } else {
                format!("{drive}:\\{joined}")
            }
        } else {
            format!("{drive}:{joined}")
        };
        return NormalizedProjectPath {
            key: native_path.to_ascii_lowercase(),
            native_path,
        };
    }

    let components = normalize_components(unified.split('\\'), false);
    let native_path = components.join("\\");
    NormalizedProjectPath {
        key: native_path.to_ascii_lowercase(),
        native_path,
    }
}

fn atomic_write_json(path: &Path, value: &impl Serialize) -> Result<(), AppError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, value)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn config_backup_path(config_path: &Path) -> PathBuf {
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    config_path.with_extension(format!("json.projects-migration-{timestamp}.bak"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        add_project_binding, migrate_legacy_projects, ProjectMigrationRegistry,
        ProjectMigrationState, ProjectPathSemantics, ProjectsStore,
    };
    use crate::environment::types::ProjectBinding;
    use crate::error::AppError;

    #[test]
    fn missing_projects_file_returns_empty_registry() {
        let temp = tempdir().expect("tempdir");
        let store = ProjectsStore::new(temp.path().join("projects.json"));

        assert!(store.read().expect("read projects").is_empty());
    }

    #[test]
    fn projects_store_round_trips_and_deduplicates_native_paths() {
        let temp = tempdir().expect("tempdir");
        let store = ProjectsStore::new_with_semantics(
            temp.path().join("projects.json"),
            ProjectPathSemantics::WindowsHost,
        );
        let projects = vec![
            ProjectBinding {
                id: "first".to_string(),
                native_path: "C:\\Code\\app\\".to_string(),
                display_name: None,
                order: None,
                suppress_cross_storage_warning: false,
            },
            ProjectBinding {
                id: "second".to_string(),
                native_path: "C:\\Code\\app".to_string(),
                display_name: Some("duplicate".to_string()),
                order: None,
                suppress_cross_storage_warning: false,
            },
        ];

        store.write(&projects).expect("write projects");
        let saved = store.read().expect("read projects");

        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].id, "first");
        assert_eq!(saved[0].native_path, "C:\\Code\\app");
    }

    #[test]
    fn projects_store_persists_cross_storage_warning_suppression_per_binding() {
        let temp = tempdir().expect("tempdir");
        let store = ProjectsStore::new(temp.path().join("projects.json"));
        store
            .write(&[
                ProjectBinding {
                    id: "target".to_string(),
                    native_path: "/mnt/c/Code/app".to_string(),
                    display_name: None,
                    order: None,
                    suppress_cross_storage_warning: false,
                },
                ProjectBinding {
                    id: "other".to_string(),
                    native_path: "/home/alice/other".to_string(),
                    display_name: None,
                    order: None,
                    suppress_cross_storage_warning: false,
                },
            ])
            .expect("write projects");

        let saved = store
            .set_cross_storage_warning_suppressed("target", true)
            .expect("suppress warning");

        assert!(saved[0].suppress_cross_storage_warning);
        assert!(!saved[1].suppress_cross_storage_warning);
    }

    #[test]
    fn duplicate_add_returns_existing_binding_without_rewriting_registry() {
        let temp = tempdir().expect("tempdir");
        let projects_path = temp.path().join("projects.json");
        let original = br#"{
  "schemaVersion": 1,
  "projects": [{
    "id": "existing",
    "nativePath": "C:\\Code\\app",
    "displayName": null,
    "order": null,
    "suppressCrossStorageWarning": false
  }]
}
"#;
        fs::write(&projects_path, original).expect("seed projects");
        let store = ProjectsStore::new_with_semantics(
            projects_path.clone(),
            ProjectPathSemantics::WindowsHost,
        );

        let result = store
            .add("C:\\Code\\app\\".to_string())
            .expect("add duplicate");

        assert!(!result.created);
        assert_eq!(result.project.id, "existing");
        assert_eq!(fs::read(projects_path).expect("read projects"), original);
    }

    #[test]
    fn migration_moves_legacy_projects_once_and_preserves_other_config_fields() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.json");
        let projects_path = temp.path().join("projects.json");
        fs::write(
            &config_path,
            serde_json::to_vec_pretty(&json!({
                "projects": ["C:\\Code\\app", "\\\\wsl.localhost\\Ubuntu\\home\\user\\app"],
                "gitCloneTimeoutSecs": 300,
                "futureField": { "enabled": true }
            }))
            .expect("config json"),
        )
        .expect("write config");

        let state = migrate_legacy_projects(&config_path, &projects_path).expect("migrate");
        assert_eq!(state, ProjectMigrationState::Succeeded);
        let migrated = ProjectsStore::new(projects_path.clone())
            .read()
            .expect("read migrated projects");

        assert_eq!(migrated.len(), 2);
        assert!(migrated.iter().all(|project| !project.id.is_empty()));
        assert_eq!(
            migrated[1].native_path,
            "\\\\wsl.localhost\\Ubuntu\\home\\user\\app"
        );

        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(&config_path).expect("read config"))
                .expect("parse config");
        assert!(config.get("projects").is_none());
        assert_eq!(config["gitCloneTimeoutSecs"], 300);
        assert_eq!(config["futureField"]["enabled"], true);

        let second = migrate_legacy_projects(&config_path, &projects_path).expect("migrate again");
        assert_eq!(second, ProjectMigrationState::NotNeeded);
    }

    #[test]
    fn migration_keeps_legacy_projects_when_projects_write_fails() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.json");
        let projects_path = temp.path().join("projects.json");
        fs::write(
            &config_path,
            r#"{"projects":["/demo"],"gitCloneTimeoutSecs":120}"#,
        )
        .expect("write config");
        fs::create_dir(&projects_path).expect("block projects file");

        assert!(migrate_legacy_projects(&config_path, &projects_path).is_err());

        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(&config_path).expect("read config"))
                .expect("parse config");
        assert_eq!(config["projects"], json!(["/demo"]));
    }

    #[test]
    fn failed_migration_state_blocks_host_projects_until_replaced() {
        let registry = ProjectMigrationRegistry::new(ProjectMigrationState::Failed {
            error: AppError::Custom {
                message: "disk is read-only".to_string(),
            },
        });

        assert!(matches!(
            registry.ensure_ready(),
            Err(AppError::ProjectMigrationFailed { .. })
        ));

        registry.set(ProjectMigrationState::Succeeded);
        assert!(registry.ensure_ready().is_ok());
    }

    #[test]
    fn add_and_remove_project_use_stable_ids() {
        let temp = tempdir().expect("tempdir");
        let store = ProjectsStore::new(temp.path().join("projects.json"));

        let added = store.add("/work/demo".to_string()).expect("add project");
        assert!(added.created);
        let id = added.project.id;
        let duplicate = store.add("/work/demo/".to_string()).expect("add duplicate");
        assert!(!duplicate.created);
        assert_eq!(duplicate.projects.len(), 1);
        assert_eq!(duplicate.project.id, id);

        assert!(store.remove(&id).expect("remove project").is_empty());
    }

    #[test]
    fn windows_project_keys_normalize_separators_and_case() {
        let first = add_project_binding(
            Vec::new(),
            r"c:/Code/Skills/../App/".to_string(),
            ProjectPathSemantics::WindowsHost,
        );
        let duplicate = add_project_binding(
            first.projects,
            r"C:\code\app".to_string(),
            ProjectPathSemantics::WindowsHost,
        );

        assert_eq!(duplicate.project.native_path, r"C:\Code\App");
        assert!(!duplicate.created);
        assert_eq!(duplicate.projects.len(), 1);
    }

    #[test]
    fn wsl_unc_alias_and_distro_are_insensitive_but_linux_remainder_is_sensitive() {
        let first = add_project_binding(
            Vec::new(),
            r"\\wsl$\Ubuntu\home\alice\App".to_string(),
            ProjectPathSemantics::WindowsHost,
        );
        let duplicate = add_project_binding(
            first.projects,
            r"\\WSL.LOCALHOST\ubuntu\home\alice\App\".to_string(),
            ProjectPathSemantics::WindowsHost,
        );
        assert!(!duplicate.created);

        let distinct = add_project_binding(
            duplicate.projects,
            r"\\wsl.localhost\Ubuntu\home\alice\app".to_string(),
            ProjectPathSemantics::WindowsHost,
        );
        assert!(distinct.created);
        assert_eq!(distinct.projects.len(), 2);
    }

    #[test]
    fn posix_project_keys_are_lexical_and_case_sensitive() {
        let first = add_project_binding(
            Vec::new(),
            "/work/./Skills/../App/".to_string(),
            ProjectPathSemantics::Posix,
        );
        assert_eq!(first.project.native_path, "/work/App");

        let distinct = add_project_binding(
            first.projects,
            "/work/app".to_string(),
            ProjectPathSemantics::Posix,
        );
        assert!(distinct.created);
        assert_eq!(distinct.projects.len(), 2);
    }

    #[test]
    fn project_path_roots_are_preserved() {
        let posix = add_project_binding(Vec::new(), "/".to_string(), ProjectPathSemantics::Posix);
        let windows = add_project_binding(
            Vec::new(),
            "c:/".to_string(),
            ProjectPathSemantics::WindowsHost,
        );

        assert_eq!(posix.project.native_path, "/");
        assert_eq!(windows.project.native_path, "C:\\");
    }
}
