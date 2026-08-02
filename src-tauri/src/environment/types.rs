use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[specta(tag = "kind", rename_all = "camelCase")]
pub enum EnvironmentRef {
    Host,
    Wsl { distro_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum EnvironmentKey {
    Host,
    Wsl(String),
}

impl EnvironmentKey {
    pub fn from_ref(environment: &EnvironmentRef) -> Self {
        match environment {
            EnvironmentRef::Host => Self::Host,
            EnvironmentRef::Wsl { distro_name } => {
                Self::Wsl(normalized_wsl_distro_name(distro_name))
            }
        }
    }

    pub fn wsl(distro_name: &str) -> Self {
        Self::Wsl(normalized_wsl_distro_name(distro_name))
    }
}

pub fn normalized_wsl_distro_name(distro_name: &str) -> String {
    distro_name.to_ascii_lowercase()
}

pub fn same_environment_identity(left: &EnvironmentRef, right: &EnvironmentRef) -> bool {
    EnvironmentKey::from_ref(left) == EnvironmentKey::from_ref(right)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "scope", rename_all = "camelCase")]
#[specta(tag = "scope", rename_all = "camelCase")]
pub enum ContextScope {
    Global,
    Project { project_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ContextRef {
    pub environment: EnvironmentRef,
    pub scope: ContextScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ProjectBinding {
    pub id: String,
    pub native_path: String,
    pub display_name: Option<String>,
    pub order: Option<u32>,
    #[serde(default)]
    pub suppress_cross_storage_warning: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ResourceLocator {
    pub environment: EnvironmentRef,
    pub native_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum StorageAccess {
    Native,
    CrossStorage,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ProjectStorageInfo {
    pub access: StorageAccess,
    pub owner: Option<EnvironmentRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub binding: ProjectBinding,
    pub storage: ProjectStorageInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AddProjectResult {
    pub project: ProjectInfo,
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum EnvironmentStatus {
    Available,
    Connecting,
    Unavailable,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct EnvironmentRuntimeEvent {
    pub capability_revision: u64,
    pub revision: u64,
    pub environment: EnvironmentRef,
    pub status: EnvironmentStatus,
    pub error: Option<AppError>,
}

#[cfg(test)]
mod tests {
    use super::{
        same_environment_identity, AddProjectResult, ContextRef, ContextScope, EnvironmentKey,
        EnvironmentRef, ProjectBinding, ProjectInfo, ProjectStorageInfo, StorageAccess,
    };

    #[test]
    fn environment_ref_round_trips_host_and_wsl() {
        for environment in [
            EnvironmentRef::Host,
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu-24.04".to_string(),
            },
        ] {
            let json = serde_json::to_string(&environment).expect("serialize environment");
            let decoded: EnvironmentRef =
                serde_json::from_str(&json).expect("deserialize environment");
            assert_eq!(decoded, environment);
        }
    }

    #[test]
    fn environment_key_normalizes_wsl_identity_without_changing_display_name() {
        let display = EnvironmentRef::Wsl {
            distro_name: "Ubuntu-24.04".to_string(),
        };

        assert_eq!(
            EnvironmentKey::from_ref(&display),
            EnvironmentKey::Wsl("ubuntu-24.04".to_string())
        );
        assert_eq!(
            serde_json::to_value(&display).expect("serialize display environment"),
            serde_json::json!({ "kind": "wsl", "distro_name": "Ubuntu-24.04" })
        );
    }

    #[test]
    fn environment_identity_is_case_insensitive_only_within_the_same_wsl_distro() {
        let ubuntu = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let ubuntu_upper = EnvironmentRef::Wsl {
            distro_name: "UBUNTU".to_string(),
        };
        let debian = EnvironmentRef::Wsl {
            distro_name: "Debian".to_string(),
        };

        assert!(same_environment_identity(&ubuntu, &ubuntu_upper));
        assert!(!same_environment_identity(&ubuntu, &debian));
        assert!(!same_environment_identity(&EnvironmentRef::Host, &ubuntu));
    }

    #[test]
    fn context_ref_round_trips_global_and_project() {
        for scope in [
            ContextScope::Global,
            ContextScope::Project {
                project_id: "project-1".to_string(),
            },
        ] {
            let context = ContextRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                scope,
            };
            let json = serde_json::to_string(&context).expect("serialize context");
            let decoded: ContextRef = serde_json::from_str(&json).expect("deserialize context");
            assert_eq!(decoded, context);
        }
    }

    #[test]
    fn same_project_id_in_different_distros_is_not_same_context() {
        let ubuntu = ContextRef {
            environment: EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            scope: ContextScope::Project {
                project_id: "project-1".to_string(),
            },
        };
        let debian = ContextRef {
            environment: EnvironmentRef::Wsl {
                distro_name: "Debian".to_string(),
            },
            scope: ContextScope::Project {
                project_id: "project-1".to_string(),
            },
        };

        assert_ne!(ubuntu, debian);
    }

    #[test]
    fn project_info_keeps_runtime_storage_outside_the_persisted_binding() {
        let result = AddProjectResult {
            project: ProjectInfo {
                binding: ProjectBinding {
                    id: "project-1".to_string(),
                    native_path: "/work/app".to_string(),
                    display_name: None,
                    order: None,
                    suppress_cross_storage_warning: false,
                },
                storage: ProjectStorageInfo {
                    access: StorageAccess::Unknown,
                    owner: None,
                },
            },
            created: true,
        };

        let binding = serde_json::to_value(&result.project.binding).expect("serialize binding");
        assert!(binding.get("storage").is_none());
        assert_eq!(result.project.storage.access, StorageAccess::Unknown);
        assert!(result.created);
    }
}
