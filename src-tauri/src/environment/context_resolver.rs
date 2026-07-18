use std::path::PathBuf;

use crate::core::projects::ProjectsStore;
use crate::core::{get_config_path, skill_lock};
use crate::environment::types::{
    ContextRef, ContextScope, EnvironmentKey, EnvironmentRef, ProjectBinding, ResourceLocator,
};
use crate::environment::wsl::operations::projects;
use crate::environment::wsl::WslSession;
use crate::error::AppError;

pub struct ContextResolver;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContext {
    pub context: ContextRef,
    pub project: Option<ProjectBinding>,
    pub home: ResourceLocator,
    pub skill_root: ResourceLocator,
    pub lock: ResourceLocator,
}

impl ResolvedContext {
    pub fn context_root(&self) -> &str {
        self.project
            .as_ref()
            .map(|project| project.native_path.as_str())
            .unwrap_or(self.home.native_path.as_str())
    }
}

impl ContextResolver {
    pub fn resolve_host(context: ContextRef) -> Result<ResolvedContext, AppError> {
        let home = dirs::home_dir().ok_or_else(|| AppError::Path {
            message: "cannot resolve home directory".to_string(),
        })?;
        let projects = if matches!(context.scope, ContextScope::Project { .. }) {
            ProjectsStore::new(get_config_path()?.with_file_name("projects.json")).read()?
        } else {
            Vec::new()
        };
        Self::resolve_host_from(context, home, skill_lock::get_skill_lock_path(), projects)
    }

    pub async fn resolve_wsl(
        context: ContextRef,
        session: &WslSession,
    ) -> Result<ResolvedContext, AppError> {
        let projects = if matches!(context.scope, ContextScope::Project { .. }) {
            projects::read_projects(session).await?
        } else {
            Vec::new()
        };
        Self::resolve_wsl_from_projects(context, session, projects)
    }

    pub(crate) fn resolve_host_from(
        context: ContextRef,
        home: PathBuf,
        global_lock: PathBuf,
        projects: Vec<ProjectBinding>,
    ) -> Result<ResolvedContext, AppError> {
        if context.environment != EnvironmentRef::Host {
            return Err(environment_mismatch(&context.environment));
        }

        let project = resolve_project(&context.scope, projects)?;
        let context_root = project
            .as_ref()
            .map(|project| PathBuf::from(&project.native_path))
            .unwrap_or_else(|| home.clone());
        let skill_root = context_root.join(".agents").join("skills");
        let lock = project.as_ref().map_or(global_lock, |project| {
            PathBuf::from(&project.native_path).join("skills-lock.json")
        });

        Ok(ResolvedContext {
            context,
            project,
            home: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: home.to_string_lossy().to_string(),
            },
            skill_root: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: skill_root.to_string_lossy().to_string(),
            },
            lock: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: lock.to_string_lossy().to_string(),
            },
        })
    }

    pub(crate) fn resolve_wsl_from_projects(
        context: ContextRef,
        session: &WslSession,
        projects: Vec<ProjectBinding>,
    ) -> Result<ResolvedContext, AppError> {
        let EnvironmentRef::Wsl { distro_name } = &context.environment else {
            return Err(environment_mismatch(&context.environment));
        };
        if EnvironmentKey::wsl(distro_name) != EnvironmentKey::wsl(&session.distro_name) {
            return Err(environment_mismatch(&context.environment));
        }

        let environment = context.environment.clone();
        let project = resolve_project(&context.scope, projects)?;
        let context_root = project
            .as_ref()
            .map(|project| project.native_path.as_str())
            .unwrap_or(session.home.as_str());
        let skill_root = join_wsl_path(context_root, ".agents/skills");
        let lock = project.as_ref().map_or_else(
            || {
                session
                    .xdg_state_home
                    .as_deref()
                    .filter(|path| !path.trim().is_empty())
                    .map(|path| join_wsl_path(path, "skills/.skill-lock.json"))
                    .unwrap_or_else(|| join_wsl_path(&session.home, ".agents/.skill-lock.json"))
            },
            |project| join_wsl_path(&project.native_path, "skills-lock.json"),
        );

        Ok(ResolvedContext {
            context,
            project,
            home: ResourceLocator {
                environment: environment.clone(),
                native_path: session.home.clone(),
            },
            skill_root: ResourceLocator {
                environment: environment.clone(),
                native_path: skill_root,
            },
            lock: ResourceLocator {
                environment,
                native_path: lock,
            },
        })
    }
}

