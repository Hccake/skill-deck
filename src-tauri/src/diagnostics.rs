use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::application::mutation::result::ErrorReport;
use crate::environment::types::{ContextScope, EnvironmentRef};
use crate::error::AppError;
use tauri::{Manager, Runtime};
use tauri_plugin_log::{Target, TargetKind};

const MAX_DIAGNOSTIC_DETAIL_CHARS: usize = 1024;
const LOCAL_LOG_MAX_FILE_SIZE: u128 = 1024 * 1024;
const LOCAL_LOG_RETAINED_FILES: usize = 3;
pub(crate) const DIAGNOSTIC_LOG_FILE_NAME: &str = "skill-deck-diagnostics.log";
const DIAGNOSTIC_LOG_TARGET: &str = "skill_deck::diagnostics";
const REDACTED: &str = "[redacted]";
const REDACTED_PATH: &str = "[redacted-path]";

static DIAGNOSTIC_RECORDER: OnceLock<Mutex<Option<DiagnosticRecorder>>> = OnceLock::new();

struct DiagnosticRecorder {
    directory: PathBuf,
    max_file_size: u64,
    retained_files: usize,
}

impl DiagnosticRecorder {
    fn production(directory: PathBuf) -> Self {
        Self {
            directory,
            max_file_size: LOCAL_LOG_MAX_FILE_SIZE as u64,
            retained_files: LOCAL_LOG_RETAINED_FILES,
        }
    }

    fn active_path(&self) -> PathBuf {
        self.directory.join(DIAGNOSTIC_LOG_FILE_NAME)
    }

    fn rotated_path(&self, index: usize) -> PathBuf {
        self.directory
            .join(format!("skill-deck-diagnostics.{index}.log"))
    }

    fn write_line(&self, line: &str) -> std::io::Result<()> {
        fs::create_dir_all(&self.directory)?;
        let active = self.active_path();
        let pending_bytes = line.len().saturating_add(1) as u64;
        let current_bytes = match fs::symlink_metadata(&active) {
            Ok(metadata) if metadata_is_link_like(&metadata) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "diagnostics log path must not be a link",
                ));
            }
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        if current_bytes > 0 && current_bytes.saturating_add(pending_bytes) > self.max_file_size {
            self.rotate()?;
        }
        let mut file = open_append_no_follow(&active)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        Ok(())
    }

    fn rotate(&self) -> std::io::Result<()> {
        if self.retained_files <= 1 {
            let active = self.active_path();
            if active.exists() {
                fs::remove_file(active)?;
            }
            return Ok(());
        }

        let oldest = self.rotated_path(self.retained_files - 1);
        if oldest.exists() {
            fs::remove_file(oldest)?;
        }
        for index in (2..self.retained_files).rev() {
            let source = self.rotated_path(index - 1);
            if source.exists() {
                fs::rename(source, self.rotated_path(index))?;
            }
        }
        let active = self.active_path();
        if active.exists() {
            fs::rename(active, self.rotated_path(1))?;
        }
        Ok(())
    }
}

fn open_append_no_follow(path: &std::path::Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }

    options.open(path)
}

#[cfg(not(windows))]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

pub(crate) fn diagnostic_log_paths(directory: &std::path::Path) -> Vec<PathBuf> {
    std::iter::once(directory.join(DIAGNOSTIC_LOG_FILE_NAME))
        .chain(
            (1..LOCAL_LOG_RETAINED_FILES)
                .map(|index| directory.join(format!("skill-deck-diagnostics.{index}.log"))),
        )
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum DiagnosticOperation {
    ManageAgents,
    Remove,
    Copy,
    Install,
    Update,
    DuplicateCleanup,
    SourceDiscovery,
    SourceAcquisition,
}

impl DiagnosticOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::ManageAgents => "manageAgents",
            Self::Remove => "remove",
            Self::Copy => "copy",
            Self::Install => "install",
            Self::Update => "update",
            Self::DuplicateCleanup => "duplicateCleanup",
            Self::SourceDiscovery => "sourceDiscovery",
            Self::SourceAcquisition => "sourceAcquisition",
        }
    }
}

pub(crate) fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_log::Builder::default()
        .level(log::LevelFilter::Info)
        .targets([Target::new(TargetKind::Stdout)])
        .build()
}

