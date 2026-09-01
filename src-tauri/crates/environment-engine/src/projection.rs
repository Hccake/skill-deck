use std::fmt;
#[cfg(target_os = "linux")]
use std::path::Component;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use std::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionRequest {
    pub destinations: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedTarget {
    pub anchor_device: u64,
    pub anchor_inode: u64,
    pub physical_anchor: PathBuf,
    pub physical_destination: PathBuf,
    pub relative_components: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionResponse {
    pub targets: Vec<ProjectedTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionError {
    UnsupportedPlatform,
    InvalidRequest,
    Unavailable,
    Cancelled,
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ProjectionError {}

pub fn project_targets(request: &ProjectionRequest) -> Result<ProjectionResponse, ProjectionError> {
    project_targets_with_cancel(request, || false)
}

pub fn project_targets_with_cancel<F>(
    request: &ProjectionRequest,
    is_cancelled: F,
) -> Result<ProjectionResponse, ProjectionError>
where
    F: Fn() -> bool,
{
    if request.destinations.is_empty()
        || request.destinations.iter().any(|path| !path.is_absolute())
    {
        return Err(ProjectionError::InvalidRequest);
    }
    project_platform(request, &is_cancelled)
}

#[cfg(not(target_os = "linux"))]
fn project_platform(
    _request: &ProjectionRequest,
    _is_cancelled: &impl Fn() -> bool,
) -> Result<ProjectionResponse, ProjectionError> {
    Err(ProjectionError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn project_platform(
    request: &ProjectionRequest,
    is_cancelled: &impl Fn() -> bool,
) -> Result<ProjectionResponse, ProjectionError> {
    use std::os::unix::fs::MetadataExt;

    let mut targets = Vec::with_capacity(request.destinations.len());
    for destination in &request.destinations {
        if is_cancelled() {
            return Err(ProjectionError::Cancelled);
        }
        let mut parent = destination
            .parent()
            .ok_or(ProjectionError::InvalidRequest)?
            .to_path_buf();
        let name = destination
            .file_name()
            .filter(|name| !name.is_empty())
            .ok_or(ProjectionError::InvalidRequest)?;
        let mut components = vec![PathBuf::from(name)];
        while fs::symlink_metadata(&parent).is_err() {
            let name = parent
                .file_name()
                .filter(|name| !name.is_empty())
                .ok_or(ProjectionError::Unavailable)?;
            components.push(PathBuf::from(name));
            parent = parent
                .parent()
                .ok_or(ProjectionError::Unavailable)?
                .to_path_buf();
        }
        if !fs::metadata(&parent)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
        {
            return Err(ProjectionError::Unavailable);
        }
        components.reverse();
        if components.iter().any(|component| {
            component
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        }) {
            return Err(ProjectionError::InvalidRequest);
        }
        let physical_anchor =
            fs::canonicalize(&parent).map_err(|_| ProjectionError::Unavailable)?;
        let metadata = fs::metadata(&physical_anchor).map_err(|_| ProjectionError::Unavailable)?;
        let physical_destination = components
            .iter()
            .fold(physical_anchor.clone(), |path, component| {
                path.join(component)
            });
        targets.push(ProjectedTarget {
            anchor_device: metadata.dev(),
            anchor_inode: metadata.ino(),
            physical_anchor,
            physical_destination,
            relative_components: components,
        });
    }
    Ok(ProjectionResponse { targets })
}
