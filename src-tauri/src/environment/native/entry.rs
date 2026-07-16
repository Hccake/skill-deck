use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

use crate::core::skill_payload::{verify_skill_payload_integrity, PayloadEntryKind, SkillPayload};
use crate::environment::content_manifest::ContentManifestHash;
use crate::environment::native::content_manifest::read_directory;
use crate::environment::native::tree::{
    inspect_entry_no_follow, physical_parent_identity, project_target, remove_entry_no_follow,
    NativeEntryKind,
};
use crate::environment::recovery::RecoveryExpectedEntryState;
use crate::environment::runtime::{EntryFingerprint, ExecutionBackend, PhysicalTargetKey};
use crate::error::AppError;

#[derive(Debug, Clone)]
pub enum NativeEntryAction {
    Keep,
    Materialize { payload: Arc<SkillPayload> },
    Symlink { target: PathBuf },
    Remove,
}

#[derive(Debug, Clone)]
pub struct NativeEntryIntent {
    pub target: PhysicalTargetKey,
    pub destination: PathBuf,
    pub expected_fingerprint: EntryFingerprint,
    pub expected_content_manifest_hash: Option<ContentManifestHash>,
    pub action: NativeEntryAction,
}

#[derive(Debug, Clone)]
struct StagedEntry {
    intent: NativeEntryIntent,
    parent_identity: crate::environment::runtime::PhysicalParentIdentity,
    staged_path: Option<PathBuf>,
    backup_path: PathBuf,
    backup_created: bool,
    installed: bool,
}

