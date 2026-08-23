use std::time::Duration;

use crate::environment::wsl::protocol::{
    wsl_operation, WslOperationDescriptor, WslOperationExecutor, WslOperationRequest,
    DEFAULT_WSL_STDERR_LIMIT,
};
use crate::environment::wsl::WslSession;
use crate::error::AppError;

const SCRIPT: &str = include_str!("../scripts/library-content.sh");
const RECOVER: WslOperationDescriptor = wsl_operation("library-content", "recover", SCRIPT);
const REPLACE: WslOperationDescriptor = wsl_operation("library-content", "replace", SCRIPT);
const DELETE: WslOperationDescriptor = wsl_operation("library-content", "delete", SCRIPT);
const PREPARE_CATALOG: WslOperationDescriptor =
    wsl_operation("library-content", "prepare-catalog", SCRIPT);
const FINALIZE_CATALOG: WslOperationDescriptor =
    wsl_operation("library-content", "finalize-catalog", SCRIPT);
const ENSURE_LIBRARIES: WslOperationDescriptor =
    wsl_operation("library-content", "ensure-libraries", SCRIPT);
const REMOVE_LIBRARY: WslOperationDescriptor =
    wsl_operation("library-content", "remove-library", SCRIPT);
const REMOVE_APPLICATION: WslOperationDescriptor =
    wsl_operation("library-content", "remove-application", SCRIPT);

pub async fn recover_library_content(session: &WslSession) -> Result<(), AppError> {
    run(&RECOVER, session, Vec::new(), Vec::new()).await
}

pub async fn replace_library_skill(
    session: &WslSession,
    library_id: &str,
    skill_name: &str,
    archive: Vec<u8>,
) -> Result<(), AppError> {
    validate_component(library_id)?;
    validate_component(skill_name)?;
    run(
        &REPLACE,
        session,
        vec![
            library_id.to_string(),
            skill_name.to_string(),
            uuid::Uuid::new_v4().simple().to_string(),
        ],
        archive,
    )
    .await
}

pub async fn stage_library_skill_deletion(
    session: &WslSession,
    library_id: &str,
    skill_name: &str,
) -> Result<(), AppError> {
    validate_component(library_id)?;
    validate_component(skill_name)?;
    run(
        &DELETE,
        session,
        vec![
            library_id.to_string(),
            skill_name.to_string(),
            uuid::Uuid::new_v4().simple().to_string(),
        ],
        Vec::new(),
    )
    .await
}

pub async fn prepare_library_catalog(
    session: &WslSession,
    catalog_hash: &str,
) -> Result<(), AppError> {
    validate_hash(catalog_hash)?;
    run(
        &PREPARE_CATALOG,
        session,
        vec![catalog_hash.to_string()],
        Vec::new(),
    )
    .await
}

pub async fn finalize_library_catalog(
    session: &WslSession,
    catalog_hash: &str,
) -> Result<(), AppError> {
    validate_hash(catalog_hash)?;
    run(
        &FINALIZE_CATALOG,
        session,
        vec![catalog_hash.to_string()],
        Vec::new(),
    )
    .await
}

pub async fn ensure_library_roots(
    session: &WslSession,
    library_ids: &[String],
) -> Result<(), AppError> {
    for library_id in library_ids {
        validate_component(library_id)?;
    }
    run(&ENSURE_LIBRARIES, session, library_ids.to_vec(), Vec::new()).await
}

pub async fn remove_library(session: &WslSession, library_id: &str) -> Result<(), AppError> {
    validate_component(library_id)?;
    run(
        &REMOVE_LIBRARY,
        session,
        vec![library_id.to_string()],
        Vec::new(),
    )
    .await
}

pub async fn remove_library_application(
    session: &WslSession,
    project_id: &str,
) -> Result<(), AppError> {
    validate_component(project_id)?;
    run(
        &REMOVE_APPLICATION,
        session,
        vec![project_id.to_string()],
        Vec::new(),
    )
    .await
}

async fn run(
    operation: &WslOperationDescriptor,
    session: &WslSession,
    args: Vec<String>,
    stdin: Vec<u8>,
) -> Result<(), AppError> {
    let output = WslOperationExecutor::execute(
        operation,
        WslOperationRequest {
            session: session.clone(),
            args,
            stdin,
            timeout: Duration::from_secs(120),
            stdout_limit: 32,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation: None,
        },
    )
    .await?;
    if output.stdout == b"1\0" {
        Ok(())
    } else {
        Err(AppError::ConfigurationCorrupted {
            message: "invalid WSL Skill Library content response".to_string(),
        })
    }
}

fn validate_component(value: &str) -> Result<(), AppError> {
    if value.is_empty() || matches!(value, "." | "..") || value.contains(['/', '\\', '\0']) {
        return Err(AppError::Validation {
            field: Some("libraryStorageComponent".to_string()),
            message: "invalid Skill Library storage component".to_string(),
        });
    }
    Ok(())
}

