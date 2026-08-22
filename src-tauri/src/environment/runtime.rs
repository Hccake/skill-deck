use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use unicode_normalization::UnicodeNormalization;

use crate::environment::types::{
    same_environment_identity, EnvironmentKey, EnvironmentRef, RegisteredProject, ResourceLocator,
    SkillLocation, SkillLocationRef,
};
use crate::error::AppError;

const CONTEXT_REVISION_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExecutionBackend {
    NativeWindows,
    NativeUnix,
    WslPosix { distro_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PhysicalParentIdentity {
    Windows {
        volume_serial: u64,
        file_id: u128,
    },
    Unix {
        device: u64,
        inode: u64,
    },
    Wsl {
        distro_name: String,
        device: u64,
        inode: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalProjectIdentity {
    pub owner: EnvironmentRef,
    pub stable_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum PhysicalIdentityComparison {
    Same,
    Different,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalTargetKey {
    pub backend: ExecutionBackend,
    pub physical_parent: PhysicalParentIdentity,
    pub normalized_final_child_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryFingerprint(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ContextSnapshotRevision(String);

impl ContextSnapshotRevision {
    pub fn parse(value: impl Into<String>) -> Result<Self, AppError> {
        let value = value.into();
        validate_opaque_revision(&value, "contextRevision")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ObservedEntryId(String);

impl ObservedEntryId {
    #[cfg(test)]
    pub fn parse(value: impl Into<String>) -> Result<Self, AppError> {
        let value = value.into();
        validate_opaque_revision(&value, "observedEntryId")?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextOwnedRootFields {
    pub schema_version: u32,
    pub storage_mapping_fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRevisionInput {
    pub context: SkillLocationRef,
    pub selected_project: Option<RegisteredProject>,
    pub owned_root_fields: ContextOwnedRootFields,
    pub resolved_project_identity: Option<PhysicalProjectIdentity>,
    pub canonical_root_identity: PhysicalParentIdentity,
    pub lock_parent_identity: PhysicalParentIdentity,
    pub storage_mapping_identity: String,
}

#[cfg(test)]
pub fn physical_target_key(
    backend: ExecutionBackend,
    physical_parent: PhysicalParentIdentity,
    final_child_name: &str,
    case_sensitive: bool,
) -> Result<PhysicalTargetKey, AppError> {
    projected_physical_target_key(
        backend,
        physical_parent,
        std::iter::once(final_child_name),
        case_sensitive,
    )
}

pub fn projected_physical_target_key<I, S>(
    backend: ExecutionBackend,
    physical_parent: PhysicalParentIdentity,
    relative_components: I,
    case_sensitive: bool,
) -> Result<PhysicalTargetKey, AppError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut normalized_components = Vec::new();
    for component in relative_components {
        let component = component.as_ref().trim();
        if component.is_empty()
            || matches!(component, "." | "..")
            || component.contains(['/', '\\', '\0'])
        {
            return Err(AppError::UnsafePath {
                path: component.to_string(),
                reason: "invalid projected target component".to_string(),
            });
        }
        let normalized: String = component.nfc().collect();
        normalized_components.push(if case_sensitive {
            normalized
        } else {
            normalized.to_lowercase()
        });
    }
    if normalized_components.is_empty() {
        return Err(AppError::UnsafePath {
            path: String::new(),
            reason: "projected target requires a relative path".to_string(),
        });
    }
    Ok(PhysicalTargetKey {
        backend,
        physical_parent,
        normalized_final_child_name: normalized_components.join("/"),
    })
}

pub fn posix_relative_target(
    canonical_parent: &str,
    canonical_target: &str,
) -> Result<String, AppError> {
    let components = |path: &str| {
        if !path.starts_with('/') {
            return None;
        }
        path.split('/')
            .skip(1)
            .filter(|component| !component.is_empty())
            .map(|component| match component {
                "." | ".." => None,
                value => Some(value.to_string()),
            })
            .collect::<Option<Vec<_>>>()
    };
    let (Some(parent), Some(target)) = (components(canonical_parent), components(canonical_target))
    else {
        return Err(AppError::UnsafePath {
            path: canonical_target.to_string(),
            reason: "POSIX link target paths must be absolute and canonical".to_string(),
        });
    };
    let common = parent
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let relative = std::iter::repeat_n("..", parent.len() - common)
        .chain(target[common..].iter().map(String::as_str))
        .collect::<Vec<_>>();
    Ok(if relative.is_empty() {
        ".".to_string()
    } else {
        relative.join("/")
    })
}

pub fn physical_paths_overlap(
    left: &ResourceLocator,
    right: &ResourceLocator,
    case_sensitive: bool,
) -> Result<bool, AppError> {
    if !same_environment_identity(&left.environment, &right.environment) {
        return Ok(false);
    }
    let left = normalized_path_components(&left.native_path, case_sensitive)?;
    let right = normalized_path_components(&right.native_path, case_sensitive)?;
    Ok(is_component_prefix(&left, &right) || is_component_prefix(&right, &left))
}

fn normalized_path_components(path: &str, case_sensitive: bool) -> Result<Vec<String>, AppError> {
    let mut components = Vec::new();
    for component in path.replace('\\', "/").split('/') {
        match component {
            "" | "." => {}
            ".." => {
                return Err(AppError::UnsafePath {
                    path: path.to_string(),
                    reason: "physical path is not normalized".to_string(),
                });
            }
            value => components.push(if case_sensitive {
                value.to_string()
            } else {
                value.to_lowercase()
            }),
        }
    }
    Ok(components)
}

fn is_component_prefix(left: &[String], right: &[String]) -> bool {
    left.len() <= right.len() && left.iter().zip(right).all(|(left, right)| left == right)
}

pub fn context_snapshot_revision(
    input: &ContextRevisionInput,
) -> Result<ContextSnapshotRevision, AppError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ProjectProjection<'a> {
        id: &'a str,
        native_path: &'a str,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ContextProjection<'a> {
        environment: EnvironmentKey,
        scope: &'a SkillLocation,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ProjectIdentityProjection<'a> {
        owner: EnvironmentKey,
        stable_id: &'a str,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Projection<'a> {
        format_version: u32,
        context: ContextProjection<'a>,
        selected_project: Option<ProjectProjection<'a>>,
        schema_version: u32,
        storage_mapping_fields: &'a BTreeMap<String, String>,
        resolved_project_identity: Option<ProjectIdentityProjection<'a>>,
        canonical_root_identity: &'a PhysicalParentIdentity,
        lock_parent_identity: &'a PhysicalParentIdentity,
        storage_mapping_identity: &'a str,
    }

    let selected_project = input
        .selected_project
        .as_ref()
        .map(|project| ProjectProjection {
            id: &project.id,
            native_path: &project.native_path,
        });
    let resolved_project_identity =
        input
            .resolved_project_identity
            .as_ref()
            .map(|identity| ProjectIdentityProjection {
                owner: EnvironmentKey::from_ref(&identity.owner),
                stable_id: &identity.stable_id,
            });
    let encoded = serde_json::to_vec(&Projection {
        format_version: CONTEXT_REVISION_FORMAT_VERSION,
        context: ContextProjection {
            environment: EnvironmentKey::from_ref(&input.context.environment),
            scope: &input.context.scope,
        },
        selected_project,
        schema_version: input.owned_root_fields.schema_version,
        storage_mapping_fields: &input.owned_root_fields.storage_mapping_fields,
        resolved_project_identity,
        canonical_root_identity: &input.canonical_root_identity,
        lock_parent_identity: &input.lock_parent_identity,
        storage_mapping_identity: &input.storage_mapping_identity,
    })?;
    Ok(ContextSnapshotRevision(format!(
        "context-v{CONTEXT_REVISION_FORMAT_VERSION}-{:x}",
        Sha256::digest(encoded)
    )))
}

pub fn observed_entry_id(
    key: &PhysicalTargetKey,
    fingerprint: &EntryFingerprint,
) -> Result<ObservedEntryId, AppError> {
    let encoded = serde_json::to_vec(&(key, fingerprint))?;
    Ok(ObservedEntryId(format!(
        "entry-v1-{:x}",
        Sha256::digest(encoded)
    )))
}

fn validate_opaque_revision(value: &str, field: &str) -> Result<(), AppError> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AppError::Validation {
            field: Some(field.to_string()),
            message: "invalid opaque identity".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::environment::types::{
        EnvironmentRef, RegisteredProject, SkillLocation, SkillLocationRef,
    };

    fn backend() -> ExecutionBackend {
        ExecutionBackend::NativeUnix
    }

    fn parent(inode: u64) -> PhysicalParentIdentity {
        PhysicalParentIdentity::Unix { device: 7, inode }
    }

    #[test]
    fn projected_target_key_keeps_missing_root_suffix_under_existing_identity() {
        let key = projected_physical_target_key(
            backend(),
            parent(9),
            [".custom", "skills", "demo"],
            true,
        )
        .unwrap();

        assert_eq!(key.physical_parent, parent(9));
        assert_eq!(key.normalized_final_child_name, ".custom/skills/demo");
        assert!(projected_physical_target_key(
            backend(),
            parent(9),
            [".custom", "..", "demo"],
            true,
        )
        .is_err());
    }

    #[test]
    fn posix_relative_target_uses_the_physical_parent() {
        assert_eq!(
            posix_relative_target(
                "/home/alice/.config/agent/skills",
                "/home/alice/.agents/skills/demo",
            )
            .unwrap(),
            "../../../.agents/skills/demo"
        );
    }

    #[test]
    fn posix_relative_target_rejects_non_absolute_paths() {
        assert!(posix_relative_target("agent/skills", "/home/alice/.agents/skills/demo",).is_err());
    }

    fn project(id: &str, path: &str) -> RegisteredProject {
        RegisteredProject {
            id: id.to_string(),
            native_path: path.to_string(),
            display_name: Some("display-only".to_string()),
            order: Some(9),
            suppress_cross_storage_warning: false,
        }
    }

    fn context_input(mapping_fields: BTreeMap<String, String>) -> ContextRevisionInput {
        ContextRevisionInput {
            context: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Project {
                    project_id: "app".to_string(),
                },
            },
            selected_project: Some(project("app", "/work/app")),
            owned_root_fields: ContextOwnedRootFields {
                schema_version: 1,
                storage_mapping_fields: mapping_fields,
            },
            resolved_project_identity: Some(PhysicalProjectIdentity {
                owner: EnvironmentRef::Native,
                stable_id: "volume-7-file-9".to_string(),
            }),
            canonical_root_identity: parent(10),
            lock_parent_identity: parent(11),
            storage_mapping_identity: "native-v1".to_string(),
        }
    }

    #[test]
    fn target_key_uses_parent_and_final_name_without_following_final_link() {
        let first = physical_target_key(backend(), parent(10), "Skill", true).expect("key");
        let same = physical_target_key(backend(), parent(10), "Skill", true).expect("same key");
        let other_parent =
            physical_target_key(backend(), parent(11), "Skill", true).expect("other parent");
        let other_case =
            physical_target_key(backend(), parent(10), "skill", true).expect("other case");

        assert_eq!(first, same);
        assert_ne!(first, other_parent);
        assert_ne!(first, other_case);
    }

    #[test]
    fn target_key_case_folds_only_for_case_insensitive_root() {
        let upper = physical_target_key(backend(), parent(10), "Skill", false).expect("upper");
        let lower = physical_target_key(backend(), parent(10), "skill", false).expect("lower");
        assert_eq!(upper, lower);
    }

    #[test]
    fn physical_path_overlap_uses_component_and_environment_identity_rules() {
        let locator = |environment, native_path: &str| ResourceLocator {
            environment,
            native_path: native_path.to_string(),
        };
        let native = locator(EnvironmentRef::Native, r"C:\Code\App");
        let native_child = locator(EnvironmentRef::Native, r"c:\code\app\skills");
        let native_sibling = locator(EnvironmentRef::Native, r"C:\Code\Application");
        let wsl = locator(
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            "/mnt/c/Code/App",
        );

        assert!(physical_paths_overlap(&native, &native_child, false).expect("native overlap"));
        assert!(!physical_paths_overlap(&native, &native_sibling, false).expect("native sibling"));
        assert!(!physical_paths_overlap(&native, &wsl, false).expect("different Environment"));
    }

    #[test]
    fn context_revision_is_deterministic_and_scoped_to_owned_inputs() {
        let mut first_fields = BTreeMap::new();
        first_fields.insert("mountRoot".to_string(), "/mnt".to_string());
        first_fields.insert("mappingVersion".to_string(), "1".to_string());
        let mut reversed_fields = BTreeMap::new();
        reversed_fields.insert("mappingVersion".to_string(), "1".to_string());
        reversed_fields.insert("mountRoot".to_string(), "/mnt".to_string());

        let first = context_snapshot_revision(&context_input(first_fields)).expect("revision");
        let same = context_snapshot_revision(&context_input(reversed_fields)).expect("revision");
        assert_eq!(first, same);

        let mut display_only = context_input(BTreeMap::from([
            ("mappingVersion".to_string(), "1".to_string()),
            ("mountRoot".to_string(), "/mnt".to_string()),
        ]));
        let selected = display_only.selected_project.as_mut().expect("project");
        selected.display_name = Some("renamed".to_string());
        selected.order = Some(1);
        selected.suppress_cross_storage_warning = true;
        assert_eq!(
            first,
            context_snapshot_revision(&display_only).expect("display-only revision")
        );

        let mut changed = context_input(BTreeMap::new());
        changed.selected_project = Some(project("app", "/work/other"));
        assert_ne!(
            first,
            context_snapshot_revision(&changed).expect("changed revision")
        );
    }

    #[test]
    fn context_revision_uses_normalized_environment_identity() {
        let mut ubuntu = context_input(BTreeMap::new());
        ubuntu.context.environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        ubuntu.resolved_project_identity = Some(PhysicalProjectIdentity {
            owner: EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            stable_id: "7:9".to_string(),
        });
        let mut alias = ubuntu.clone();
        alias.context.environment = EnvironmentRef::Wsl {
            distro_name: "UBUNTU".to_string(),
        };
        alias.resolved_project_identity = Some(PhysicalProjectIdentity {
            owner: EnvironmentRef::Wsl {
                distro_name: "ubuntu".to_string(),
            },
            stable_id: "7:9".to_string(),
        });
        let mut debian = ubuntu.clone();
        debian.context.environment = EnvironmentRef::Wsl {
            distro_name: "Debian".to_string(),
        };
        debian.resolved_project_identity = Some(PhysicalProjectIdentity {
            owner: EnvironmentRef::Wsl {
                distro_name: "Debian".to_string(),
            },
            stable_id: "7:9".to_string(),
        });

        assert_eq!(
            context_snapshot_revision(&ubuntu).expect("Ubuntu revision"),
            context_snapshot_revision(&alias).expect("alias revision")
        );
        assert_ne!(
            context_snapshot_revision(&ubuntu).expect("Ubuntu revision"),
            context_snapshot_revision(&debian).expect("Debian revision")
        );
    }

    #[test]
    fn observed_entry_id_binds_physical_key_and_fingerprint() {
        let key = physical_target_key(backend(), parent(10), "skill", true).expect("key");
        let first =
            observed_entry_id(&key, &EntryFingerprint("entry-v1".to_string())).expect("entry ID");
        let same =
            observed_entry_id(&key, &EntryFingerprint("entry-v1".to_string())).expect("same ID");
        let changed =
            observed_entry_id(&key, &EntryFingerprint("entry-v2".to_string())).expect("changed ID");

        assert_eq!(first, same);
        assert_ne!(first, changed);
    }
}
