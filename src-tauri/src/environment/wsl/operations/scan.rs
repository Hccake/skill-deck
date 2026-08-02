use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use tokio::time::Duration;

use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::protocol::{
    wsl_operation, WslOperationDescriptor, WslOperationExecutor, WslOperationRequest,
    DEFAULT_WSL_STDERR_LIMIT,
};
use crate::environment::wsl::WslSession;
use crate::error::AppError;

const PROTOCOL_VERSION: &str = "2";
pub(crate) const SCAN_SCRIPT: &str = include_str!("../scripts/scan.sh");
const SCAN_OPERATION: WslOperationDescriptor = wsl_operation("scan", "scan", SCAN_SCRIPT);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRequest {
    pub roots: Vec<String>,
    #[serde(default)]
    pub stat_only_root_indexes: BTreeSet<u32>,
    #[serde(default)]
    pub recursive: bool,
    pub per_file_limit: u32,
    pub aggregate_limit: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScannedEntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    Other,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedEntry {
    pub root_index: u32,
    pub relative_path: String,
    pub kind: ScannedEntryKind,
    pub resolved_target: Option<String>,
    pub size: u64,
    pub mode: u32,
    pub modified_seconds: i64,
    pub content_bytes: Vec<u8>,
    pub truncated: bool,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResponse {
    pub entries: Vec<ScannedEntry>,
    pub root_count: u32,
    pub total_content_bytes: u32,
}

pub async fn scan(
    session: &WslSession,
    request: ScanRequest,
    cancellation: Option<CancellationSignal>,
) -> Result<ScanResponse, AppError> {
    let mode = if request.recursive { "1" } else { "0" };
    execute_scan(session, request, mode, cancellation).await
}

/// 扫描 plugin manifest 声明目录的直接子 Skill。
///
/// 该 mode 不改变通用 ScanRequest，也不会回传目录中的普通 payload metadata。
pub async fn scan_priority_directories(
    session: &WslSession,
    request: ScanRequest,
    cancellation: Option<CancellationSignal>,
) -> Result<ScanResponse, AppError> {
    if request.recursive {
        return Err(AppError::Validation {
            field: Some("scanRequest.recursive".to_string()),
            message: "priority directory scan must not enable recursive mode".to_string(),
        });
    }
    execute_scan(session, request, "2", cancellation).await
}

async fn execute_scan(
    session: &WslSession,
    request: ScanRequest,
    mode: &str,
    cancellation: Option<CancellationSignal>,
) -> Result<ScanResponse, AppError> {
    validate_request(&request)?;
    let mut args = vec![
        request.per_file_limit.to_string(),
        request.aggregate_limit.to_string(),
        stat_only_indexes_arg(&request.stat_only_root_indexes),
        mode.to_string(),
    ];
    args.extend(request.roots.iter().cloned());
    let metadata_allowance = 4usize * 1024 * 1024;
    let stdout_limit = usize::try_from(request.aggregate_limit)
        .unwrap_or(usize::MAX)
        .saturating_add(metadata_allowance);
    let output = WslOperationExecutor::execute(
        &SCAN_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args,
            stdin: Vec::new(),
            timeout: Duration::from_secs(30),
            stdout_limit,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation,
        },
    )
    .await?;
    parse_scan_response(&output.stdout, &request)
}

pub fn parse_scan_response(bytes: &[u8], request: &ScanRequest) -> Result<ScanResponse, AppError> {
    validate_request(request)?;
    let mut cursor = 0;
    if read_text_field(bytes, &mut cursor)? != PROTOCOL_VERSION {
        return Err(protocol_error("unsupported scan protocol version"));
    }
    let mut entries = Vec::new();
    let mut total_content_bytes = 0usize;
    while cursor < bytes.len() {
        if read_text_field(bytes, &mut cursor)? != "E" {
            return Err(protocol_error("invalid scan record tag"));
        }
        let root_index = parse_field::<u32>(bytes, &mut cursor, "root index")?;
        if usize::try_from(root_index).unwrap_or(usize::MAX) >= request.roots.len() {
            return Err(protocol_error("scan root index is out of range"));
        }
        let relative_path = read_text_field(bytes, &mut cursor)?.to_string();
        validate_relative_path(&relative_path)?;
        let kind = match read_text_field(bytes, &mut cursor)? {
            "missing" => ScannedEntryKind::Missing,
            "file" => ScannedEntryKind::File,
            "directory" => ScannedEntryKind::Directory,
            "symlink" => ScannedEntryKind::Symlink,
            "other" => ScannedEntryKind::Other,
            "error" => ScannedEntryKind::Error,
            _ => return Err(protocol_error("invalid scanned entry kind")),
        };
        let target = read_text_field(bytes, &mut cursor)?;
        let resolved_target = (!target.is_empty()).then(|| target.to_string());
        let size = parse_field::<u64>(bytes, &mut cursor, "entry size")?;
        let mode = parse_field::<u32>(bytes, &mut cursor, "entry mode")?;
        let modified_seconds = parse_field::<i64>(bytes, &mut cursor, "entry modification time")?;
        let truncated = match read_text_field(bytes, &mut cursor)? {
            "0" => false,
            "1" => true,
            _ => return Err(protocol_error("invalid truncation flag")),
        };
        let error = read_text_field(bytes, &mut cursor)?;
        let error_code = (!error.is_empty()).then(|| error.to_string());
        let content_len = parse_field::<usize>(bytes, &mut cursor, "content length")?;
        if content_len > request.per_file_limit as usize
            || total_content_bytes.saturating_add(content_len) > request.aggregate_limit as usize
            || cursor.saturating_add(content_len) >= bytes.len()
        {
            return Err(protocol_error("scan content length exceeds its boundary"));
        }
        let content_bytes = bytes[cursor..cursor + content_len].to_vec();
        cursor += content_len;
        if bytes.get(cursor) != Some(&0) {
            return Err(protocol_error("scan content terminator is missing"));
        }
        cursor += 1;
        total_content_bytes += content_len;
        entries.push(ScannedEntry {
            root_index,
            relative_path,
            kind,
            resolved_target,
            size,
            mode,
            modified_seconds,
            content_bytes,
            truncated,
            error_code,
        });
    }
    Ok(ScanResponse {
        entries,
        root_count: request.roots.len() as u32,
        total_content_bytes: total_content_bytes as u32,
    })
}

fn validate_request(request: &ScanRequest) -> Result<(), AppError> {
    if request.roots.is_empty()
        || request.per_file_limit == 0
        || request.aggregate_limit == 0
        || request.per_file_limit > request.aggregate_limit
        || request
            .stat_only_root_indexes
            .iter()
            .any(|index| *index as usize >= request.roots.len())
    {
        return Err(AppError::Validation {
            field: Some("scanRequest".to_string()),
            message: "invalid bounded scan request".to_string(),
        });
    }
    Ok(())
}

fn stat_only_indexes_arg(indexes: &BTreeSet<u32>) -> String {
    indexes
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn read_text_field<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a str, AppError> {
    let remaining = bytes
        .get(*cursor..)
        .ok_or_else(|| protocol_error("scan cursor is out of range"))?;
    let length = remaining
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| protocol_error("scan field terminator is missing"))?;
    let field = std::str::from_utf8(&remaining[..length])
        .map_err(|_| protocol_error("scan field is not UTF-8"))?;
    *cursor += length + 1;
    Ok(field)
}

fn parse_field<T>(bytes: &[u8], cursor: &mut usize, name: &str) -> Result<T, AppError>
where
    T: std::str::FromStr,
{
    read_text_field(bytes, cursor)?
        .parse()
        .map_err(|_| protocol_error(&format!("invalid {name}")))
}

fn validate_relative_path(path: &str) -> Result<(), AppError> {
    if path.starts_with('/')
        || path.split('/').any(|component| component == "..")
        || path.contains('\\')
    {
        return Err(protocol_error("unsafe relative path in scan response"));
    }
    Ok(())
}

fn protocol_error(message: &str) -> AppError {
    AppError::ConfigurationCorrupted {
        message: message.to_string(),
    }
}

#[cfg(all(test, target_os = "linux"))]
#[allow(
    clippy::disallowed_methods,
    reason = "扫描协议测试需要直接运行待验证的 shell 测试脚本"
)]
mod tests {
    use std::fs;
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;