fn validate_hash(value: &str) -> Result<(), AppError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(AppError::Validation {
            field: Some("catalogHash".to_string()),
            message: "invalid Skill Library catalog hash".to_string(),
        })
    }
}

#[cfg(all(test, target_os = "linux"))]
#[allow(
    clippy::disallowed_methods,
    reason = "WSL library protocol tests execute the shipped POSIX shell script directly"
)]
mod tests {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::process::{Command, Stdio};

    use super::SCRIPT;

    #[test]
    fn script_replaces_and_recovers_library_content_inside_the_managed_root() {
        let temp = tempfile::tempdir().unwrap();
        let first = archive(b"---\nname: demo\ndescription: First\n---\nfirst\n");
        let second = archive(b"---\nname: demo\ndescription: Second\n---\nsecond\n");

        run(temp.path(), "ensure-libraries", &["lib-empty"], &[]);
        assert!(temp
            .path()
            .join(".skill-deck/skill-libraries/libraries/lib-empty/skills")
            .is_dir());

        run(temp.path(), "replace", &["lib-1", "demo", "op-1"], &first);
        commit_catalog(temp.path(), br#"{"schemaVersion":1,"libraries":["first"]}"#);
        let skill = temp
            .path()
            .join(".skill-deck/skill-libraries/libraries/lib-1/skills/demo/SKILL.md");
        assert_eq!(
            std::fs::read(&skill).unwrap(),
            b"---\nname: demo\ndescription: First\n---\nfirst\n"
        );

        run(temp.path(), "replace", &["lib-1", "demo", "op-2"], &second);
        commit_catalog(
            temp.path(),
            br#"{"schemaVersion":1,"libraries":["second"]}"#,
        );
        assert_eq!(
            std::fs::read(&skill).unwrap(),
            b"---\nname: demo\ndescription: Second\n---\nsecond\n"
        );
        run(
            temp.path(),
            "replace",
            &["lib-1", "demo", "op-rollback"],
            &first,
        );
        run(temp.path(), "recover", &[], &[]);
        assert_eq!(
            std::fs::read(&skill).unwrap(),
            b"---\nname: demo\ndescription: Second\n---\nsecond\n"
        );
        run(
            temp.path(),
            "delete",
            &["lib-1", "demo", "delete-rollback"],
            &[],
        );
        run(temp.path(), "recover", &[], &[]);
        assert!(skill.exists());

        run(
            temp.path(),
            "delete",
            &["lib-1", "demo", "delete-commit"],
            &[],
        );
        commit_catalog(temp.path(), br#"{"schemaVersion":1,"libraries":[]}"#);
        assert!(!skill.exists());

        run(temp.path(), "replace", &["lib-1", "demo", "op-3"], &first);
        commit_catalog(temp.path(), br#"{"schemaVersion":1,"libraries":["third"]}"#);
        run(temp.path(), "remove-library", &["lib-1"], &[]);
        assert!(!temp
            .path()
            .join(".skill-deck/skill-libraries/libraries/lib-1")
            .exists());

        let orphan = temp
            .path()
            .join(".skill-deck/skill-libraries/.transactions/orphan");
        std::fs::create_dir_all(&orphan).unwrap();
        run_failure(temp.path(), "recover", &[], &[]);
        assert!(orphan.exists());
    }

    fn archive(content: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut directory = tar::Header::new_gnu();
        directory.set_path("stage").unwrap();
        directory.set_entry_type(tar::EntryType::Directory);
        directory.set_mode(0o755);
        directory.set_size(0);
        directory.set_cksum();
        builder.append(&directory, std::io::empty()).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_path("stage/SKILL.md").unwrap();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(content.len() as u64);
        header.set_cksum();
        builder.append(&header, content).unwrap();
        builder.finish().unwrap();
        builder.into_inner().unwrap()
    }

    fn commit_catalog(home: &std::path::Path, content: &[u8]) {
        let hash = format!("{:x}", Sha256::digest(content));
        run(home, "prepare-catalog", &[&hash], &[]);
        let root = home.join(".skill-deck/skill-libraries");
        std::fs::write(root.join("catalog.json"), content).unwrap();
        run(home, "finalize-catalog", &[&hash], &[]);
    }

    fn run(home: &std::path::Path, subcommand: &str, args: &[&str], stdin: &[u8]) {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(SCRIPT)
            .arg("--")
            .arg(subcommand)
            .args(args)
            .env("HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"1\0");
    }

    fn run_failure(home: &std::path::Path, subcommand: &str, args: &[&str], stdin: &[u8]) {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(SCRIPT)
            .arg("--")
            .arg(subcommand)
            .args(args)
            .env("HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
    }
}