fn resolve_project(
    scope: &ContextScope,
    projects: Vec<ProjectBinding>,
) -> Result<Option<ProjectBinding>, AppError> {
    match scope {
        ContextScope::Global => Ok(None),
        ContextScope::Project { project_id } => projects
            .into_iter()
            .find(|project| project.id == *project_id)
            .map(Some)
            .ok_or_else(|| AppError::PathNotFound {
                path: project_id.clone(),
            }),
    }
}

fn join_wsl_path(root: &str, child: &str) -> String {
    format!("{}/{}", root.trim_end_matches('/'), child)
}

fn environment_mismatch(environment: &EnvironmentRef) -> AppError {
    AppError::EnvironmentUnavailable {
        environment: environment.clone(),
        message: "context does not belong to the active environment".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::ContextResolver;
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ProjectBinding};
    use crate::environment::wsl::WslSession;
    use crate::error::AppError;

    fn project(id: &str, native_path: &str) -> ProjectBinding {
        ProjectBinding {
            id: id.to_string(),
            native_path: native_path.to_string(),
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        }
    }

    fn wsl_session(xdg_state_home: Option<&str>) -> WslSession {
        WslSession {
            distro_name: "Ubuntu".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: xdg_state_home.map(str::to_string),
            config_home: "/home/alice/.config".to_string(),
            environment: BTreeMap::new(),
            git_available: true,
            execution_profile: crate::environment::wsl_protocol::WslExecutionProfile::all_supported(
            ),
            runtime_generation: 0,
        }
    }

    #[test]
    fn resolves_host_global_and_project_resources() {
        let global = ContextResolver::resolve_host_from(
            ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Global,
            },
            PathBuf::from("/home/alice"),
            PathBuf::from("/state/skills/.skill-lock.json"),
            vec![project("app", "/work/app")],
        )
        .unwrap();
        assert_eq!(global.home.native_path, "/home/alice");
        assert_eq!(global.skill_root.native_path, "/home/alice/.agents/skills");
        assert_eq!(global.lock.native_path, "/state/skills/.skill-lock.json");
        assert_eq!(global.context_root(), "/home/alice");
        assert!(global.project.is_none());

        let project_context = ContextResolver::resolve_host_from(
            ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Project {
                    project_id: "app".to_string(),
                },
            },
            PathBuf::from("/home/alice"),
            PathBuf::from("/state/skills/.skill-lock.json"),
            vec![project("app", "/work/app")],
        )
        .unwrap();
        assert_eq!(
            project_context.skill_root.native_path,
            "/work/app/.agents/skills"
        );
        assert_eq!(
            project_context.lock.native_path,
            "/work/app/skills-lock.json"
        );
        assert_eq!(project_context.context_root(), "/work/app");
        assert_eq!(project_context.project.unwrap().id, "app");
    }

    #[test]
    fn resolves_wsl_global_and_project_resources() {
        let session = wsl_session(Some("/home/alice/.local/state"));
        let global = ContextResolver::resolve_wsl_from_projects(
            ContextRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                scope: ContextScope::Global,
            },
            &session,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(global.home.native_path, "/home/alice");
        assert_eq!(global.skill_root.native_path, "/home/alice/.agents/skills");
        assert_eq!(
            global.lock.native_path,
            "/home/alice/.local/state/skills/.skill-lock.json"
        );

        let project_context = ContextResolver::resolve_wsl_from_projects(
            ContextRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                scope: ContextScope::Project {
                    project_id: "app".to_string(),
                },
            },
            &session,
            vec![project("app", "/work/app")],
        )
        .unwrap();
        assert_eq!(
            project_context.skill_root.native_path,
            "/work/app/.agents/skills"
        );
        assert_eq!(
            project_context.lock.native_path,
            "/work/app/skills-lock.json"
        );
        assert_eq!(project_context.project.unwrap().id, "app");
    }

    #[test]
    fn rejects_missing_or_foreign_projects() {
        let missing = ContextResolver::resolve_wsl_from_projects(
            ContextRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                scope: ContextScope::Project {
                    project_id: "missing".to_string(),
                },
            },
            &wsl_session(None),
            vec![project("other", "/work/other")],
        )
        .unwrap_err();
        assert_eq!(
            missing,
            AppError::PathNotFound {
                path: "missing".to_string(),
            }
        );

        let foreign = ContextResolver::resolve_wsl_from_projects(
            ContextRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Debian".to_string(),
                },
                scope: ContextScope::Global,
            },
            &wsl_session(None),
            Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(foreign, AppError::EnvironmentUnavailable { .. }));
    }
}