    fn run_script_with_mode(request: &ScanRequest, mode: &str) -> ScanResponse {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(SCAN_SCRIPT)
            .arg("--")
            .arg("scan")
            .arg(request.per_file_limit.to_string())
            .arg(request.aggregate_limit.to_string())
            .arg(stat_only_indexes_arg(&request.stat_only_root_indexes))
            .arg(mode)
            .args(&request.roots);
        let output = command.output().expect("run scan script");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        parse_scan_response(&output.stdout, request).expect("parse")
    }

    fn run_script(request: &ScanRequest) -> ScanResponse {
        run_script_with_mode(request, if request.recursive { "1" } else { "0" })
    }

    #[test]
    fn one_batch_isolates_missing_root_and_reads_binary_skill_content() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("skills");
        fs::create_dir_all(root.join("demo")).expect("demo");
        fs::write(root.join("demo/SKILL.md"), [b'a', 0, b'b', b'c']).expect("skill");
        let request = ScanRequest {
            roots: vec![
                temp.path().join("missing").to_string_lossy().into_owned(),
                root.to_string_lossy().into_owned(),
            ],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: false,
            per_file_limit: 16,
            aggregate_limit: 32,
        };

        let response = run_script(&request);

        assert_eq!(response.root_count, 2);
        assert!(response
            .entries
            .iter()
            .any(|entry| { entry.root_index == 0 && entry.kind == ScannedEntryKind::Missing }));
        let skill = response
            .entries
            .iter()
            .find(|entry| entry.root_index == 1 && entry.relative_path == "demo/SKILL.md")
            .expect("skill entry");
        assert_eq!(skill.content_bytes, [b'a', 0, b'b', b'c']);
        assert!(!skill.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn default_scan_reads_skill_document_through_a_direct_child_directory_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("temp");
        let canonical = temp.path().join("canonical/toolkit");
        let agent_root = temp.path().join("agent-skills");
        fs::create_dir_all(&canonical).expect("canonical Skill");
        fs::create_dir_all(&agent_root).expect("Agent Skill directory");
        let document = b"---\nname: toolkit\ndescription: Toolkit\n---\n";
        fs::write(canonical.join("SKILL.md"), document).expect("Skill document");
        symlink(&canonical, agent_root.join("toolkit")).expect("Skill directory link");
        let request = ScanRequest {
            roots: vec![agent_root.to_string_lossy().into_owned()],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: false,
            per_file_limit: 1024,
            aggregate_limit: 4096,
        };

        let response = run_script(&request);

        let linked_directory = response
            .entries
            .iter()
            .find(|entry| entry.relative_path == "toolkit")
            .expect("linked Skill directory");
        assert_eq!(linked_directory.kind, ScannedEntryKind::Symlink);
        let skill_document = response
            .entries
            .iter()
            .find(|entry| entry.relative_path == "toolkit/SKILL.md")
            .expect("Skill document through directory link");
        assert_eq!(skill_document.kind, ScannedEntryKind::File);
        assert_eq!(skill_document.content_bytes, document);
    }

