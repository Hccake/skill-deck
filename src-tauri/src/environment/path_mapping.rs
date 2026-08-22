use crate::environment::types::EnvironmentRef;
use crate::environment::wsl::operations::path;
use crate::environment::wsl::WslSession;
use crate::error::AppError;

pub(crate) fn parse_wsl_unc_path(path: &str) -> Option<(String, String)> {
    let normalized = path.replace('/', "\\");
    let lower = normalized.to_ascii_lowercase();
    let prefix_len = if lower.starts_with("\\\\wsl.localhost\\") {
        "\\\\wsl.localhost\\".len()
    } else if lower.starts_with("\\\\wsl$\\") {
        "\\\\wsl$\\".len()
    } else {
        return None;
    };
    let without_prefix = &normalized[prefix_len..];
    let (distro, remainder) = without_prefix
        .split_once('\\')
        .unwrap_or((without_prefix, ""));
    Some((
        distro.to_string(),
        normalize_posix_path(&remainder.replace('\\', "/")),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WindowsStorageOwner {
    Windows,
    Wsl { distro_name: String },
    Unknown,
}

pub(crate) fn windows_storage_owner(path: &str) -> WindowsStorageOwner {
    if let Some((distro_name, _)) = parse_wsl_unc_path(path) {
        return WindowsStorageOwner::Wsl { distro_name };
    }
    let normalized = path.trim().replace('/', "\\");
    let bytes = normalized.as_bytes();
    if bytes.len() >= 3 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() && bytes[2] == b'\\' {
        return WindowsStorageOwner::Windows;
    }
    if let Some(remainder) = normalized.strip_prefix("\\\\") {
        let mut components = remainder
            .split('\\')
            .filter(|component| !component.is_empty());
        if components.next().is_some() && components.next().is_some() {
            return WindowsStorageOwner::Windows;
        }
    }
    WindowsStorageOwner::Unknown
}

fn normalize_posix_path(path: &str) -> String {
    let rooted = path.starts_with('/');
    let mut components: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." if components.last().is_some_and(|last| *last != "..") => {
                components.pop();
            }
            ".." if !rooted => components.push(component),
            ".." => {}
            _ => components.push(component),
        }
    }
    let joined = components.join("/");
    if rooted {
        if joined.is_empty() {
            "/".to_string()
        } else {
            format!("/{joined}")
        }
    } else if joined.is_empty() {
        "/".to_string()
    } else {
        format!("/{joined}")
    }
}

pub fn map_wsl_input_without_wslpath(
    distro_name: &str,
    path: &str,
) -> Result<Option<String>, AppError> {
    if path.trim().starts_with('/') {
        return Ok(Some(normalize_posix_path(path.trim())));
    }
    if let Some((owner_distro, linux_path)) = parse_wsl_unc_path(path) {
        if owner_distro.eq_ignore_ascii_case(distro_name) {
            return Ok(Some(linux_path));
        }
        return Err(AppError::StorageMappingUnsupported {
            path: path.to_string(),
            environment: EnvironmentRef::Wsl {
                distro_name: distro_name.to_string(),
            },
        });
    }
    Ok(None)
}

pub async fn map_windows_path_with_wslpath(
    session: &WslSession,
    path: &str,
) -> Result<String, AppError> {
    match path::map_host_bridge_path(session, path, None).await {
        Ok(mapped) => Ok(mapped),
        Err(AppError::WslCommandFailed { .. }) => Err(AppError::StorageMappingUnsupported {
            path: path.to_string(),
            environment: EnvironmentRef::Wsl {
                distro_name: session.distro_name.clone(),
            },
        }),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{map_wsl_input_without_wslpath, windows_storage_owner, WindowsStorageOwner};
    use crate::environment::types::EnvironmentRef;
    use crate::error::AppError;

    #[test]
    fn recognizes_verbatim_windows_drive_paths_as_windows_storage() {
        assert_eq!(
            windows_storage_owner(r"\\?\C:\Users\alice\project"),
            WindowsStorageOwner::Windows
        );
    }

    #[test]
    fn maps_native_and_current_distro_inputs_without_wslpath() {
        assert_eq!(
            map_wsl_input_without_wslpath("Ubuntu", "/work/./app/").expect("native input"),
            Some("/work/app".to_string())
        );
        assert_eq!(
            map_wsl_input_without_wslpath("Ubuntu", r"\\wsl.localhost\Ubuntu\home\alice\项目",)
                .expect("current distro UNC"),
            Some("/home/alice/项目".to_string())
        );
    }

    #[test]
    fn windows_inputs_require_wslpath_and_other_distro_unc_is_rejected() {
        assert_eq!(
            map_wsl_input_without_wslpath("Ubuntu", r"C:\Code\app").expect("drive input"),
            None
        );
        assert_eq!(
            map_wsl_input_without_wslpath("Ubuntu", r"\\server\share\app")
                .expect("network UNC input"),
            None
        );
        assert!(matches!(
            map_wsl_input_without_wslpath(
                "Ubuntu",
                r"\\wsl.localhost\Debian\home\alice\app",
            ),
            Err(AppError::StorageMappingUnsupported {
                environment: EnvironmentRef::Wsl { distro_name },
                ..
            }) if distro_name == "Ubuntu"
        ));
    }
}
