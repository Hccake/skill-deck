use tokio::time::Duration;

use crate::core::mutation::CancellationSignal;
use crate::environment::content_manifest::{
    ContentManifest, ContentManifestRecord, ContentManifestTarget,
};
use crate::environment::runtime::ExecutionBackend;
use crate::environment::types::{normalized_wsl_distro_name, EnvironmentRef};
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    wsl_operation, WslOperationDescriptor, WslOperationExecutor, WslOperationRequest,
    DEFAULT_WSL_STDERR_LIMIT,
};
use crate::error::AppError;

const PROTOCOL_HEADER: &[u8] = b"SDCM 1\n";
pub(crate) const CONTENT_MANIFEST_SCRIPT: &str = include_str!("../scripts/content-manifest.sh");
const CONTENT_MANIFEST_OPERATION: WslOperationDescriptor =
    wsl_operation("content-manifest", "inspect", CONTENT_MANIFEST_SCRIPT);

pub async fn inspect(
    session: &WslSession,
    target: &ContentManifestTarget,
    cancellation: Option<CancellationSignal>,
) -> Result<ContentManifest, AppError> {
    let expected_environment = EnvironmentRef::Wsl {
        distro_name: session.distro_name.clone(),
    };
    if target.location.environment != expected_environment
        || !matches!(
            &target.key.backend,
            ExecutionBackend::WslPosix { distro_name }
                if distro_name == &normalized_wsl_distro_name(&session.distro_name)
        )
        || !target.location.native_path.starts_with('/')
    {
        return Err(AppError::StorageUnsupported {
            path: target.location.native_path.clone(),
        });
    }
    inspect_path(session, &target.location.native_path, cancellation).await
}

pub(crate) async fn inspect_path(
    session: &WslSession,
    path: &str,
    cancellation: Option<CancellationSignal>,
) -> Result<ContentManifest, AppError> {
    if !path.starts_with('/') {
        return Err(AppError::StorageUnsupported {
            path: path.to_string(),
        });
    }
    let output = WslOperationExecutor::execute(
        &CONTENT_MANIFEST_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args: vec![path.to_string()],
            stdin: Vec::new(),
            timeout: Duration::from_secs(60),
            stdout_limit: 32 * 1024 * 1024,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation,
        },
    )
    .await?;
    parse_content_manifest(&output.stdout)
}

pub fn parse_content_manifest(bytes: &[u8]) -> Result<ContentManifest, AppError> {
    if !bytes.starts_with(PROTOCOL_HEADER) {
        return Err(protocol_error());
    }
    let mut cursor = PROTOCOL_HEADER.len();
    let mut records = Vec::new();
    loop {
        let header = next_line(bytes, &mut cursor)?;
        let header = std::str::from_utf8(header).map_err(|_| protocol_error())?;
        if let Some(count) = header.strip_prefix("E ") {
            if cursor != bytes.len()
                || count.parse::<usize>().map_err(|_| protocol_error())? != records.len()
            {
                return Err(protocol_error());
            }
            return ContentManifest::from_records(records);
        }
        let fields = header.split(' ').collect::<Vec<_>>();
        if fields.len() != 5 || fields[0] != "R" {
            return Err(protocol_error());
        }
        let executable = match fields[2] {
            "0" => false,
            "1" => true,
            _ => return Err(protocol_error()),
        };
        let path_length = fields[3].parse::<usize>().map_err(|_| protocol_error())?;
        let data_length = fields[4].parse::<usize>().map_err(|_| protocol_error())?;
        let record_end = cursor
            .checked_add(path_length)
            .and_then(|value| value.checked_add(data_length))
            .filter(|end| *end <= bytes.len())
            .ok_or_else(protocol_error)?;
        let path_end = cursor + path_length;
        let path = std::str::from_utf8(&bytes[cursor..path_end]).map_err(|_| protocol_error())?;
        let data =
            std::str::from_utf8(&bytes[path_end..record_end]).map_err(|_| protocol_error())?;
        cursor = record_end;
        let record = match fields[1] {
            "d" if !executable && data.is_empty() => ContentManifestRecord::directory(path),
            "f" => ContentManifestRecord::file(path, data, executable),
            "l" if !executable => ContentManifestRecord::symlink(path, data),
            _ => return Err(protocol_error()),
        }
        .map_err(|_| protocol_error())?;
        records.push(record);
    }
}

