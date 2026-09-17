use std::path::PathBuf;

use serde::Serialize;
use specta::Type;

use crate::core::projects::ProjectsStore;
use crate::core::{get_config_path, skill_lock};
use crate::environment::types::{
    EnvironmentKey, EnvironmentRef, RegisteredProject, ResourceLocator, SkillLocation,
    SkillLocationRef,
};
use crate::environment::wsl::operations::projects;
use crate::environment::wsl::{WslSession, WslWorkspace};
use crate::error::AppError;

pub struct ContextResolver;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContext {
    pub context: SkillLocationRef,
    pub project: Option<RegisteredProject>,
    pub home: ResourceLocator,
    pub skill_root: ResourceLocator,
    pub lock: ResourceLocator,
}

/// 仅供界面缩短安装路径；执行仍使用原有目标标识与物理身份。
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ScopePathBase {
    pub logical_root: ResourceLocator,
    pub physical_root: Option<ResourceLocator>,
    pub path_style: DisplayPathStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum DisplayPathStyle {
    Posix,
    Windows,
}

impl ResolvedContext {
    pub fn context_root(&self) -> &str {
        self.project
            .as_ref()
            .map(|project| project.native_path.as_str())
            .unwrap_or(self.home.native_path.as_str())
    }

    pub(crate) async fn resolved_root(
        &self,
        targets: &dyn crate::environment::planning::TargetFactResolver,
    ) -> Result<ResourceLocator, AppError> {
        use crate::environment::types::same_environment_identity;

        let logical_root = ResourceLocator {
            environment: self.context.environment.clone(),
            native_path: self.context_root().to_string(),
        };
        // 通过虚拟子路径观察父目录的实际身份，不创建目录或探针文件。
        const PROBE: &str = ".skill-deck-path-base";
        let facts = targets
            .resolve(&self.context, &[logical_root.join_child(PROBE)], None)
            .await?;
        let [fact] = facts.as_slice() else {
            return Err(AppError::StaleTarget);
        };
        if !same_environment_identity(&fact.destination.environment, &self.context.environment) {
            return Err(AppError::StaleEnvironment);
        }
        if fact.key.normalized_final_child_name != PROBE {
            return Err(AppError::PathNotFound {
                path: logical_root.native_path,
            });
        }
        let native_path = match &self.context.environment {
            EnvironmentRef::Native => std::path::Path::new(&fact.destination.native_path)
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned()),
            EnvironmentRef::Wsl { .. } => {
                fact.destination
                    .native_path
                    .rsplit_once('/')
                    .map(|(parent, _)| {
                        if parent.is_empty() {
                            "/".into()
                        } else {
                            parent.into()
                        }
                    })
            }
        }
        .ok_or(AppError::StaleTarget)?;
        Ok(ResourceLocator {
            environment: self.context.environment.clone(),
            native_path,
        })
    }

    pub async fn path_base(
        &self,
        targets: &dyn crate::environment::planning::TargetFactResolver,
    ) -> ScopePathBase {
        use crate::environment::types::display_locator;

        ScopePathBase {
            logical_root: display_locator(&ResourceLocator {
                environment: self.context.environment.clone(),
                native_path: self.context_root().to_string(),
            }),
            physical_root: self
                .resolved_root(targets)
                .await
                .ok()
                .map(|root| display_locator(&root)),
            path_style: if cfg!(windows) && self.context.environment == EnvironmentRef::Native {
                DisplayPathStyle::Windows
            } else {
                DisplayPathStyle::Posix
            },
        }
    }
}

impl ContextResolver {
    pub fn resolve_native(context: SkillLocationRef) -> Result<ResolvedContext, AppError> {
        let home = dirs::home_dir().ok_or_else(|| AppError::Path {
            message: "cannot resolve home directory".to_string(),
        })?;
        let projects = if matches!(context.scope, SkillLocation::Project { .. }) {
            ProjectsStore::new(get_config_path()?.with_file_name("projects.json")).read()?
        } else {
            Vec::new()
        };
        Self::resolve_native_from(context, home, skill_lock::get_skill_lock_path(), projects)
    }

    pub async fn resolve_wsl(
        context: SkillLocationRef,
        session: &WslSession,
        workspace: &WslWorkspace,
    ) -> Result<ResolvedContext, AppError> {
        let projects = if matches!(context.scope, SkillLocation::Project { .. }) {
            projects::read_projects(session, workspace).await?
        } else {
            Vec::new()
        };
        Self::resolve_wsl_from_projects(context, session, projects)
    }

