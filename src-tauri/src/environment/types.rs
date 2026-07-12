use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[specta(tag = "kind", rename_all = "camelCase")]
pub enum EnvironmentRef {
    Host,
    Wsl { distro_name: String },
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
    Unknown,
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

#[cfg(test)]
mod tests {
    use super::{ContextRef, ContextScope, EnvironmentRef};

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
}
