use std::path::Path;
use std::process::Command;

use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResourceOpener;

/// 按用户明确请求打开外部资源；该进程属于可见的前台交互，不应用后台隐藏策略。
#[allow(
    clippy::disallowed_methods,
    reason = "用户主动打开资源时需要保留外部应用的前台窗口"
)]
pub fn open_authorized_resource(target: &ResourceLocator) -> Result<(), AppError> {
    let (program, path) = open_command(target)?;
    Command::new(program).arg(path).spawn()?;
    Ok(())
}

fn open_command(target: &ResourceLocator) -> Result<(&'static str, String), AppError> {
    match &target.environment {
        EnvironmentRef::Native => {
            if !Path::new(&target.native_path).is_absolute() {
                return Err(unsafe_target(
                    target,
                    "Native resource path is not absolute",
                ));
            }
            #[cfg(target_os = "windows")]
            return Ok(("explorer.exe", target.native_path.clone()));
            #[cfg(target_os = "macos")]
            return Ok(("open", target.native_path.clone()));
            #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
            return Ok(("xdg-open", target.native_path.clone()));
        }
        EnvironmentRef::Wsl { distro_name } => {
            #[cfg(target_os = "windows")]
            {
                Ok((
                    "explorer.exe",
                    wsl_unc_path(distro_name, &target.native_path)?,
                ))
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = distro_name;
                Err(AppError::EnvironmentUnavailable {
                    environment: target.environment.clone(),
                    message: format!(
                        "WSL resource opening is unavailable on {}",
                        std::env::consts::OS
                    ),
                })
            }
        }
    }
}

#[cfg(any(target_os = "windows", test))]
fn wsl_unc_path(distro_name: &str, path: &str) -> Result<String, AppError> {
    if distro_name.is_empty()
        || distro_name.contains(['/', '\\', '\0'])
        || !path.starts_with('/')
        || path.contains('\0')
    {
        return Err(AppError::UnsafePath {
            path: path.to_string(),
            reason: "invalid WSL resource identity".to_string(),
        });
    }
    Ok(format!(
        "\\\\wsl.localhost\\{}\\{}",
        distro_name,
        path.trim_start_matches('/').replace('/', "\\")
    ))
}

fn unsafe_target(target: &ResourceLocator, reason: &str) -> AppError {
    AppError::UnsafePath {
        path: target.native_path.clone(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_resource_path_is_projected_to_a_distro_scoped_unc_path() {
        assert_eq!(
            wsl_unc_path("Ubuntu-24.04", "/home/alice/project").unwrap(),
            r"\\wsl.localhost\Ubuntu-24.04\home\alice\project"
        );
        assert!(wsl_unc_path("../Ubuntu", "/home/alice").is_err());
        assert!(wsl_unc_path("Ubuntu", "relative/path").is_err());
    }
}