    #[cfg(unix)]
    #[test]
    fn default_scan_does_not_read_through_a_broken_child_directory_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("temp");
        let agent_root = temp.path().join("agent-skills");
        fs::create_dir_all(&agent_root).expect("Agent Skill directory");
        symlink(temp.path().join("missing"), agent_root.join("toolkit"))
            .expect("broken Skill directory link");
        let request = ScanRequest {
            roots: vec![agent_root.to_string_lossy().into_owned()],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: false,
            per_file_limit: 1024,
            aggregate_limit: 4096,
        };

        let response = run_script(&request);

        assert!(response.entries.iter().any(|entry| {
            entry.relative_path == "toolkit" && entry.kind == ScannedEntryKind::Symlink
        }));
        assert!(!response
            .entries
            .iter()
            .any(|entry| entry.relative_path == "toolkit/SKILL.md"));
    }

    #[test]
    fn per_file_and_aggregate_limits_are_enforced_by_the_protocol() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("skills");
        fs::create_dir_all(root.join("first")).expect("first");
        fs::create_dir_all(root.join("second")).expect("second");
        fs::write(root.join("first/SKILL.md"), b"1234567890").expect("first skill");
        fs::write(root.join("second/SKILL.md"), b"abcdefghij").expect("second skill");
        let response = run_script(&ScanRequest {
            roots: vec![root.to_string_lossy().into_owned()],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: false,
            per_file_limit: 6,
            aggregate_limit: 8,
        });
        let contents = response
            .entries
            .iter()
            .filter(|entry| entry.relative_path.ends_with("SKILL.md"))
            .collect::<Vec<_>>();
        assert_eq!(
            contents
                .iter()
                .map(|entry| entry.content_bytes.len())
                .sum::<usize>(),
            8
        );
        assert!(contents.iter().all(|entry| entry.truncated));
        assert_eq!(response.total_content_bytes, 8);
    }

    #[test]
    fn stat_only_root_does_not_enumerate_children_or_consume_content_budget() {
        let temp = tempdir().expect("temp");
        fs::create_dir_all(temp.path().join("unrelated")).expect("unrelated");
        fs::write(
            temp.path().join("unrelated/SKILL.md"),
            b"---\nname: unrelated\ndescription: Unrelated\n---\n",
        )
        .expect("skill");
        let response = run_script(&ScanRequest {
            roots: vec![temp.path().to_string_lossy().into_owned()],
            stat_only_root_indexes: [0].into_iter().collect(),
            recursive: false,
            per_file_limit: 1024,
            aggregate_limit: 4096,
        });

        assert_eq!(response.entries.len(), 1);
        assert_eq!(response.entries[0].relative_path, "");
        assert_eq!(response.total_content_bytes, 0);
    }

    #[test]
    fn recursive_scan_finds_nested_skill_files_without_changing_default_scan() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join("packages/tools/demo")).expect("nested skill");
        fs::write(
            root.join("packages/tools/demo/SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\n",
        )
        .expect("skill");
        let response = run_script(&ScanRequest {
            roots: vec![root.to_string_lossy().into_owned()],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: true,
            per_file_limit: 1024,
            aggregate_limit: 4096,
        });

        assert!(response
            .entries
            .iter()
            .any(|entry| entry.relative_path == "packages/tools/demo/SKILL.md"));
    }

    #[test]
    fn recursive_scan_returns_only_discovery_documents_and_reads_local_lock() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join("skills/demo/scripts")).expect("skill tree");
        fs::create_dir_all(root.join(".claude-plugin")).expect("plugin directory");
        fs::write(
            root.join("skills/demo/SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\n",
        )
        .expect("skill");
        fs::write(root.join("skills/demo/scripts/run.sh"), b"#!/bin/sh\n").expect("script");
        fs::write(
            root.join(".claude-plugin/plugin.json"),
            br#"{"name":"demo","skills":["./skills/demo"]}"#,
        )
        .expect("plugin");
        let lock = br#"{"version":1,"skills":{"demo":{}}}"#;
        fs::write(root.join("skills-lock.json"), lock).expect("lock");

        let response = run_script(&ScanRequest {
            roots: vec![root.to_string_lossy().into_owned()],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: true,
            per_file_limit: 1024,
            aggregate_limit: 4096,
        });

        assert!(!response
            .entries
            .iter()
            .any(|entry| entry.relative_path == "skills/demo/scripts/run.sh"));
        let lock_entry = response
            .entries
            .iter()
            .find(|entry| entry.relative_path == "skills-lock.json")
            .expect("lock entry");
        assert_eq!(lock_entry.content_bytes, lock);
    }

    #[test]
    fn recursive_scan_includes_skill_directory_at_cli_depth_five() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("repo");
        let skill = root.join("one/two/three/four/five");
        fs::create_dir_all(&skill).expect("deep skill");
        fs::write(
            skill.join("SKILL.md"),
            b"---\nname: deep\ndescription: Deep\n---\n",
        )
        .expect("skill");

        let response = run_script(&ScanRequest {
            roots: vec![root.to_string_lossy().into_owned()],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: true,
            per_file_limit: 1024,
            aggregate_limit: 4096,
        });

        assert!(response
            .entries
            .iter()
            .any(|entry| entry.relative_path == "one/two/three/four/five/SKILL.md"));
    }

    #[test]
    fn priority_directory_scan_reads_only_direct_child_skill_documents() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("plugin-catalog");
        fs::create_dir_all(root.join("direct/scripts")).expect("direct skill");
        fs::create_dir_all(root.join("category/nested")).expect("nested skill");
        fs::write(
            root.join("direct/SKILL.md"),
            b"---\nname: direct\ndescription: Direct\n---\n",
        )
        .expect("direct document");
        fs::write(root.join("direct/scripts/run.sh"), b"#!/bin/sh\n").expect("ordinary file");
        fs::write(
            root.join("category/nested/SKILL.md"),
            b"---\nname: nested\ndescription: Nested\n---\n",
        )
        .expect("nested document");
        let request = ScanRequest {
            roots: vec![root.to_string_lossy().into_owned()],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: false,
            per_file_limit: 1024,
            aggregate_limit: 4096,
        };

        let response = run_script_with_mode(&request, "2");

        assert!(response
            .entries
            .iter()
            .any(|entry| entry.relative_path == "direct/SKILL.md"));
        assert!(!response
            .entries
            .iter()
            .any(|entry| entry.relative_path == "direct/scripts"));
        assert!(!response
            .entries
            .iter()
            .any(|entry| entry.relative_path == "category/nested/SKILL.md"));
        assert!(response.entries.iter().all(|entry| {
            entry.relative_path.is_empty()
                || entry
                    .relative_path
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
        }));
    }

    #[cfg(unix)]
    #[test]
    fn recursive_scan_reports_plugin_documents_without_payload_metadata() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("temp");
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join(".claude-plugin")).expect("plugin directory");
        fs::create_dir_all(root.join("skills/demo")).expect("Skill directory");
        let executable = root.join("skills/demo/run.sh");
        fs::write(&executable, b"#!/bin/sh\n").expect("script");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
            .expect("executable mode");
        fs::write(
            root.join("skills/demo/SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\n",
        )
        .expect("Skill");
        let plugin_document = br#"{"name":"demo-plugin","skills":["./skills/demo"]}"#;
        fs::write(root.join(".claude-plugin/plugin.json"), plugin_document).expect("plugin");

        let response = run_script(&ScanRequest {
            roots: vec![root.to_string_lossy().into_owned()],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: true,
            per_file_limit: 1024,
            aggregate_limit: 4096,
        });

        assert!(!response
            .entries
            .iter()
            .any(|entry| entry.relative_path == "skills/demo/run.sh"));
        let plugin = response
            .entries
            .iter()
            .find(|entry| entry.relative_path == ".claude-plugin/plugin.json")
            .expect("plugin manifest entry");
        assert_eq!(plugin.content_bytes, plugin_document);
    }

    #[test]
    fn parser_rejects_unknown_protocol_and_truncated_content() {
        let request = ScanRequest {
            roots: vec!["/tmp".to_string()],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: false,
            per_file_limit: 10,
            aggregate_limit: 10,
        };
        assert!(parse_scan_response(b"99\0", &request).is_err());
        assert!(parse_scan_response(
            b"1\0E\0\x30\0SKILL.md\0file\0\x31\x30\0\x31\0\x35\0abc",
            &request,
        )
        .is_err());
    }
}

#[cfg(all(test, not(target_os = "linux")))]
mod portable_tests {
    use std::collections::BTreeSet;

    use super::{parse_scan_response, ScanRequest};

    #[test]
    fn parser_rejects_unknown_protocol_and_truncated_content() {
        let request = ScanRequest {
            roots: vec!["/tmp".to_string()],
            stat_only_root_indexes: BTreeSet::new(),
            recursive: false,
            per_file_limit: 10,
            aggregate_limit: 10,
        };
        assert!(parse_scan_response(b"99\0", &request).is_err());
        assert!(parse_scan_response(
            b"1\0E\0\x30\0SKILL.md\0file\0\x31\x30\0\x31\0\x35\0abc",
            &request,
        )
        .is_err());
    }
}