#[derive(Debug, Clone)]
pub struct NativeEntrySet {
    entries: Vec<StagedEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRecoveryPath {
    pub target: PhysicalTargetKey,
    pub destination: PathBuf,
    pub backup: PathBuf,
    pub expected_state: RecoveryExpectedEntryState,
    pub original_fingerprint: EntryFingerprint,
}

pub fn stage_entry_set(intents: &[NativeEntryIntent]) -> Result<NativeEntrySet, AppError> {
    preflight_initial_intents(intents)?;
    let mut entries = Vec::with_capacity(intents.len());
    for intent in intents {
        let parent = intent
            .destination
            .parent()
            .ok_or_else(|| unsafe_destination(&intent.destination))?;
        fs::create_dir_all(parent)?;
        let parent_identity = physical_parent_identity(parent)?;
        let backup_path = unique_sibling(&intent.destination, "backup")?;
        let staged_path = match &intent.action {
            NativeEntryAction::Keep => None,
            NativeEntryAction::Materialize { payload } => {
                let stage = unique_sibling(&intent.destination, "stage")?;
                if let Err(error) = materialize_payload(payload, &stage) {
                    let _ = remove_entry_no_follow(&stage);
                    cleanup_staged(&mut entries);
                    return Err(error);
                }
                Some(stage)
            }
            NativeEntryAction::Symlink { target } => {
                let stage = unique_sibling(&intent.destination, "stage")?;
                if let Err(error) = create_directory_link(target, &stage) {
                    let _ = remove_entry_no_follow(&stage);
                    cleanup_staged(&mut entries);
                    return Err(error);
                }
                Some(stage)
            }
            NativeEntryAction::Remove => None,
        };
        entries.push(StagedEntry {
            intent: intent.clone(),
            parent_identity,
            staged_path,
            backup_path,
            backup_created: false,
            installed: false,
        });
    }
    Ok(NativeEntrySet { entries })
}

pub fn planned_recovery_paths(entries: &NativeEntrySet) -> Vec<NativeRecoveryPath> {
    entries
        .entries
        .iter()
        .filter(|entry| !matches!(entry.intent.action, NativeEntryAction::Keep))
        .map(|entry| NativeRecoveryPath {
            target: entry.intent.target.clone(),
            destination: entry.intent.destination.clone(),
            backup: entry.backup_path.clone(),
            expected_state: match entry.intent.action {
                NativeEntryAction::Remove => RecoveryExpectedEntryState::Missing,
                NativeEntryAction::Materialize { .. } | NativeEntryAction::Symlink { .. } => {
                    RecoveryExpectedEntryState::Present
                }
                NativeEntryAction::Keep => unreachable!("Keep entries are filtered"),
            },
            original_fingerprint: entry.intent.expected_fingerprint.clone(),
        })
        .collect()
}

pub fn swap_entry_set(entries: &mut NativeEntrySet) -> Result<(), AppError> {
    recheck_entry_set(entries)?;

    for index in 0..entries.entries.len() {
        let result = swap_one(&mut entries.entries[index]);
        if let Err(primary) = result {
            if let Err(restore) = restore_entry_set(entries) {
                return Err(AppError::RestoreFailed {
                    message: format!("{primary}; {restore}"),
                });
            }
            return Err(primary);
        }
    }
    Ok(())
}

pub fn recheck_entry_set(entries: &NativeEntrySet) -> Result<(), AppError> {
    for entry in &entries.entries {
        let parent = entry
            .intent
            .destination
            .parent()
            .ok_or_else(|| unsafe_destination(&entry.intent.destination))?;
        if physical_parent_identity(parent)? != entry.parent_identity
            || inspect_entry_no_follow(&entry.intent.destination)?.fingerprint
                != entry.intent.expected_fingerprint
        {
            return Err(AppError::StaleTarget);
        }
        if let Some(expected) = &entry.intent.expected_content_manifest_hash {
            let actual =
                read_directory(&entry.intent.destination).map_err(|_| AppError::StaleTarget)?;
            if actual.hash() != expected {
                return Err(AppError::StaleTarget);
            }
        }
        if !matches!(entry.intent.action, NativeEntryAction::Keep)
            && inspect_entry_no_follow(&entry.backup_path)?.kind != NativeEntryKind::Missing
        {
            return Err(AppError::StaleTarget);
        }
        preflight_staged_entry(entry)?;
        preflight_atomic_replace(parent)?;
    }
    Ok(())
}

pub fn verify_entry_set(entries: &NativeEntrySet) -> Result<(), AppError> {
    for entry in &entries.entries {
        let inspected = inspect_entry_no_follow(&entry.intent.destination)?;
        let valid = match &entry.intent.action {
            NativeEntryAction::Keep => inspected.fingerprint == entry.intent.expected_fingerprint,
            NativeEntryAction::Materialize { payload } => {
                verify_materialized_payload(payload, &entry.intent.destination).is_ok()
            }
            NativeEntryAction::Symlink { target } => {
                matches!(
                    inspected.kind,
                    NativeEntryKind::Symlink | NativeEntryKind::ReparsePoint
                ) && fs::canonicalize(&entry.intent.destination).is_ok_and(|actual| {
                    fs::canonicalize(target).is_ok_and(|expected| actual == expected)
                })
            }
            NativeEntryAction::Remove => inspected.kind == NativeEntryKind::Missing,
        };
        if !valid {
            return Err(AppError::ExecutionFailed {
                message: "native entry verification failed".to_string(),
            });
        }
    }
    Ok(())
}

pub fn restore_entry_set(entries: &mut NativeEntrySet) -> Result<(), AppError> {
    for entry in entries.entries.iter_mut().rev() {
        if entry.installed {
            remove_entry_no_follow(&entry.intent.destination)?;
            entry.installed = false;
        }
        if entry.backup_created {
            fs::rename(&entry.backup_path, &entry.intent.destination)?;
            entry.backup_created = false;
        }
    }
    Ok(())
}

pub fn cleanup_entry_set(entries: NativeEntrySet) -> Result<Vec<String>, AppError> {
    let mut warnings = Vec::new();
    for entry in entries.entries {
        let mut paths = entry.staged_path.into_iter().collect::<Vec<_>>();
        if entry.backup_created {
            paths.push(entry.backup_path);
        }
        for path in paths {
            if let Err(error) = remove_entry_no_follow(&path) {
                warnings.push(format!("{}: {error}", path.display()));
            }
        }
    }
    Ok(warnings)
}

fn preflight_initial_intents(intents: &[NativeEntryIntent]) -> Result<(), AppError> {
    let mut keys = BTreeSet::new();
    for intent in intents {
        if !keys.insert(intent.target.clone()) {
            return Err(AppError::StaleTarget);
        }
        if projected_key_for_destination(&intent.destination, &intent.target.backend)?
            != intent.target
        {
            return Err(AppError::StaleTarget);
        }
        if inspect_entry_no_follow(&intent.destination)?.fingerprint != intent.expected_fingerprint
        {
            return Err(AppError::StaleTarget);
        }
    }
    Ok(())
}

fn projected_key_for_destination(
    destination: &Path,
    backend: &ExecutionBackend,
) -> Result<PhysicalTargetKey, AppError> {
    let projection = project_target(destination, backend.clone())?;
    if projection.physical_destination != destination {
        return Err(AppError::StaleTarget);
    }
    Ok(projection.key)
}

fn unsafe_destination(destination: &Path) -> AppError {
    AppError::UnsafePath {
        path: destination.to_string_lossy().into_owned(),
        reason: "destination has no safe existing ancestor".to_string(),
    }
}

fn swap_one(entry: &mut StagedEntry) -> Result<(), AppError> {
    if matches!(entry.intent.action, NativeEntryAction::Keep) {
        return Ok(());
    }
    if inspect_entry_no_follow(&entry.intent.destination)?.kind != NativeEntryKind::Missing {
        fs::rename(&entry.intent.destination, &entry.backup_path)?;
        entry.backup_created = true;
    }
    if let Some(stage) = entry.staged_path.take() {
        fs::rename(stage, &entry.intent.destination)?;
        entry.installed = true;
    }
    Ok(())
}

fn materialize_payload(payload: &SkillPayload, destination: &Path) -> Result<(), AppError> {
    verify_skill_payload_integrity(payload)?;
    fs::create_dir(destination)?;
    for entry in &payload.entries {
        let path = destination.join(&entry.relative_path);
        match entry.kind {
            PayloadEntryKind::Directory => fs::create_dir_all(&path)?,
            PayloadEntryKind::File => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let blob_id = entry.blob_id.as_deref().ok_or(AppError::StalePayload)?;
                fs::write(
                    &path,
                    payload.blobs.get(blob_id).ok_or(AppError::StalePayload)?,
                )?;
                set_executable(&path, entry.executable)?;
            }
        }
    }
    Ok(())
}

