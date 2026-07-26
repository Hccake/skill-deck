use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentFieldError {
    pub field: String,
    pub code: String,
}

impl AgentFieldError {
    pub fn new(field: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            code: code.into(),
        }
    }
}

impl fmt::Display for AgentFieldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.field, self.code)
    }
}

impl std::error::Error for AgentFieldError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    pub fn parse(value: impl Into<String>) -> Result<Self, AgentFieldError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.split('-').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            });

        if !valid {
            return Err(AgentFieldError::new("id", "invalidAgentId"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> Result<(), AgentFieldError> {
        Self::parse(self.0.clone()).map(|_| ())
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for AgentId {
    type Err = AgentFieldError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum AgentSource {
    Builtin,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum ScopeLocation {
    Shared,
    Private,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum CustomPathBase {
    Home,
    ConfigHome,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[specta(tag = "kind", rename_all = "camelCase")]
pub enum PathSpec {
    Home {
        #[serde(rename = "relativePath")]
        #[specta(rename = "relativePath")]
        relative_path: String,
    },
    ConfigHome {
        #[serde(rename = "relativePath")]
        #[specta(rename = "relativePath")]
        relative_path: String,
    },
    Project {
        #[serde(rename = "relativePath")]
        #[specta(rename = "relativePath")]
        relative_path: String,
    },
    EnvironmentVariable {
        name: String,
        #[serde(rename = "relativePath")]
        #[specta(rename = "relativePath")]
        relative_path: String,
        fallback: Box<PathSpec>,
    },
    FirstExisting {
        candidates: Vec<PathSpec>,
        fallback: Box<PathSpec>,
    },
    /// Absolute paths are reserved for built-in system detection candidates.
    Absolute { path: String },
}

impl PathSpec {
    pub fn home(relative_path: impl Into<String>) -> Self {
        Self::Home {
            relative_path: relative_path.into(),
        }
    }

    pub fn config_home(relative_path: impl Into<String>) -> Self {
        Self::ConfigHome {
            relative_path: relative_path.into(),
        }
    }

    pub fn project(relative_path: impl Into<String>) -> Self {
        Self::Project {
            relative_path: relative_path.into(),
        }
    }

    pub fn absolute(path: impl Into<String>) -> Self {
        Self::Absolute { path: path.into() }
    }

    #[cfg(test)]
    fn contains_project(&self) -> bool {
        match self {
            Self::Project { .. } => true,
            Self::EnvironmentVariable { fallback, .. } => fallback.contains_project(),
            Self::FirstExisting {
                candidates,
                fallback,
            } => candidates.iter().any(Self::contains_project) || fallback.contains_project(),
            Self::Home { .. } | Self::ConfigHome { .. } | Self::Absolute { .. } => false,
        }
    }

    #[cfg(test)]
    fn contains_absolute(&self) -> bool {
        match self {
            Self::Absolute { .. } => true,
            Self::EnvironmentVariable { fallback, .. } => fallback.contains_absolute(),
            Self::FirstExisting {
                candidates,
                fallback,
            } => candidates.iter().any(Self::contains_absolute) || fallback.contains_absolute(),
            Self::Home { .. } | Self::ConfigHome { .. } | Self::Project { .. } => false,
        }
    }

    #[cfg(test)]
    fn validate(&self, field: &str) -> Result<(), AgentFieldError> {
        match self {
            Self::Home { relative_path }
            | Self::ConfigHome { relative_path }
            | Self::Project { relative_path } => validate_relative_path(relative_path, field),
            Self::EnvironmentVariable {
                name,
                relative_path,
                fallback,
            } => {
                if name.trim().is_empty() || name.contains('\0') {
                    return Err(AgentFieldError::new(
                        format!("{field}.name"),
                        "invalidEnvName",
                    ));
                }
                if !relative_path.is_empty() {
                    validate_relative_path(relative_path, &format!("{field}.relativePath"))?;
                }
                fallback.validate(&format!("{field}.fallback"))
            }
            Self::FirstExisting {
                candidates,
                fallback,
            } => {
                if candidates.is_empty() {
                    return Err(AgentFieldError::new(
                        format!("{field}.candidates"),
                        "required",
                    ));
                }
                for (index, candidate) in candidates.iter().enumerate() {
                    candidate.validate(&format!("{field}.candidates[{index}]"))?;
                }
                fallback.validate(&format!("{field}.fallback"))
            }
            Self::Absolute { path } => validate_absolute_path(path, &format!("{field}.path")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[specta(tag = "kind", rename_all = "camelCase")]
pub enum DetectionSpec {
    AnyPathExists { paths: Vec<PathSpec> },
    Eve,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ScopeDefinition {
    pub enabled: bool,
    pub reads_shared: bool,
    pub private_path: Option<PathSpec>,
}

impl ScopeDefinition {
    #[cfg(test)]
    fn validate(&self, field: &str) -> Result<(), AgentFieldError> {
        if self.enabled && !self.reads_shared && self.private_path.is_none() {
            return Err(AgentFieldError::new(field, "invalidScope"));
        }
        if let Some(path) = &self.private_path {
            path.validate(&format!("{field}.privatePath"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LegacyPathScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LegacyPathBehavior {
    DetectOnly,
    OfferMigration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LegacyMigrationTarget {
    CurrentPrivate,
    SharedCanonical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LegacyPath {
    pub scope: LegacyPathScope,
    pub path: PathSpec,
    pub behavior: LegacyPathBehavior,
    pub migration_target: LegacyMigrationTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum AgentAdapter {
    Standard,
    Eve,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentDefinition {
    pub id: AgentId,
    pub display_name: String,
    pub source: AgentSource,
    pub aliases: Vec<AgentId>,
    pub global: ScopeDefinition,
    pub project: ScopeDefinition,
    pub detection: DetectionSpec,
    pub legacy_paths: Vec<LegacyPath>,
    pub adapter: AgentAdapter,
}

impl AgentDefinition {
    #[cfg(test)]
    pub fn validate(&self) -> Result<(), AgentFieldError> {
        self.id.validate()?;
        if self.display_name.trim().is_empty() {
            return Err(AgentFieldError::new("displayName", "required"));
        }
        if !self.global.enabled && !self.project.enabled {
            return Err(AgentFieldError::new("scopes", "required"));
        }
        self.global.validate("global")?;
        self.project.validate("project")?;
        if self
            .global
            .private_path
            .as_ref()
            .is_some_and(PathSpec::contains_project)
        {
            return Err(AgentFieldError::new(
                "global.privatePath.base",
                "projectBaseNotAllowed",
            ));
        }
        if self
            .project
            .private_path
            .as_ref()
            .is_some_and(PathSpec::contains_absolute)
        {
            return Err(AgentFieldError::new(
                "project.privatePath.kind",
                "absolutePathNotAllowed",
            ));
        }

        match &self.detection {
            DetectionSpec::AnyPathExists { paths } => {
                if paths.is_empty() {
                    return Err(AgentFieldError::new("detection.paths", "required"));
                }
                for (index, path) in paths.iter().enumerate() {
                    path.validate(&format!("detection.paths[{index}]"))?;
                }
            }
            DetectionSpec::Eve => {
                if self.adapter != AgentAdapter::Eve {
                    return Err(AgentFieldError::new("adapter", "invalidAdapter"));
                }
            }
        }

        for (index, legacy) in self.legacy_paths.iter().enumerate() {
            legacy
                .path
                .validate(&format!("legacyPaths[{index}].path"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[specta(tag = "kind", rename_all = "camelCase")]
pub enum CustomPathSpec {
    Based {
        base: CustomPathBase,
        #[serde(rename = "relativePath")]
        #[specta(rename = "relativePath")]
        relative_path: String,
    },
    Absolute {
        path: String,
    },
}

impl CustomPathSpec {
    #[cfg(test)]
    pub fn based(base: CustomPathBase, relative_path: impl Into<String>) -> Self {
        Self::Based {
            base,
            relative_path: relative_path.into(),
        }
    }

    #[cfg(test)]
    pub fn absolute(path: impl Into<String>) -> Self {
        Self::Absolute { path: path.into() }
    }

    fn validate(&self, field: &str) -> Result<(), AgentFieldError> {
        match self {
            Self::Based { relative_path, .. } => {
                validate_relative_path(relative_path, &format!("{field}.relativePath"))
            }
            Self::Absolute { path } => validate_absolute_path(path, &format!("{field}.path")),
        }
    }

    pub fn normalize(&self) -> PathSpec {
        match self {
            Self::Based {
                base,
                relative_path,
            } => match base {
                CustomPathBase::Home => PathSpec::home(relative_path.clone()),
                CustomPathBase::ConfigHome => PathSpec::config_home(relative_path.clone()),
                CustomPathBase::Project => PathSpec::project(relative_path.clone()),
            },
            Self::Absolute { path } => PathSpec::absolute(path.clone()),
        }
    }

    fn is_project_based(&self) -> bool {
        matches!(
            self,
            Self::Based {
                base: CustomPathBase::Project,
                ..
            }
        )
    }

    fn is_absolute(&self) -> bool {
        matches!(self, Self::Absolute { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CustomScopeDefinition {
    pub enabled: bool,
    pub location: ScopeLocation,
    pub private_path: Option<CustomPathSpec>,
}

impl CustomScopeDefinition {
    #[cfg(test)]
    pub fn normalize(&self) -> Result<ScopeDefinition, AgentFieldError> {
        self.normalize_at("")
    }

    fn normalize_at(&self, field: &str) -> Result<ScopeDefinition, AgentFieldError> {
        let private_field = if field.is_empty() {
            "privatePath".to_string()
        } else {
            format!("{field}.privatePath")
        };

        match self.location {
            ScopeLocation::Shared if self.private_path.is_some() => {
                return Err(AgentFieldError::new(private_field, "forbidden"));
            }
            ScopeLocation::Private | ScopeLocation::Both if self.private_path.is_none() => {
                return Err(AgentFieldError::new(private_field, "required"));
            }
            _ => {}
        }

        if let Some(path) = &self.private_path {
            let path_field = if field.is_empty() {
                "privatePath".to_string()
            } else {
                format!("{field}.privatePath")
            };
            path.validate(&path_field)?;
        }

        Ok(ScopeDefinition {
            enabled: self.enabled,
            reads_shared: matches!(self.location, ScopeLocation::Shared | ScopeLocation::Both),
            private_path: self.private_path.as_ref().map(CustomPathSpec::normalize),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CustomAgentDefinition {
    pub id: AgentId,
    pub display_name: String,
    pub global: CustomScopeDefinition,
    pub project: CustomScopeDefinition,
    pub detection_paths: Vec<CustomPathSpec>,
}

impl CustomAgentDefinition {
    pub fn validate(&self) -> Result<(), AgentFieldError> {
        self.id.validate()?;
        if self.display_name.trim().is_empty() {
            return Err(AgentFieldError::new("displayName", "required"));
        }
        if !self.global.enabled && !self.project.enabled {
            return Err(AgentFieldError::new("scopes", "required"));
        }

        self.global.normalize_at("global")?;
        self.project.normalize_at("project")?;

        if self
            .global
            .private_path
            .as_ref()
            .is_some_and(CustomPathSpec::is_project_based)
        {
            return Err(AgentFieldError::new(
                "global.privatePath.base",
                "projectBaseNotAllowed",
            ));
        }
        if self
            .project
            .private_path
            .as_ref()
            .is_some_and(CustomPathSpec::is_absolute)
        {
            return Err(AgentFieldError::new(
                "project.privatePath.kind",
                "absolutePathNotAllowed",
            ));
        }

        if self.detection_paths.is_empty() {
            return Err(AgentFieldError::new("detectionPaths", "required"));
        }
        for (index, path) in self.detection_paths.iter().enumerate() {
            path.validate(&format!("detectionPaths[{index}]"))?;
        }
        Ok(())
    }

    pub fn normalize(&self) -> Result<AgentDefinition, AgentFieldError> {
        self.validate()?;
        let mut seen = HashSet::new();
        let paths = self
            .detection_paths
            .iter()
            .filter(|path| seen.insert((*path).clone()))
            .map(CustomPathSpec::normalize)
            .collect();

        Ok(AgentDefinition {
            id: self.id.clone(),
            display_name: self.display_name.trim().to_string(),
            source: AgentSource::Custom,
            aliases: Vec::new(),
            global: self.global.normalize_at("global")?,
            project: self.project.normalize_at("project")?,
            detection: DetectionSpec::AnyPathExists { paths },
            legacy_paths: Vec::new(),
            adapter: AgentAdapter::Standard,
        })
    }
}

fn validate_relative_path(value: &str, field: &str) -> Result<(), AgentFieldError> {
    let normalized = value.replace('\\', "/");
    let drive_absolute = normalized.as_bytes().get(1) == Some(&b':');
    let invalid = normalized.trim().is_empty()
        || normalized == "."
        || normalized.starts_with('/')
        || drive_absolute
        || normalized.contains('\0')
        || normalized
            .split('/')
            .any(|component| component == "." || component == "..");

    if invalid {
        return Err(AgentFieldError::new(field, "invalidRelativePath"));
    }
    Ok(())
}

fn validate_absolute_path(value: &str, field: &str) -> Result<(), AgentFieldError> {
    let normalized = value.replace('\\', "/");
    let drive_absolute = normalized.len() >= 3
        && normalized.as_bytes()[0].is_ascii_alphabetic()
        && normalized.as_bytes()[1] == b':'
        && normalized.as_bytes()[2] == b'/';
    let unix_absolute = normalized.starts_with('/') && !normalized.starts_with("//");
    let unc_components = normalized.strip_prefix("//").map(|path| {
        path.split('/')
            .filter(|component| !component.is_empty())
            .count()
    });
    let unc_absolute = unc_components.is_some_and(|count| count >= 3);
    let separator_body = if drive_absolute || normalized.starts_with("//") {
        &normalized[2..]
    } else {
        normalized.strip_prefix('/').unwrap_or(&normalized)
    };
    let repeated_separators = separator_body.contains("//");
    let root = normalized == "/"
        || (drive_absolute && normalized[3..].trim_matches('/').is_empty())
        || unc_components.is_some_and(|count| count < 3);

    if value.trim() != value
        || normalized.is_empty()
        || normalized.contains('\0')
        || root
        || repeated_separators
        || !(unix_absolute || drive_absolute || unc_absolute)
    {
        return Err(AgentFieldError::new(field, "invalidAbsolutePath"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom_path(base: CustomPathBase, relative_path: &str) -> CustomPathSpec {
        CustomPathSpec::based(base, relative_path)
    }

    fn custom_absolute(path: &str) -> CustomPathSpec {
        CustomPathSpec::absolute(path)
    }

    fn valid_custom_definition() -> CustomAgentDefinition {
        CustomAgentDefinition {
            id: AgentId::parse("demo-agent").unwrap(),
            display_name: "Demo Agent".to_string(),
            global: CustomScopeDefinition {
                enabled: true,
                location: ScopeLocation::Shared,
                private_path: None,
            },
            project: CustomScopeDefinition {
                enabled: false,
                location: ScopeLocation::Shared,
                private_path: None,
            },
            detection_paths: vec![custom_path(CustomPathBase::Home, ".demo")],
        }
    }

    #[test]
    fn agent_id_is_a_kebab_case_string() {
        let id = AgentId::parse("demo-agent-2").expect("valid ID");
        assert_eq!(id.as_str(), "demo-agent-2");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"demo-agent-2\"");

        for invalid in [
            "",
            "Demo-agent",
            "demo_agent",
            "-demo",
            "demo-",
            "demo--agent",
            "demo agent",
        ] {
            assert!(
                AgentId::parse(invalid).is_err(),
                "{invalid} must be rejected"
            );
        }
    }

    #[test]
    fn scope_location_uses_kebab_case_serialization() {
        assert_eq!(
            serde_json::to_string(&ScopeLocation::Shared).unwrap(),
            "\"shared\""
        );
        assert_eq!(
            serde_json::to_string(&ScopeLocation::Private).unwrap(),
            "\"private\""
        );
        assert_eq!(
            serde_json::to_string(&ScopeLocation::Both).unwrap(),
            "\"both\""
        );
    }

    #[test]
    fn custom_scope_normalizes_both_without_install_mode() {
        let scope = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Both,
            private_path: Some(custom_path(CustomPathBase::Home, ".demo/skills")),
        };

        let normalized = scope.normalize().expect("valid scope");

        assert!(normalized.reads_shared);
        assert!(normalized.private_path.is_some());
    }

    #[test]
    fn custom_scope_requires_private_path_for_private_and_both() {
        for location in [ScopeLocation::Private, ScopeLocation::Both] {
            let scope = CustomScopeDefinition {
                enabled: true,
                location,
                private_path: None,
            };
            assert_eq!(scope.normalize().unwrap_err().field, "privatePath");
        }
    }

    #[test]
    fn custom_scope_rejects_private_path_for_shared() {
        let scope = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Shared,
            private_path: Some(custom_path(CustomPathBase::Home, ".demo/skills")),
        };

        assert_eq!(scope.normalize().unwrap_err().field, "privatePath");
    }

    #[test]
    fn custom_definition_requires_detection_paths() {
        let mut definition = valid_custom_definition();
        definition.detection_paths.clear();

        assert_eq!(definition.validate().unwrap_err().field, "detectionPaths");
    }

    #[test]
    fn custom_definition_requires_an_enabled_scope() {
        let mut definition = valid_custom_definition();
        definition.global.enabled = false;

        assert_eq!(definition.validate().unwrap_err().field, "scopes");
    }

    #[test]
    fn global_private_path_rejects_project_base_with_stable_field_path() {
        let mut definition = valid_custom_definition();
        definition.global = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Private,
            private_path: Some(custom_path(CustomPathBase::Project, ".demo/skills")),
        };

        assert_eq!(
            definition.validate().unwrap_err().field,
            "global.privatePath.base"
        );
    }

    #[test]
    fn runtime_definition_rejects_global_project_private_path() {
        let definition = AgentDefinition {
            id: AgentId::parse("demo-agent").unwrap(),
            display_name: "Demo Agent".to_string(),
            source: AgentSource::Custom,
            aliases: Vec::new(),
            global: ScopeDefinition {
                enabled: true,
                reads_shared: false,
                private_path: Some(PathSpec::project(".demo/skills")),
            },
            project: ScopeDefinition {
                enabled: true,
                reads_shared: true,
                private_path: None,
            },
            detection: DetectionSpec::AnyPathExists {
                paths: vec![PathSpec::home(".demo")],
            },
            legacy_paths: Vec::new(),
            adapter: AgentAdapter::Standard,
        };

        assert_eq!(
            definition.validate().unwrap_err().field,
            "global.privatePath.base"
        );
    }

    #[test]
    fn runtime_definition_recursively_rejects_project_paths_in_global_private_path() {
        let nested_paths = [
            PathSpec::EnvironmentVariable {
                name: "DEMO_HOME".to_string(),
                relative_path: "skills".to_string(),
                fallback: Box::new(PathSpec::project(".demo/skills")),
            },
            PathSpec::FirstExisting {
                candidates: vec![PathSpec::home(".demo/skills"), PathSpec::project("skills")],
                fallback: Box::new(PathSpec::home(".demo/skills")),
            },
            PathSpec::FirstExisting {
                candidates: vec![PathSpec::home(".demo/skills")],
                fallback: Box::new(PathSpec::project("skills")),
            },
        ];

        for private_path in nested_paths {
            let definition = AgentDefinition {
                id: AgentId::parse("demo-agent").unwrap(),
                display_name: "Demo Agent".to_string(),
                source: AgentSource::Custom,
                aliases: Vec::new(),
                global: ScopeDefinition {
                    enabled: true,
                    reads_shared: false,
                    private_path: Some(private_path),
                },
                project: ScopeDefinition {
                    enabled: false,
                    reads_shared: false,
                    private_path: None,
                },
                detection: DetectionSpec::AnyPathExists {
                    paths: vec![PathSpec::home(".demo")],
                },
                legacy_paths: Vec::new(),
                adapter: AgentAdapter::Standard,
            };

            assert_eq!(
                definition.validate().unwrap_err().field,
                "global.privatePath.base"
            );
        }
    }

    #[test]
    fn custom_absolute_paths_are_allowed_for_detection_and_global_private_scope() {
        let mut definition = valid_custom_definition();
        definition.global = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Private,
            private_path: Some(custom_absolute("/opt/demo/skills")),
        };
        definition.detection_paths = vec![custom_absolute("C:\\Users\\alice\\.demo")];

        let normalized = definition.normalize().expect("valid absolute paths");

        assert_eq!(
            normalized.global.private_path,
            Some(PathSpec::absolute("/opt/demo/skills"))
        );
        assert_eq!(
            normalized.detection,
            DetectionSpec::AnyPathExists {
                paths: vec![PathSpec::absolute("C:\\Users\\alice\\.demo")]
            }
        );
    }

    #[test]
    fn custom_project_private_scope_rejects_absolute_path() {
        let mut definition = valid_custom_definition();
        definition.project = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Private,
            private_path: Some(custom_absolute("/work/demo/.agent/skills")),
        };

        assert_eq!(
            definition.validate().unwrap_err().field,
            "project.privatePath.kind"
        );
    }

    #[test]
    fn runtime_project_private_scope_rejects_absolute_path() {
        let definition = AgentDefinition {
            id: AgentId::parse("demo-agent").unwrap(),
            display_name: "Demo Agent".to_string(),
            source: AgentSource::Custom,
            aliases: Vec::new(),
            global: ScopeDefinition {
                enabled: true,
                reads_shared: true,
                private_path: None,
            },
            project: ScopeDefinition {
                enabled: true,
                reads_shared: false,
                private_path: Some(PathSpec::absolute("/work/demo/.agent/skills")),
            },
            detection: DetectionSpec::AnyPathExists {
                paths: vec![PathSpec::absolute("/opt/demo")],
            },
            legacy_paths: Vec::new(),
            adapter: AgentAdapter::Standard,
        };

        assert_eq!(
            definition.validate().unwrap_err().field,
            "project.privatePath.kind"
        );
    }

    #[test]
    fn absolute_paths_reject_whitespace_roots_and_repeated_separators() {
        for invalid in [
            "",
            " /opt/demo",
            "/opt/demo ",
            "/",
            "/opt//demo",
            "C:/",
            "C://",
            "C:\\",
            "C:\\\\",
            "C://Users/alice",
            "\\\\server\\share",
            "\\\\server\\\\share\\folder",
        ] {
            let mut definition = valid_custom_definition();
            definition.detection_paths = vec![custom_absolute(invalid)];
            let error = definition
                .validate()
                .expect_err(&format!("{invalid} should be rejected"));
            assert_eq!(error.field, "detectionPaths[0].path");
        }
    }

    #[test]
    fn absolute_paths_preserve_valid_posix_windows_and_unc_forms() {
        for valid in [
            "/opt/demo",
            "C:/Users/alice/.demo",
            "C:\\Users\\alice\\.demo",
            "\\\\server\\share\\folder",
        ] {
            let mut definition = valid_custom_definition();
            definition.detection_paths = vec![custom_absolute(valid)];

            let normalized = definition.normalize().expect("valid absolute path");
            assert_eq!(
                normalized.detection,
                DetectionSpec::AnyPathExists {
                    paths: vec![PathSpec::absolute(valid)]
                }
            );
        }
    }

    #[test]
    fn custom_project_detection_is_valid() {
        let mut definition = valid_custom_definition();
        definition.detection_paths =
            vec![CustomPathSpec::based(CustomPathBase::Project, ".my-agent")];

        definition
            .validate()
            .expect("project detection is supported");
    }

    #[test]
    fn custom_definition_round_trips_project_detection_paths() {
        let mut definition = valid_custom_definition();
        definition.detection_paths =
            vec![CustomPathSpec::based(CustomPathBase::Project, ".my-agent")];

        let encoded = serde_json::to_string(&definition).expect("serialize definition");
        let decoded: CustomAgentDefinition =
            serde_json::from_str(&encoded).expect("deserialize definition");

        assert_eq!(decoded.detection_paths, definition.detection_paths);
    }

    #[test]
    fn relative_paths_reject_roots_absolute_paths_and_traversal() {
        for invalid in [
            "",
            ".",
            "..",
            "../skills",
            "/skills",
            "C:\\skills",
            "foo/../../bar",
        ] {
            let mut definition = valid_custom_definition();
            definition.detection_paths = vec![custom_path(CustomPathBase::Home, invalid)];
            assert_eq!(
                definition.validate().unwrap_err().field,
                "detectionPaths[0].relativePath",
                "{invalid} must be rejected"
            );
        }
    }

    #[test]
    fn normalization_deduplicates_detection_paths() {
        let mut definition = valid_custom_definition();
        definition
            .detection_paths
            .push(custom_path(CustomPathBase::Home, ".demo"));

        let normalized = definition.normalize().expect("valid definition");
        let DetectionSpec::AnyPathExists { paths } = normalized.detection else {
            panic!("custom definitions must use any-path detection");
        };
        assert_eq!(paths.len(), 1);
    }
}