pub(crate) fn initialize<R: Runtime>(app: &tauri::AppHandle<R>) {
    let recorder = app
        .path()
        .app_log_dir()
        .ok()
        .map(DiagnosticRecorder::production);
    let slot = DIAGNOSTIC_RECORDER.get_or_init(|| Mutex::new(None));
    if let Ok(mut current) = slot.lock() {
        *current = recorder;
    }
}

pub(crate) fn record_mutation_failure(report: &ErrorReport) {
    record_mutation_failure_for(None, report);
}

fn record_mutation_failure_for(operation: Option<DiagnosticOperation>, report: &ErrorReport) {
    let event = format_mutation_failure(operation, report);
    log::error!(
        target: DIAGNOSTIC_LOG_TARGET,
        "{}",
        event
    );
    persist(&event);
}

pub(crate) fn record_command_result<T>(
    operation: DiagnosticOperation,
    result: &Result<T, AppError>,
    context: &crate::environment::types::ContextRef,
) {
    if let Err(error) = result {
        let report = ErrorReport::from_app_error(error.clone(), Some(context.clone()));
        record_mutation_failure_for(Some(operation), &report);
    }
}

pub(crate) fn record_command_result_for_environment<T>(
    operation: DiagnosticOperation,
    result: &Result<T, AppError>,
    environment: &EnvironmentRef,
) {
    if let Err(error) = result {
        let mut report = ErrorReport::from_app_error(error.clone(), None);
        if report.environment.is_none() {
            report.environment = Some(environment.clone());
        }
        record_mutation_failure_for(Some(operation), &report);
    }
}

pub(crate) fn record_mutation_cleanup_warning(details: &str) {
    let event = serde_json::json!({
        "event": "mutationCleanupWarning",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "technicalDetails": redact_untrusted_details(details),
    })
    .to_string();
    log::warn!(target: DIAGNOSTIC_LOG_TARGET, "{event}");
    persist(&event);
}

pub(crate) fn format_mutation_failure(
    operation: Option<DiagnosticOperation>,
    report: &ErrorReport,
) -> String {
    let parameters = report
        .parameters
        .iter()
        .map(|(key, value)| {
            let sanitized = sanitize_parameter(key, value);
            (key.clone(), sanitized)
        })
        .collect::<BTreeMap<_, _>>();

    serde_json::json!({
        "event": "mutationFailure",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "operation": operation.map(DiagnosticOperation::as_str),
        "code": report.code,
        "severity": report.severity,
        "retryable": report.retryable,
        "parameters": parameters,
        "environment": report.environment.as_ref().map(environment_label),
        "context": report.context.as_ref().map(|context| match context.scope {
            ContextScope::Global => "global",
            ContextScope::Project { .. } => "project",
        }),
        "recoveryId": report.recovery_resource_id.as_ref().map(|id| id.as_str()),
        "technicalDetails": report
            .technical_details
            .as_deref()
            .map(|details| sanitize_report_details(report, details)),
    })
    .to_string()
}

fn persist(event: &str) {
    let Some(slot) = DIAGNOSTIC_RECORDER.get() else {
        return;
    };
    let Ok(current) = slot.lock() else {
        return;
    };
    if let Some(recorder) = current.as_ref() {
        let _ = recorder.write_line(event);
    }
}

fn environment_label(environment: &EnvironmentRef) -> String {
    match environment {
        EnvironmentRef::Host => "host".to_string(),
        EnvironmentRef::Wsl { .. } => "wsl".to_string(),
    }
}

fn sanitize_parameter(key: &str, value: &str) -> String {
    if is_path_key(key) {
        return REDACTED_PATH.to_string();
    }
    if key.eq_ignore_ascii_case("capability") && is_known_capability(value) {
        return value.to_string();
    }
    REDACTED.to_string()
}