fn preflight_staged_entry(entry: &StagedEntry) -> Result<(), AppError> {
    match (&entry.intent.action, &entry.staged_path) {
        (NativeEntryAction::Materialize { payload }, Some(stage)) => {
            verify_materialized_payload(payload, stage)
        }
        (NativeEntryAction::Symlink { target }, Some(stage)) => {
            let kind = inspect_entry_no_follow(stage)?.kind;
            if !matches!(
                kind,
                NativeEntryKind::Symlink | NativeEntryKind::ReparsePoint
            ) || !staged_directory_link_matches(stage, target)?
            {
                return Err(stage_preflight_failed(stage, "staged link target mismatch"));
            }
            Ok(())
        }
        (NativeEntryAction::Keep | NativeEntryAction::Remove, None) => Ok(()),
        _ => Err(stage_preflight_failed(
            &entry.intent.destination,
            "staged action shape mismatch",
        )),
    }
}

fn verify_materialized_payload(payload: &SkillPayload, stage: &Path) -> Result<(), AppError> {
    verify_skill_payload_integrity(payload)?;
    let metadata = fs::symlink_metadata(stage)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(stage_preflight_failed(
            stage,
            "staged payload root is not a directory",
        ));
    }

    let expected = payload
        .entries
        .iter()
        .map(|entry| (PathBuf::from(&entry.relative_path), entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut pending = vec![stage.to_path_buf()];
    let mut actual = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        for child in fs::read_dir(&directory)? {
            let child = child?;
            let path = child.path();
            let relative = path
                .strip_prefix(stage)
                .map_err(|_| stage_preflight_failed(&path, "staged path escaped payload root"))?
                .to_path_buf();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || !actual.insert(relative.clone()) {
                return Err(stage_preflight_failed(&path, "unexpected staged entry"));
            }
            let Some(manifest_entry) = expected.get(&relative) else {
                return Err(stage_preflight_failed(
                    &path,
                    "entry is absent from manifest",
                ));
            };
            match manifest_entry.kind {
                PayloadEntryKind::Directory if metadata.is_dir() => pending.push(path),
                PayloadEntryKind::File if metadata.is_file() => {
                    let blob_id = manifest_entry
                        .blob_id
                        .as_deref()
                        .ok_or(AppError::StalePayload)?;
                    let expected_blob = payload.blobs.get(blob_id).ok_or(AppError::StalePayload)?;
                    if metadata.len() != manifest_entry.size || fs::read(&path)? != *expected_blob {
                        return Err(stage_preflight_failed(
                            &path,
                            "staged file content mismatch",
                        ));
                    }
                    verify_executable_mode(&path, manifest_entry.executable)?;
                }
                _ => return Err(stage_preflight_failed(&path, "staged entry type mismatch")),
            }
        }
    }
    if actual.len() != expected.len() {
        return Err(stage_preflight_failed(stage, "staged tree is incomplete"));
    }
    Ok(())
}

