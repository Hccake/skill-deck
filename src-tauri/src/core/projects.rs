use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use uuid::Uuid;

use crate::environment::types::ProjectBinding;
use crate::error::AppError;

const PROJECTS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectsFile {
    pub(crate) schema_version: u32,
    pub(crate) projects: Vec<ProjectBinding>,
}

impl ProjectsFile {
    pub(crate) fn new(projects: Vec<ProjectBinding>) -> Self {
        Self {
            schema_version: PROJECTS_SCHEMA_VERSION,
            projects: deduplicate_projects(&projects),
        }
    }
}

pub(crate) fn add_project_binding(
    mut projects: Vec<ProjectBinding>,
    native_path: String,
) -> Vec<ProjectBinding> {
    let normalized = normalize_native_path(&native_path);
    if !projects
        .iter()
        .any(|project| normalize_native_path(&project.native_path) == normalized)
    {
        projects.push(ProjectBinding {
            id: Uuid::new_v4().to_string(),
            native_path: normalized,
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        });
    }
    deduplicate_projects(&projects)
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
}

impl ProjectsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
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
        let projects = deduplicate_projects(projects);
        let file = ProjectsFile {
            schema_version: PROJECTS_SCHEMA_VERSION,
            projects,
        };
        atomic_write_json(&self.path, &file)
    }

    pub fn add(&self, native_path: String) -> Result<Vec<ProjectBinding>, AppError> {
        let projects = add_project_binding(self.read()?, native_path);
        self.write(&projects)?;
        self.read()
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
) -> Result<Vec<ProjectBinding>, AppError> {
    let store = ProjectsStore::new(projects_path.to_path_buf());
    if projects_path.is_file() {
        return store.read();
    }
    if !config_path.exists() {
        return Ok(Vec::new());
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
        return Ok(Vec::new());
    }

    let projects = legacy_paths
        .into_iter()
        .map(|native_path| ProjectBinding {
            id: Uuid::new_v4().to_string(),
            native_path: normalize_native_path(&native_path),
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

    store.read()
}

fn deduplicate_projects(projects: &[ProjectBinding]) -> Vec<ProjectBinding> {
    let mut seen = HashSet::new();
    projects
        .iter()
        .filter_map(|project| {
            let normalized = normalize_native_path(&project.native_path);
            if !seen.insert(normalized.clone()) {
                return None;
            }
            let mut project = project.clone();
            project.native_path = normalized;
            Some(project)
        })
        .collect()
}

fn normalize_native_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed == "/" || trimmed.ends_with(":\\") {
        return trimmed.to_string();
    }
    trimmed.trim_end_matches(['/', '\\']).to_string()
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

    use super::{migrate_legacy_projects, ProjectsStore};
    use crate::environment::types::ProjectBinding;

    #[test]
    fn missing_projects_file_returns_empty_registry() {
        let temp = tempdir().expect("tempdir");
        let store = ProjectsStore::new(temp.path().join("projects.json"));

        assert!(store.read().expect("read projects").is_empty());
    }

    #[test]
    fn projects_store_round_trips_and_deduplicates_native_paths() {
        let temp = tempdir().expect("tempdir");
        let store = ProjectsStore::new(temp.path().join("projects.json"));
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

        let migrated = migrate_legacy_projects(&config_path, &projects_path).expect("migrate");

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
        assert_eq!(second, migrated);
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
    fn add_and_remove_project_use_stable_ids() {
        let temp = tempdir().expect("tempdir");
        let store = ProjectsStore::new(temp.path().join("projects.json"));

        let added = store.add("/work/demo".to_string()).expect("add project");
        let id = added[0].id.clone();
        let duplicate = store.add("/work/demo/".to_string()).expect("add duplicate");
        assert_eq!(duplicate.len(), 1);
        assert_eq!(duplicate[0].id, id);

        assert!(store.remove(&id).expect("remove project").is_empty());
    }
}
