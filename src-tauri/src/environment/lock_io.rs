use std::fs;
use std::io::Write;
use std::path::Path;

use tempfile::NamedTempFile;
use tokio::time::Duration;

use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::run_wsl_script;
use crate::error::AppError;

pub enum EnvironmentLockIo {
    Host,
    Wsl(WslSession),
}

impl EnvironmentLockIo {
    pub async fn read_optional(
        &self,
        locator: &ResourceLocator,
    ) -> Result<Option<Vec<u8>>, AppError> {
        match self {
            Self::Host => {
                let path = Path::new(&locator.native_path);
                if !path.exists() {
                    return Ok(None);
                }
                Ok(Some(fs::read(path)?))
            }
            Self::Wsl(session) => {
                ensure_wsl_locator(locator, &session.distro_name)?;
                const SCRIPT: &str =
                    r#"if [ -f "$1" ]; then printf '1'; cat -- "$1"; else printf '0'; fi"#;
                let output = run_wsl_script(
                    session,
                    SCRIPT,
                    std::slice::from_ref(&locator.native_path),
                    Vec::new(),
                    Duration::from_secs(10),
                )
                .await?;
                match output.split_first() {
                    Some((b'0', [])) => Ok(None),
                    Some((b'1', rest)) => Ok(Some(rest.to_vec())),
                    _ => Err(AppError::Custom {
                        message: "invalid optional lock response".to_string(),
                    }),
                }
            }
        }
    }

    #[cfg(any(target_os = "windows", test))]
    pub async fn read(&self, locator: &ResourceLocator) -> Result<Vec<u8>, AppError> {
        match self {
            Self::Host => Ok(fs::read(&locator.native_path)?),
            Self::Wsl(session) => {
                ensure_wsl_locator(locator, &session.distro_name)?;
                const SCRIPT: &str = r#"cat -- "$1""#;
                run_wsl_script(
                    session,
                    SCRIPT,
                    std::slice::from_ref(&locator.native_path),
                    Vec::new(),
                    Duration::from_secs(10),
                )
                .await
            }
        }
    }

    pub async fn write_atomic(
        &self,
        locator: &ResourceLocator,
        bytes: Vec<u8>,
    ) -> Result<(), AppError> {
        match self {
            Self::Host => write_host_atomic(Path::new(&locator.native_path), &bytes),
            Self::Wsl(session) => {
                ensure_wsl_locator(locator, &session.distro_name)?;
                const SCRIPT: &str = r#"path=$1; dir=${path%/*}; mkdir -p -- "$dir"; tmp=$(mktemp "$dir/.lock.XXXXXX"); trap 'rm -f -- "$tmp"' EXIT HUP INT TERM; cat > "$tmp"; sync "$tmp" 2>/dev/null || true; mv -f -- "$tmp" "$path"; trap - EXIT HUP INT TERM"#;
                run_wsl_script(
                    session,
                    SCRIPT,
                    std::slice::from_ref(&locator.native_path),
                    bytes,
                    Duration::from_secs(10),
                )
                .await?;
                Ok(())
            }
        }
    }
}

pub(crate) fn write_host_atomic(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn ensure_wsl_locator(locator: &ResourceLocator, distro_name: &str) -> Result<(), AppError> {
    match &locator.environment {
        EnvironmentRef::Wsl {
            distro_name: locator_distro,
        } if locator_distro == distro_name => Ok(()),
        _ => Err(AppError::Path {
            message: "lock locator does not belong to the WSL session".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::EnvironmentLockIo;
    use crate::environment::types::{EnvironmentRef, ResourceLocator};

    #[tokio::test]
    async fn host_lock_io_round_trips_bytes_atomically() {
        let temp = tempdir().expect("tempdir");
        let locator = ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: temp
                .path()
                .join("state/lock.json")
                .to_string_lossy()
                .to_string(),
        };
        let io = EnvironmentLockIo::Host;

        io.write_atomic(&locator, br#"{"skills":{}}\n"#.to_vec())
            .await
            .expect("write lock");

        assert_eq!(
            io.read(&locator).await.expect("read lock"),
            br#"{"skills":{}}\n"#
        );
    }

    #[tokio::test]
    async fn host_optional_read_distinguishes_missing_lock_from_empty_bytes() {
        let temp = tempdir().expect("tempdir");
        let locator = ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: temp
                .path()
                .join("state/lock.json")
                .to_string_lossy()
                .to_string(),
        };
        let io = EnvironmentLockIo::Host;

        assert_eq!(
            io.read_optional(&locator).await.expect("missing lock"),
            None
        );

        io.write_atomic(&locator, Vec::new())
            .await
            .expect("write empty lock");
        assert_eq!(
            io.read_optional(&locator).await.expect("empty lock"),
            Some(Vec::new())
        );
    }
}