fn preflight_atomic_replace(parent: &Path) -> Result<(), AppError> {
    let probe = unique_sibling(&parent.join("skill-deck-probe"), "probe")?;
    let renamed = unique_sibling(&parent.join("skill-deck-probe"), "probe-renamed")?;
    fs::create_dir(&probe)?;
    fs::write(probe.join(".skill-deck-owner"), b"stage-preflight-v1\n")?;
    let result = fs::rename(&probe, &renamed).and_then(|_| fs::rename(&renamed, &probe));
    let cleanup_result =
        remove_entry_no_follow(&probe).or_else(|_| remove_entry_no_follow(&renamed));
    result?;
    cleanup_result?;
    Ok(())
}

#[cfg(unix)]
fn verify_executable_mode(path: &Path, expected: bool) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let executable = fs::metadata(path)?.permissions().mode() & 0o111 != 0;
    if executable != expected {
        return Err(stage_preflight_failed(
            path,
            "staged executable mode mismatch",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_executable_mode(_path: &Path, _expected: bool) -> Result<(), AppError> {
    Ok(())
}

fn stage_preflight_failed(path: &Path, reason: &str) -> AppError {
    AppError::ExecutionFailed {
        message: format!(
            "native stage preflight failed for {}: {reason}",
            path.display()
        ),
    }
}

fn unique_sibling(destination: &Path, kind: &str) -> Result<PathBuf, AppError> {
    let parent = destination.parent().ok_or_else(|| AppError::UnsafePath {
        path: destination.to_string_lossy().into_owned(),
        reason: "destination has no parent".to_string(),
    })?;
    for _ in 0..8 {
        let candidate = parent.join(format!(".skill-deck-{kind}-{}", Uuid::new_v4().simple()));
        if fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    Err(AppError::ExecutionFailed {
        message: "failed to allocate native staging entry".to_string(),
    })
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::symlink;

    let parent = link.parent().ok_or_else(|| AppError::UnsafePath {
        path: link.to_string_lossy().into_owned(),
        reason: "link has no parent".to_string(),
    })?;
    symlink(unix_link_target(parent, target), link)?;
    Ok(())
}

#[cfg(unix)]
fn staged_directory_link_matches(link: &Path, target: &Path) -> Result<bool, AppError> {
    let parent = link.parent().ok_or_else(|| AppError::UnsafePath {
        path: link.to_string_lossy().into_owned(),
        reason: "link has no parent".to_string(),
    })?;
    Ok(fs::read_link(link)? == unix_link_target(parent, target))
}

#[cfg(unix)]
fn unix_link_target(parent: &Path, target: &Path) -> PathBuf {
    relative_path(parent, target).unwrap_or_else(|| target.to_path_buf())
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> Result<(), AppError> {
    if junction::create(target, link).is_ok() {
        return Ok(());
    }
    std::os::windows::fs::symlink_dir(target, link)?;
    Ok(())
}

#[cfg(windows)]
fn staged_directory_link_matches(link: &Path, target: &Path) -> Result<bool, AppError> {
    if target.exists() {
        return Ok(fs::canonicalize(link)? == fs::canonicalize(target)?);
    }
    Ok(fs::read_link(link)? == target)
}

#[cfg(not(any(unix, windows)))]
fn create_directory_link(_target: &Path, link: &Path) -> Result<(), AppError> {
    Err(AppError::CapabilityUnavailable {
        capability: "createLink".to_string(),
        path: Some(link.to_string_lossy().into_owned()),
    })
}

#[cfg(not(any(unix, windows)))]
fn staged_directory_link_matches(_link: &Path, _target: &Path) -> Result<bool, AppError> {
    Ok(false)
}

fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
    let from = fs::canonicalize(from).ok()?;
    let to = fs::canonicalize(to).ok()?;
    let from_components = from.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    let common = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0
        || matches!(
            (from_components.first(), to_components.first()),
            (Some(Component::Prefix(_)), Some(Component::Prefix(_)))
        ) && from_components.first() != to_components.first()
    {
        return None;
    }
    let mut relative = PathBuf::new();
    for _ in common..from_components.len() {
        relative.push("..");
    }
    for component in &to_components[common..] {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    let mode = permissions.mode();
    permissions.set_mode(if executable {
        mode | 0o111
    } else {
        mode & !0o111
    });
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _executable: bool) -> Result<(), AppError> {
    Ok(())
}

fn cleanup_staged(entries: &mut Vec<StagedEntry>) {
    for entry in entries.drain(..) {
        if let Some(path) = entry.staged_path {
            let _ = remove_entry_no_follow(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::core::skill_payload::build_skill_payload;
    use crate::environment::native::tree::{inspect_entry_no_follow, physical_parent_identity};
    use crate::environment::runtime::{
        physical_target_key, projected_physical_target_key, ExecutionBackend,
    };

    fn payload() -> Arc<SkillPayload> {
        let temp = tempdir().expect("payload source");
        let root = temp.path().join("demo");
        fs::create_dir_all(root.join("scripts")).expect("scripts");
        fs::write(root.join("SKILL.md"), b"new skill").expect("skill");
        fs::write(root.join("scripts/run.sh"), b"#!/bin/sh\n").expect("script");
        Arc::new(build_skill_payload(&root).expect("payload"))
    }

    fn existing_destination(parent: &Path, name: &str, content: &[u8]) -> PathBuf {
        let destination = parent.join(name);
        fs::create_dir(&destination).expect("destination");
        fs::write(destination.join("SKILL.md"), content).expect("old skill");
        destination
    }

    fn intent(
        parent: &Path,
        destination: PathBuf,
        payload: Arc<SkillPayload>,
    ) -> NativeEntryIntent {
        let expected = inspect_entry_no_follow(&destination).expect("inspect");
        let expected_content_manifest_hash = read_directory(&destination)
            .expect("content manifest")
            .hash()
            .clone();
        NativeEntryIntent {
            target: physical_target_key(
                ExecutionBackend::NativeUnix,
                physical_parent_identity(parent).expect("parent identity"),
                destination.file_name().unwrap().to_str().unwrap(),
                true,
            )
            .expect("target"),
            destination,
            expected_fingerprint: expected.fingerprint,
            expected_content_manifest_hash: Some(expected_content_manifest_hash),
            action: NativeEntryAction::Materialize { payload },
        }
    }

    #[test]
    fn full_entry_set_can_swap_verify_restore_and_cleanup() {
        let temp = tempdir().expect("temp");
        let first = existing_destination(temp.path(), "first", b"old first");
        let second = existing_destination(temp.path(), "second", b"old second");
        let payload = payload();
        let mut entries = stage_entry_set(&[
            intent(temp.path(), first.clone(), payload.clone()),
            intent(temp.path(), second.clone(), payload),
        ])
        .expect("stage");
        let recovery_paths = planned_recovery_paths(&entries);
        assert_eq!(recovery_paths.len(), 2);
        assert!(recovery_paths.iter().all(|path| {
            path.backup.parent() == path.destination.parent() && !path.backup.exists()
        }));

        swap_entry_set(&mut entries).expect("swap");
        verify_entry_set(&entries).expect("verify");
        assert_eq!(fs::read(first.join("SKILL.md")).unwrap(), b"new skill");
        assert_eq!(
            fs::read(second.join("scripts/run.sh")).unwrap(),
            b"#!/bin/sh\n"
        );

        restore_entry_set(&mut entries).expect("restore");
        assert_eq!(fs::read(first.join("SKILL.md")).unwrap(), b"old first");
        assert_eq!(fs::read(second.join("SKILL.md")).unwrap(), b"old second");
        assert!(cleanup_entry_set(entries).expect("cleanup").is_empty());
    }

    #[test]
    fn stale_target_is_detected_for_the_whole_set_before_any_swap() {
        let temp = tempdir().expect("temp");
        let first = existing_destination(temp.path(), "first", b"old first");
        let second = existing_destination(temp.path(), "second", b"old second");
        let payload = payload();
        let mut entries = stage_entry_set(&[
            intent(temp.path(), first.clone(), payload.clone()),
            intent(temp.path(), second.clone(), payload),
        ])
        .expect("stage");
        fs::write(second.join("external.txt"), b"changed").expect("external change");

        assert!(matches!(
            swap_entry_set(&mut entries),
            Err(AppError::StaleTarget)
        ));
        assert_eq!(fs::read(first.join("SKILL.md")).unwrap(), b"old first");
        assert_eq!(fs::read(second.join("SKILL.md")).unwrap(), b"old second");
        cleanup_entry_set(entries).expect("cleanup stages");
    }

    #[test]
    fn existing_child_content_change_is_detected_before_swap() {
        let temp = tempdir().expect("temp");
        let destination = existing_destination(temp.path(), "demo", b"old skill");
        let mut entries =
            stage_entry_set(&[intent(temp.path(), destination.clone(), payload())]).expect("stage");
        fs::write(destination.join("SKILL.md"), b"locally changed").expect("local change");

        assert!(matches!(
            swap_entry_set(&mut entries),
            Err(AppError::StaleTarget)
        ));
        assert_eq!(
            fs::read(destination.join("SKILL.md")).expect("preserved destination"),
            b"locally changed"
        );
        cleanup_entry_set(entries).expect("cleanup stages");
    }

    #[test]
    fn tampered_stage_is_rejected_before_destination_or_backup_changes() {
        let temp = tempdir().expect("temp");
        let destination = existing_destination(temp.path(), "demo", b"old skill");
        let mut entries =
            stage_entry_set(&[intent(temp.path(), destination.clone(), payload())]).expect("stage");
        let stage = entries.entries[0]
            .staged_path
            .as_ref()
            .expect("materialized stage");
        fs::write(stage.join("unexpected.txt"), b"tampered").expect("tamper stage");

        assert!(swap_entry_set(&mut entries).is_err());
        assert_eq!(
            fs::read(destination.join("SKILL.md")).expect("original destination"),
            b"old skill"
        );
        assert!(!entries.entries[0].backup_path.exists());
        cleanup_entry_set(entries).expect("cleanup stage");
    }

    #[test]
    fn committed_cleanup_removes_backups_and_keeps_new_entries() {
        let temp = tempdir().expect("temp");
        let destination = existing_destination(temp.path(), "demo", b"old");
        let mut entries =
            stage_entry_set(&[intent(temp.path(), destination.clone(), payload())]).expect("stage");
        swap_entry_set(&mut entries).expect("swap");
        verify_entry_set(&entries).expect("verify");

        assert!(cleanup_entry_set(entries).expect("cleanup").is_empty());
        assert_eq!(
            fs::read(destination.join("SKILL.md")).unwrap(),
            b"new skill"
        );
        assert_eq!(
            fs::read_dir(temp.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".skill-deck-"))
                .count(),
            0
        );
    }

    #[test]
    fn post_swap_verification_rejects_tampered_payload_content() {
        let temp = tempdir().expect("temp");
        let destination = existing_destination(temp.path(), "demo", b"old");
        let mut entries =
            stage_entry_set(&[intent(temp.path(), destination.clone(), payload())]).expect("stage");
        swap_entry_set(&mut entries).expect("swap");
        fs::write(destination.join("SKILL.md"), b"tampered").expect("tamper installed payload");

        assert!(matches!(
            verify_entry_set(&entries),
            Err(AppError::ExecutionFailed { .. })
        ));
        restore_entry_set(&mut entries).expect("restore");
        cleanup_entry_set(entries).expect("cleanup");
    }

    #[test]
    fn swap_rejects_a_backup_path_claimed_after_staging() {
        let temp = tempdir().expect("temp");
        let destination = existing_destination(temp.path(), "demo", b"old");
        let mut entries =
            stage_entry_set(&[intent(temp.path(), destination.clone(), payload())]).expect("stage");
        let backup = entries.entries[0].backup_path.clone();
        fs::write(&backup, b"external backup").expect("claim backup");

        assert!(matches!(
            swap_entry_set(&mut entries),
            Err(AppError::StaleTarget)
        ));
        assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), b"old");
        cleanup_entry_set(entries).expect("cleanup");
        fs::remove_file(backup).expect("remove external backup");
    }

    #[test]
    fn stage_creates_a_missing_configured_root_without_making_it_the_mutation_target() {
        let temp = tempdir().expect("temp");
        let destination = temp.path().join(".custom/skills/demo");
        let target = projected_physical_target_key(
            ExecutionBackend::NativeUnix,
            physical_parent_identity(temp.path()).expect("ancestor identity"),
            [".custom", "skills", "demo"],
            true,
        )
        .unwrap();
        let intent = NativeEntryIntent {
            target,
            destination: destination.clone(),
            expected_fingerprint: inspect_entry_no_follow(&destination).unwrap().fingerprint,
            expected_content_manifest_hash: None,
            action: NativeEntryAction::Materialize { payload: payload() },
        };

        let mut entries = stage_entry_set(&[intent]).expect("stage missing root");
        assert!(destination.parent().unwrap().is_dir());
        assert!(!destination.exists());

        swap_entry_set(&mut entries).expect("swap");
        verify_entry_set(&entries).expect("verify");
        assert!(destination.join("SKILL.md").is_file());
        cleanup_entry_set(entries).expect("cleanup");
        assert!(destination.parent().unwrap().is_dir());
    }
}