fn next_line<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], AppError> {
    let end = bytes[*cursor..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|offset| *cursor + offset)
        .ok_or_else(protocol_error)?;
    let line = &bytes[*cursor..end];
    *cursor = end + 1;
    Ok(line)
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL content manifest protocol response".to_string(),
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "内容清单协议测试需要直接运行待验证的 shell 测试脚本"
)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::fs;
    #[cfg(target_os = "linux")]
    use std::process::Command;

    use crate::environment::content_manifest::{ContentManifest, ContentManifestRecord};

    #[cfg(target_os = "linux")]
    use super::CONTENT_MANIFEST_SCRIPT;
    use super::{parse_content_manifest, CONTENT_MANIFEST_OPERATION};

    fn fixture_bytes(records: &[(&str, &str, bool, &str)]) -> Vec<u8> {
        let mut bytes = b"SDCM 1\n".to_vec();
        for (kind, path, executable, data) in records {
            bytes.extend_from_slice(
                format!(
                    "R {kind} {} {} {}\n",
                    u8::from(*executable),
                    path.len(),
                    data.len()
                )
                .as_bytes(),
            );
            bytes.extend_from_slice(path.as_bytes());
            bytes.extend_from_slice(data.as_bytes());
        }
        bytes.extend_from_slice(format!("E {}\n", records.len()).as_bytes());
        bytes
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn content_manifest_remains_readable_without_nul_safe_xargs() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temporary content root");
        let content = temp.path().join("skill");
        fs::create_dir_all(&content).expect("content directory");
        fs::write(content.join("SKILL.md"), "# Demo\n").expect("skill content");

        let commands = temp.path().join("commands");
        fs::create_dir_all(&commands).expect("command directory");
        let xargs = commands.join("xargs");
        fs::write(&xargs, "#!/bin/sh\nexit 1\n").expect("failing xargs");
        fs::set_permissions(&xargs, fs::Permissions::from_mode(0o755)).expect("xargs permissions");

        let path = format!(
            "{}:{}",
            commands.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(CONTENT_MANIFEST_SCRIPT)
            .arg("--")
            .arg("inspect")
            .arg(&content)
            .env("PATH", path)
            .output()
            .expect("content manifest script");

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let manifest = parse_content_manifest(&output.stdout).expect("content manifest");
        assert_eq!(manifest.records().len(), 1);
        assert_eq!(CONTENT_MANIFEST_OPERATION.subcommand, "inspect");
    }

    #[test]
    fn wsl_records_use_the_same_rust_aggregate_hash_as_native_records() {
        let digest = "a".repeat(64);
        let bytes = fixture_bytes(&[
            ("l", "current", false, "run.sh"),
            ("d", "empty", false, ""),
            ("f", "run.sh", true, &digest),
        ]);
        let parsed = parse_content_manifest(&bytes).unwrap();
        let native = ContentManifest::from_records(vec![
            ContentManifestRecord::directory("empty").unwrap(),
            ContentManifestRecord::file("run.sh", digest, true).unwrap(),
            ContentManifestRecord::symlink("current", "run.sh").unwrap(),
        ])
        .unwrap();

        assert_eq!(parsed.hash(), native.hash());
    }

    #[test]
    fn wsl_parser_rejects_unknown_versions_malformed_lengths_and_truncation() {
        let digest = "a".repeat(64);
        let valid = fixture_bytes(&[("f", "SKILL.md", false, &digest)]);
        let mut wrong_version = valid.clone();
        wrong_version[5] = b'2';
        let malformed_length = b"SDCM 1\nR f 0 nope 64\n";
        let truncated = &valid[..valid.len() - 3];

        assert!(parse_content_manifest(&wrong_version).is_err());
        assert!(parse_content_manifest(malformed_length).is_err());
        assert!(parse_content_manifest(truncated).is_err());
    }

    #[test]
    fn wsl_parser_rejects_unsafe_relative_paths_and_record_count_mismatch() {
        let digest = "a".repeat(64);
        let unsafe_path = fixture_bytes(&[("f", "../SKILL.md", false, &digest)]);
        let mut wrong_count = fixture_bytes(&[("f", "SKILL.md", false, &digest)]);
        let footer = wrong_count.len() - 4;
        wrong_count[footer..].copy_from_slice(b"E 2\n");

        assert!(parse_content_manifest(&unsafe_path).is_err());
        assert!(parse_content_manifest(&wrong_count).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn wsl_script_emits_records_only_and_rust_computes_the_manifest_hash() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skill");
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("run.sh"), b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(root.join("run.sh")).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(root.join("run.sh"), permissions).unwrap();
        symlink("run.sh", root.join("current")).unwrap();
        fs::write(root.join("target\n"), b"target").unwrap();
        symlink("target\n", root.join("newline-target")).unwrap();

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(CONTENT_MANIFEST_SCRIPT)
            .arg("--")
            .arg("inspect")
            .arg(&root)
            .output()
            .unwrap();

        assert!(output.status.success(), "{:?}", output.stderr);
        let manifest = parse_content_manifest(&output.stdout)
            .unwrap_or_else(|error| panic!("{error:?}: {:?}", output.stdout));
        assert_eq!(manifest.records().len(), 5);
        assert!(manifest.records().iter().any(|record| {
            record.relative_path == "newline-target"
                && record.symlink_target.as_deref() == Some("target\n")
        }));
        assert_eq!(manifest.hash().as_str().len(), 64);
        assert!(!String::from_utf8_lossy(&output.stdout).contains(manifest.hash().as_str()));
    }
}
