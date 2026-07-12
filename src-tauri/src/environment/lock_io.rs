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

fn write_host_atomic(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
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
}