    pub(crate) fn resolve_native_from(
        context: SkillLocationRef,
        home: PathBuf,
        global_lock: PathBuf,
        projects: Vec<RegisteredProject>,
    ) -> Result<ResolvedContext, AppError> {
        if context.environment != EnvironmentRef::Native {
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
                environment: EnvironmentRef::Native,
                native_path: home.to_string_lossy().to_string(),
            },
            skill_root: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: skill_root.to_string_lossy().to_string(),
            },
            lock: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: lock.to_string_lossy().to_string(),
            },
        })
    }

    pub(crate) fn resolve_wsl_from_projects(
        context: SkillLocationRef,
        session: &WslSession,
        projects: Vec<RegisteredProject>,
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
    scope: &SkillLocation,
    projects: Vec<RegisteredProject>,
) -> Result<Option<RegisteredProject>, AppError> {
    match scope {
        SkillLocation::Global => Ok(None),
        SkillLocation::Project { project_id } => projects
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
    use crate::environment::types::{
        EnvironmentRef, RegisteredProject, SkillLocation, SkillLocationRef,
    };
    use crate::environment::wsl::WslSession;
    use crate::error::AppError;

    #[tokio::test]
    async fn path_base_resolves_project_aliases_without_creating_files() {
        use crate::environment::planning::RuntimeTargetFactResolver;
        use crate::environment::types::display_locator;
        use crate::environment::wsl::WslRuntime;
        use std::sync::Arc;

        let temp = tempfile::tempdir().unwrap();
        let actual = temp.path().join("project");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&actual).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        #[cfg(windows)]
        junction::create(&actual, &alias).unwrap();
        let resolved = ContextResolver::resolve_native_from(
            SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Project {
                    project_id: "app".into(),
                },
            },
            temp.path().join("home"),
            temp.path().join("lock.json"),
            vec![project("app", &alias.to_string_lossy())],
        )
        .unwrap();
        let targets = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));

        let base = resolved.path_base(&targets).await;

        assert_eq!(base.logical_root.native_path, alias.to_string_lossy());
        let expected = crate::environment::types::ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: std::fs::canonicalize(&actual)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        };
        assert_eq!(base.physical_root, Some(display_locator(&expected)));
        assert_eq!(std::fs::read_dir(actual).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn path_base_retains_selected_wsl_home_when_projection_is_unavailable() {
        use crate::environment::planning::{
            ResolvedTargetFact, TargetFactFuture, TargetFactResolver,
        };
        use crate::environment::types::ResourceLocator;

        struct Unavailable;
        impl TargetFactResolver for Unavailable {
            fn resolve<'a>(
                &'a self,
                _: &'a SkillLocationRef,
                _: &'a [ResourceLocator],
                _: Option<crate::core::mutation::CancellationSignal>,
            ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
                Box::pin(async { Err(AppError::StaleEnvironment) })
            }
        }
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".into(),
        };
        let resolved = ContextResolver::resolve_wsl_from_projects(
            SkillLocationRef {
                environment: environment.clone(),
                scope: SkillLocation::Global,
            },
            &wsl_session(None),
            vec![],
        )
        .unwrap();

        let base = resolved.path_base(&Unavailable).await;

        assert_eq!(
            base.logical_root,
            ResourceLocator {
                environment,
                native_path: "/home/alice".into()
            }
        );
        assert_eq!(base.physical_root, None);
        assert_eq!(base.path_style, super::DisplayPathStyle::Posix);
    }

    fn project(id: &str, native_path: &str) -> RegisteredProject {
        RegisteredProject {
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
            runtime_generation: 0,
        }
    }

    #[test]
    fn resolves_native_global_and_project_resources() {
        let global = ContextResolver::resolve_native_from(
            SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            PathBuf::from("/home/alice"),
            PathBuf::from("/state/skills/.skill-lock.json"),
            vec![project("app", "/work/app")],
        )
        .unwrap();
        assert_eq!(global.home.native_path, "/home/alice");
        assert_eq!(
            global.skill_root.native_path,
            PathBuf::from("/home/alice")
                .join(".agents")
                .join("skills")
                .to_string_lossy()
        );
        assert_eq!(global.lock.native_path, "/state/skills/.skill-lock.json");
        assert_eq!(global.context_root(), "/home/alice");
        assert!(global.project.is_none());

        let project_context = ContextResolver::resolve_native_from(
            SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Project {
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
            PathBuf::from("/work/app")
                .join(".agents")
                .join("skills")
                .to_string_lossy()
        );
        assert_eq!(
            project_context.lock.native_path,
            PathBuf::from("/work/app")
                .join("skills-lock.json")
                .to_string_lossy()
        );
        assert_eq!(project_context.context_root(), "/work/app");
        assert_eq!(project_context.project.unwrap().id, "app");
    }

    #[test]
    fn resolves_wsl_global_and_project_resources() {
        let session = wsl_session(Some("/home/alice/.local/state"));
        let global = ContextResolver::resolve_wsl_from_projects(
            SkillLocationRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                scope: SkillLocation::Global,
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
            SkillLocationRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                scope: SkillLocation::Project {
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
            SkillLocationRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                scope: SkillLocation::Project {
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
            SkillLocationRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Debian".to_string(),
                },
                scope: SkillLocation::Global,
            },
            &wsl_session(None),
            Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(foreign, AppError::EnvironmentUnavailable { .. }));
    }
}