fn sanitize_report_details(report: &ErrorReport, value: &str) -> String {
    let mut sanitized = value.to_string();
    let mut replacements = Vec::<(String, &'static str)>::new();
    for (key, parameter) in &report.parameters {
        if parameter.is_empty()
            || (key.eq_ignore_ascii_case("capability") && is_known_capability(parameter))
        {
            continue;
        }
        let replacement = if is_path_key(key) {
            REDACTED_PATH
        } else {
            REDACTED
        };
        replacements.push((parameter.clone(), replacement));
    }
    for display_path in &report.display_paths {
        if !display_path.native_path.is_empty() {
            replacements.push((display_path.native_path.clone(), REDACTED_PATH));
        }
    }
    if let Some(ContextScope::Project { project_id }) =
        report.context.as_ref().map(|context| &context.scope)
    {
        if !project_id.is_empty() {
            replacements.push((project_id.clone(), REDACTED));
        }
    }
    if let Some(unit_id) = report.unit_id.as_deref() {
        if !unit_id.is_empty() {
            replacements.push((unit_id.to_string(), REDACTED));
        }
    }
    replacements.sort_by_key(|item| std::cmp::Reverse(item.0.len()));
    for (sensitive, replacement) in replacements {
        sanitized = sanitized.replace(&sensitive, replacement);
    }
    redact_untrusted_details(&sanitized)
}

fn redact_untrusted_details(value: &str) -> String {
    let lowercase = value.to_ascii_lowercase();
    if [
        "authorization:",
        "bearer ",
        "password=",
        "password:",
        "token=",
        "token:",
        "secret=",
        "secret:",
        "private_key",
        "ssh_secret",
        "api_key",
        "apikey",
        "cookie:",
    ]
    .iter()
    .any(|marker| lowercase.contains(marker))
    {
        return REDACTED.to_string();
    }

    value
        .split_whitespace()
        .map(|token| {
            if detail_token_contains_private_locator(token) {
                REDACTED_PATH
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_DIAGNOSTIC_DETAIL_CHARS)
        .collect()
}

fn detail_token_contains_private_locator(token: &str) -> bool {
    let token = token.trim_start_matches(['(', '[', '{', '"', '\'']);
    let value = token
        .split_once('=')
        .map(|(_, value)| value)
        .unwrap_or(token);
    let bytes = value.as_bytes();
    let windows_absolute = bytes.len() >= 3
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
        && bytes[0].is_ascii_alphabetic();

    value.starts_with('/')
        || value.starts_with("\\\\")
        || windows_absolute
        || value.contains("://")
        || value.starts_with("git@")
}

fn is_known_capability(value: &str) -> bool {
    matches!(
        value,
        "wslExecutionFeature.nulSafeXargs"
            | "wslExecutionFeature.nulSafeSort"
            | "wslExecutionFeature.sha256Sum"
            | "wslExecutionFeature.canonicalReadlink"
            | "wslExecutionFeature.stableStat"
            | "payloadStorageCleanup"
            | "wslPayloadManifestSize"
            | "wslGit"
            | "backendPayloadAcquisition"
            | "backendSourceMetadataFingerprint"
            | "payloadSessionCapacity"
            | "runtimeMaintenancePending"
            | "runtimeMaintenanceFailed"
            | "stableIdentity"
            | "createLink"
            | "backendLocalPayload"
            | "wslMaterializeRequestSize"
    )
}

fn is_path_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == "path" || key.ends_with("path") || key == "target" || key == "source"
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::application::mutation::result::{ErrorSeverity, OperationErrorCode};
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
    use crate::error::RecoveryResourceId;

    fn report_with_sensitive_fields() -> ErrorReport {
        ErrorReport {
            code: OperationErrorCode::CapabilityUnavailable,
            parameters: BTreeMap::from([
                (
                    "capability".to_string(),
                    "wslExecutionFeature.nulSafeXargs".to_string(),
                ),
                (
                    "path".to_string(),
                    "/home/alice/private/project".to_string(),
                ),
                ("token".to_string(), "top-secret-token".to_string()),
            ]),
            field: None,
            severity: ErrorSeverity::Error,
            retryable: true,
            technical_details: Some(format!(
                "failed at C:\\Users\\Alice\\private with bearer top-secret {}",
                "x".repeat(MAX_DIAGNOSTIC_DETAIL_CHARS * 2),
            )),
            environment: Some(EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            }),
            context: Some(ContextRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                scope: ContextScope::Project {
                    project_id: "private-project-id".to_string(),
                },
            }),
            unit_id: Some("raw-private-unit-id".to_string()),
            recovery_resource_id: Some(
                RecoveryResourceId::parse("recovery-123").expect("valid recovery ID"),
            ),
            display_paths: vec![ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: "C:\\Users\\Alice\\private".to_string(),
            }],
        }
    }

    #[test]
    fn mutation_failure_keeps_stable_fields_and_redacts_sensitive_context() {
        let mut report = report_with_sensitive_fields();
        report
            .parameters
            .insert("agent".to_string(), "private-customer-agent".to_string());
        report.parameters.insert(
            "source".to_string(),
            "customer-internal-repository".to_string(),
        );
        report.technical_details = Some(
            "custom agent private-customer-agent rejected customer-internal-repository".to_string(),
        );
        let formatted = format_mutation_failure(Some(DiagnosticOperation::ManageAgents), &report);

        assert!(formatted.contains("\"event\":\"mutationFailure\""));
        assert!(formatted.contains("\"operation\":\"manageAgents\""));
        assert!(formatted.contains("\"code\":\"capabilityUnavailable\""));
        assert!(formatted.contains("\"environment\":\"wsl\""));
        assert!(formatted.contains("\"context\":\"project\""));
        assert!(formatted.contains("\"recoveryId\":\"recovery-123\""));
        assert!(formatted.contains("nulSafeXargs"));

        for sensitive in [
            "/home/alice/private/project",
            "C:\\Users\\Alice\\private",
            "top-secret-token",
            "top-secret",
            "private-project-id",
            "raw-private-unit-id",
            "private-customer-agent",
            "customer-internal-repository",
        ] {
            assert!(
                !formatted.contains(sensitive),
                "diagnostic leaked sensitive value: {sensitive}"
            );
        }
    }

    #[test]
    fn mutation_failure_bounds_untrusted_technical_details() {
        let mut report = report_with_sensitive_fields();
        report.technical_details = Some("x".repeat(MAX_DIAGNOSTIC_DETAIL_CHARS * 2));

        let formatted = format_mutation_failure(None, &report);
        let parsed: serde_json::Value =
            serde_json::from_str(&formatted).expect("structured diagnostic JSON");
        let details = parsed["technicalDetails"]
            .as_str()
            .expect("technical details string");

        assert_eq!(details, "x".repeat(MAX_DIAGNOSTIC_DETAIL_CHARS));
    }

    #[test]
    fn mutation_failure_keeps_non_sensitive_technical_root_cause() {
        let mut report = report_with_sensitive_fields();
        report.technical_details =
            Some("materialize verification failed after staged entries were swapped".to_string());

        let formatted = format_mutation_failure(None, &report);
        let parsed: serde_json::Value =
            serde_json::from_str(&formatted).expect("structured diagnostic JSON");

        assert_eq!(
            parsed["technicalDetails"],
            "materialize verification failed after staged entries were swapped"
        );
    }

    #[test]
    fn mutation_failure_redacts_credentials_embedded_in_technical_details() {
        let mut report = report_with_sensitive_fields();
        report.technical_details =
            Some("remote rejected Authorization: Bearer top-secret".to_string());

        let formatted = format_mutation_failure(None, &report);
        let parsed: serde_json::Value =
            serde_json::from_str(&formatted).expect("structured diagnostic JSON");

        assert_eq!(parsed["technicalDetails"], REDACTED);
        assert!(!formatted.contains("top-secret"));
    }

    #[test]
    fn mutation_failure_redacts_overlapping_parameter_values_completely() {
        let mut report = report_with_sensitive_fields();
        report.parameters = BTreeMap::from([
            ("agent".to_string(), "foo".to_string()),
            ("source".to_string(), "foo/private-repo".to_string()),
        ]);
        report.technical_details = Some("clone foo/private-repo failed".to_string());

        let formatted = format_mutation_failure(None, &report);

        assert!(!formatted.contains("foo"));
        assert!(!formatted.contains("private-repo"));
        assert!(formatted.contains(REDACTED_PATH));
    }

    #[test]
    fn mutation_failure_redacts_absolute_paths_embedded_in_error_fields() {
        let mut report = report_with_sensitive_fields();
        report.technical_details =
            Some("filesystem error: path=/home/alice/private/project was unavailable".to_string());

        let formatted = format_mutation_failure(None, &report);

        assert!(!formatted.contains("/home/alice/private/project"));
        assert!(formatted.contains(REDACTED_PATH));
        assert!(formatted.contains("filesystem error"));
        assert!(formatted.contains("was unavailable"));
    }

    #[test]
    fn local_log_policy_is_capacity_limited() {
        assert_eq!(LOCAL_LOG_MAX_FILE_SIZE, 1024 * 1024);
        assert_eq!(LOCAL_LOG_RETAINED_FILES, 3);
    }

    #[test]
    fn local_recorder_rotates_and_retains_only_the_configured_files() {
        let temp = tempfile::tempdir().expect("temporary diagnostics directory");
        let recorder = DiagnosticRecorder {
            directory: temp.path().to_path_buf(),
            max_file_size: 32,
            retained_files: 3,
        };

        for index in 0..8 {
            recorder
                .write_line(&format!("record-{index}-1234567890"))
                .expect("write diagnostic record");
        }

        let files = fs::read_dir(temp.path())
            .expect("read diagnostics directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read diagnostics entries");
        assert_eq!(files.len(), 3);
        assert!(recorder.active_path().exists());
        assert!(recorder.rotated_path(1).exists());
        assert!(recorder.rotated_path(2).exists());
    }

    #[test]
    fn local_recorder_continues_the_active_file_after_restart() {
        let temp = tempfile::tempdir().expect("temporary diagnostics directory");
        let first = DiagnosticRecorder {
            directory: temp.path().to_path_buf(),
            max_file_size: 1024,
            retained_files: 3,
        };
        first.write_line("first").expect("first record");
        drop(first);

        let restarted = DiagnosticRecorder {
            directory: temp.path().to_path_buf(),
            max_file_size: 1024,
            retained_files: 3,
        };
        restarted.write_line("second").expect("second record");

        let contents = fs::read_to_string(restarted.active_path()).expect("active diagnostics");
        assert_eq!(contents, "first\nsecond\n");
    }

    #[test]
    fn local_recorder_failure_does_not_require_a_fallback_file() {
        let temp = tempfile::tempdir().expect("temporary diagnostics directory");
        let file = temp.path().join("not-a-directory");
        fs::write(&file, "occupied").expect("create blocking file");
        let recorder = DiagnosticRecorder {
            directory: file,
            max_file_size: 1024,
            retained_files: 3,
        };

        assert!(recorder.write_line("record").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn local_recorder_does_not_follow_the_active_log_symlink() {
        let temp = tempfile::tempdir().expect("temporary diagnostics directory");
        let private = temp.path().join("private.txt");
        fs::write(&private, "private\n").expect("private target");
        std::os::unix::fs::symlink(&private, temp.path().join(DIAGNOSTIC_LOG_FILE_NAME))
            .expect("diagnostics symlink");
        let recorder = DiagnosticRecorder {
            directory: temp.path().to_path_buf(),
            max_file_size: 1024,
            retained_files: 3,
        };

        assert!(recorder.write_line("must-not-be-written").is_err());
        assert_eq!(
            fs::read_to_string(private).expect("private target remains readable"),
            "private\n"
        );
    }

    #[cfg(windows)]
    #[test]
    fn local_recorder_rejects_the_active_log_reparse_point() {
        let temp = tempfile::tempdir().expect("temporary diagnostics directory");
        let private = temp.path().join("private");
        fs::create_dir_all(&private).expect("private target directory");
        junction::create(&private, temp.path().join(DIAGNOSTIC_LOG_FILE_NAME))
            .expect("diagnostics junction");
        let recorder = DiagnosticRecorder {
            directory: temp.path().to_path_buf(),
            max_file_size: 1024,
            retained_files: 3,
        };

        assert!(recorder.write_line("must-not-be-written").is_err());
        assert_eq!(
            fs::read_dir(private)
                .expect("private target remains readable")
                .count(),
            0
        );
    }

    #[test]
    fn diagnostics_plugin_builds_with_the_tauri_mock_runtime() {
        let app = tauri::test::mock_builder()
            .plugin(plugin())
            .build(tauri::generate_context!());

        app.expect("diagnostics plugin should build with the mock runtime");
    }
}
