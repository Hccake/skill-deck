use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use environment_engine::payload::{
    build_payload_with_cancel, read_blob, verify_payload, BuiltPayload,
    PayloadEntryKind as EngineEntryKind, PayloadError as EngineError,
    PayloadManifest as EngineManifest,
};
use environment_protocol::{
    PayloadCleanupResponse, PayloadCleanupWarning, PayloadEntry, PayloadEntryKind, PayloadManifest,
    PayloadReadyResponse,
};

const OWNER_FILE: &str = ".skill-deck-owner";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedPayload {
    pub id: u64,
    pub root: PathBuf,
    pub manifest: PayloadManifest,
    pub total_bytes: u64,
    pub computed_hash: Option<String>,
}

impl ManagedPayload {
    pub fn into_response(self) -> PayloadReadyResponse {
        PayloadReadyResponse {
            payload_id: self.id,
            manifest: self.manifest,
            total_bytes: self.total_bytes,
            computed_hash: self.computed_hash,
        }
    }
}

#[derive(Debug)]
pub enum PayloadError {
    InvalidBase,
    InvalidSession,
    InvalidPayloadName,
    MissingPayload,
    StalePayload,
    Engine(EngineError),
    Json(serde_json::Error),
    Io(std::io::Error),
}

impl fmt::Display for PayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PayloadError {}

impl From<std::io::Error> for PayloadError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<EngineError> for PayloadError {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}

impl From<serde_json::Error> for PayloadError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

struct PayloadRecord {
    root: PathBuf,
}

pub struct PayloadManager {
    base: PathBuf,
    next_id: u64,
    payloads: HashMap<u64, PayloadRecord>,
    uploads: HashMap<u64, UploadRecord>,
}

struct UploadRecord {
    stage: PathBuf,
    final_root: PathBuf,
}

pub struct PreparedPayloadFile {
    pub path: PathBuf,
    pub file: fs::File,
}

impl PayloadManager {
    pub fn new(base: PathBuf) -> Result<Self, PayloadError> {
        if !base.is_absolute() {
            return Err(PayloadError::InvalidBase);
        }
        fs::create_dir_all(&base)?;
        let base = fs::canonicalize(base)?;
        if !base.is_dir() {
            return Err(PayloadError::InvalidBase);
        }
        Ok(Self {
            base,
            next_id: 1,
            payloads: HashMap::new(),
            uploads: HashMap::new(),
        })
    }

    pub fn acquire_from_source(
        &mut self,
        session_id: &str,
        payload_name: &str,
        source_root: &Path,
    ) -> Result<ManagedPayload, PayloadError> {
        self.acquire_from_source_with_cancel(session_id, payload_name, source_root, || false)
    }

    pub fn acquire_from_source_with_cancel<F>(
        &mut self,
        session_id: &str,
        payload_name: &str,
        source_root: &Path,
        is_cancelled: F,
    ) -> Result<ManagedPayload, PayloadError>
    where
        F: Fn() -> bool,
    {
        let session_root = self.ensure_session(session_id)?;
        validate_payload_name(payload_name)?;
        let payload_root = session_root.join(payload_name);
        if fs::symlink_metadata(&payload_root).is_ok() {
            return Err(PayloadError::StalePayload);
        }
        let id = self.allocate_id()?;
        let stage = session_root.join(format!(".stage-{payload_name}-{id}"));
        if fs::symlink_metadata(&stage).is_ok() {
            return Err(PayloadError::StalePayload);
        }
        let built = build_payload_with_cancel(source_root, &stage, is_cancelled)?;
        if let Err(error) = fs::rename(&stage, &payload_root) {
            let _ = fs::remove_dir_all(&stage);
            return Err(error.into());
        }
        Ok(self.register_built(id, payload_root, built))
    }

    pub fn verify(
        &mut self,
        session_id: &str,
        payload_name: &str,
    ) -> Result<Option<ManagedPayload>, PayloadError> {
        let session_root = self.session_root(session_id)?;
        validate_payload_name(payload_name)?;
        match fs::symlink_metadata(&session_root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => self.validate_session(&session_root, session_id)?,
        }
        let payload_root = session_root.join(payload_name);
        match fs::symlink_metadata(&payload_root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let manifest = verify_payload(&payload_root)?;
        let total_bytes = unique_total_bytes(&manifest);
        let id = self.allocate_id()?;
        self.payloads.insert(
            id,
            PayloadRecord {
                root: payload_root.clone(),
            },
        );
        Ok(Some(ManagedPayload {
            id,
            root: payload_root,
            manifest: map_manifest(manifest),
            total_bytes,
            computed_hash: None,
        }))
    }

    pub fn begin_upload(
        &mut self,
        session_id: &str,
        payload_name: &str,
    ) -> Result<u64, PayloadError> {
        let session_root = self.ensure_session(session_id)?;
        validate_payload_name(payload_name)?;
        let final_root = session_root.join(payload_name);
        let stage = session_root.join(format!("{payload_name}.upload"));
        if fs::symlink_metadata(&final_root).is_ok() || fs::symlink_metadata(&stage).is_ok() {
            return Err(PayloadError::StalePayload);
        }
        fs::create_dir(&stage)?;
        set_private_directory(&stage)?;
        fs::create_dir(stage.join("blobs"))?;
        let upload_id = self.allocate_id()?;
        self.uploads
            .insert(upload_id, UploadRecord { stage, final_root });
        Ok(upload_id)
    }

    pub fn prepare_blob(
        &self,
        upload_id: u64,
        blob_id: &str,
    ) -> Result<PreparedPayloadFile, PayloadError> {
        if !valid_blob_id(blob_id) {
            return Err(PayloadError::StalePayload);
        }
        let upload = self
            .uploads
            .get(&upload_id)
            .ok_or(PayloadError::StalePayload)?;
        let path = upload.stage.join(format!(".incoming-blob-{blob_id}"));
        prepare_file(path)
    }

    pub fn commit_blob(
        &self,
        upload_id: u64,
        blob_id: &str,
        incoming: PathBuf,
    ) -> Result<(), PayloadError> {
        let upload = self
            .uploads
            .get(&upload_id)
            .ok_or(PayloadError::StalePayload)?;
        let expected = upload.stage.join(format!(".incoming-blob-{blob_id}"));
        if incoming != expected || file_sha256(&incoming)? != blob_id {
            return Err(PayloadError::StalePayload);
        }
        let destination = upload.stage.join("blobs").join(blob_id);
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(PayloadError::StalePayload);
        }
        fs::rename(incoming, &destination)?;
        set_private_file(&destination)?;
        Ok(())
    }

    pub fn prepare_manifest(&self, upload_id: u64) -> Result<PreparedPayloadFile, PayloadError> {
        let upload = self
            .uploads
            .get(&upload_id)
            .ok_or(PayloadError::StalePayload)?;
        prepare_file(upload.stage.join(".incoming-manifest"))
    }

    pub fn finalize_upload(
        &mut self,
        upload_id: u64,
        incoming_manifest: PathBuf,
    ) -> Result<ManagedPayload, PayloadError> {
        let upload = self
            .uploads
            .remove(&upload_id)
            .ok_or(PayloadError::StalePayload)?;
        let result = finalize_upload_record(&upload, incoming_manifest).and_then(|manifest| {
            fs::rename(&upload.stage, &upload.final_root)?;
            let id = self.allocate_id()?;
            let total_bytes = unique_total_bytes(&manifest);
            self.payloads.insert(
                id,
                PayloadRecord {
                    root: upload.final_root.clone(),
                },
            );
            Ok(ManagedPayload {
                id,
                root: upload.final_root.clone(),
                manifest: map_manifest(manifest),
                total_bytes,
                computed_hash: None,
            })
        });
        if result.is_err() {
            let _ = fs::remove_dir_all(&upload.stage);
        }
        result
    }

    pub fn abort_upload(&mut self, upload_id: u64) {
        if let Some(upload) = self.uploads.remove(&upload_id) {
            let _ = fs::remove_dir_all(upload.stage);
        }
    }

    pub fn read_blob(
        &self,
        payload_id: u64,
        blob_id: &str,
    ) -> Result<Option<fs::File>, PayloadError> {
        let payload = self
            .payloads
            .get(&payload_id)
            .ok_or(PayloadError::MissingPayload)?;
        read_blob(&payload.root, blob_id).map_err(Into::into)
    }

    pub fn payload_root(&self, payload_id: u64) -> Result<PathBuf, PayloadError> {
        self.payloads
            .get(&payload_id)
            .map(|payload| payload.root.clone())
            .ok_or(PayloadError::MissingPayload)
    }

    pub fn remove(&mut self, session_id: &str, payload_name: &str) -> Result<(), PayloadError> {
        let session_root = self.session_root(session_id)?;
        validate_payload_name(payload_name)?;
        match fs::symlink_metadata(&session_root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
            Ok(_) => self.validate_session(&session_root, session_id)?,
        }
        let payload_root = session_root.join(payload_name);
        let upload_ids = self
            .uploads
            .iter()
            .filter_map(|(upload_id, upload)| {
                (upload.final_root == payload_root).then_some(*upload_id)
            })
            .collect::<Vec<_>>();
        for upload_id in upload_ids {
            self.abort_upload(upload_id);
        }
        match fs::symlink_metadata(&payload_root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                fs::remove_dir_all(&payload_root)?;
            }
            Ok(_) => return Err(PayloadError::StalePayload),
        }
        self.payloads
            .retain(|_, record| record.root != payload_root);
        Ok(())
    }

    pub fn remove_session(&mut self, session_id: &str) -> Result<(), PayloadError> {
        let session_root = self.session_root(session_id)?;
        match fs::symlink_metadata(&session_root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
            Ok(_) => self.validate_session(&session_root, session_id)?,
        }
        fs::remove_dir_all(&session_root)?;
        self.payloads
            .retain(|_, record| !record.root.starts_with(&session_root));
        self.uploads
            .retain(|_, upload| !upload.final_root.starts_with(&session_root));
        Ok(())
    }

    pub fn sweep_orphans(
        &mut self,
        protected_session_ids: &[String],
    ) -> Result<PayloadCleanupResponse, PayloadError> {
        let protected = protected_session_ids.iter().collect::<HashSet<_>>();
        let mut report = PayloadCleanupResponse {
            removed_sessions: 0,
            protected_sessions: 0,
            retained_external_bytes: 0,
            cleanup_blocked: false,
            warnings: Vec::new(),
        };
        for entry in fs::read_dir(&self.base)?.collect::<Result<Vec<_>, _>>()? {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(session_id) = name.strip_prefix("skill-deck-source-") else {
                continue;
            };
            let path = entry.path();
            if self.validate_session(&path, session_id).is_err() {
                retain_external(&mut report, &path, &name, "invalidMarker");
                continue;
            }
            if protected.contains(&session_id.to_string()) {
                report.protected_sessions = report.protected_sessions.saturating_add(1);
                continue;
            }
            match fs::remove_dir_all(&path) {
                Ok(()) => {
                    report.removed_sessions = report.removed_sessions.saturating_add(1);
                    self.payloads
                        .retain(|_, record| !record.root.starts_with(&path));
                }
                Err(error) => retain_external_with_details(
                    &mut report,
                    &path,
                    &name,
                    "deleteFailed",
                    Some(error.to_string()),
                ),
            }
        }
        Ok(report)
    }

    fn ensure_session(&self, session_id: &str) -> Result<PathBuf, PayloadError> {
        let session_root = self.session_root(session_id)?;
        match fs::symlink_metadata(&session_root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&session_root)?;
                set_private_directory(&session_root)?;
                fs::write(session_root.join(OWNER_FILE), format!("1\n{session_id}\n"))?;
            }
            Err(error) => return Err(error.into()),
            Ok(_) => self.validate_session(&session_root, session_id)?,
        }
        Ok(session_root)
    }

    fn session_root(&self, session_id: &str) -> Result<PathBuf, PayloadError> {
        if session_id.is_empty()
            || session_id.len() > 128
            || !session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(PayloadError::InvalidSession);
        }
        Ok(self.base.join(format!("skill-deck-source-{session_id}")))
    }

    fn validate_session(&self, root: &Path, session_id: &str) -> Result<(), PayloadError> {
        let metadata = fs::symlink_metadata(root)?;
        let marker = root.join(OWNER_FILE);
        let marker_metadata = fs::symlink_metadata(&marker)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || !marker_metadata.is_file()
            || marker_metadata.file_type().is_symlink()
            || fs::read_to_string(marker)? != format!("1\n{session_id}\n")
        {
            return Err(PayloadError::StalePayload);
        }
        Ok(())
    }

    fn allocate_id(&mut self) -> Result<u64, PayloadError> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(PayloadError::StalePayload)?;
        Ok(id)
    }

    fn register_built(&mut self, id: u64, root: PathBuf, built: BuiltPayload) -> ManagedPayload {
        self.payloads
            .insert(id, PayloadRecord { root: root.clone() });
        ManagedPayload {
            id,
            root,
            manifest: map_manifest(built.manifest),
            total_bytes: built.total_bytes,
            computed_hash: Some(built.computed_hash),
        }
    }
}

impl Drop for PayloadManager {
    fn drop(&mut self) {
        for upload in self.uploads.values() {
            let _ = fs::remove_dir_all(&upload.stage);
        }
    }
}

fn prepare_file(path: PathBuf) -> Result<PreparedPayloadFile, PayloadError> {
    if fs::symlink_metadata(&path).is_ok() {
        return Err(PayloadError::StalePayload);
    }
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    set_private_file(&path)?;
    Ok(PreparedPayloadFile { path, file })
}

fn finalize_upload_record(
    upload: &UploadRecord,
    incoming_manifest: PathBuf,
) -> Result<EngineManifest, PayloadError> {
    if incoming_manifest != upload.stage.join(".incoming-manifest") {
        return Err(PayloadError::StalePayload);
    }
    let manifest: EngineManifest = serde_json::from_reader(fs::File::open(&incoming_manifest)?)?;
    let expected_blobs = manifest
        .entries
        .iter()
        .filter_map(|entry| entry.blob_id.clone())
        .collect::<BTreeSet<_>>();
    let actual_blobs = fs::read_dir(upload.stage.join("blobs"))?
        .map(|entry| {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if !metadata.is_file() || entry.file_type()?.is_symlink() {
                return Err(std::io::Error::other("payload blob is not a regular file"));
            }
            Ok(entry.file_name().to_string_lossy().into_owned())
        })
        .collect::<Result<BTreeSet<_>, std::io::Error>>()?;
    if actual_blobs != expected_blobs {
        return Err(PayloadError::StalePayload);
    }
    let mut blob_list = fs::File::create(upload.stage.join("blob-list"))?;
    use std::io::Write;
    for blob_id in &expected_blobs {
        writeln!(blob_list, "{blob_id}")?;
    }
    fs::rename(incoming_manifest, upload.stage.join("manifest.json"))?;
    let verified = verify_payload(&upload.stage)?;
    if verified != manifest {
        return Err(PayloadError::StalePayload);
    }
    Ok(manifest)
}

fn validate_payload_name(payload_name: &str) -> Result<(), PayloadError> {
    if !payload_name.starts_with("payload-")
        || payload_name.len() <= "payload-".len()
        || !payload_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(PayloadError::InvalidPayloadName);
    }
    Ok(())
}

fn valid_blob_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn file_sha256(path: &Path) -> Result<String, PayloadError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(format!("{:x}", hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

fn map_manifest(manifest: EngineManifest) -> PayloadManifest {
    PayloadManifest {
        entries: manifest
            .entries
            .into_iter()
            .map(|entry| PayloadEntry {
                relative_path: entry.relative_path,
                kind: match entry.kind {
                    EngineEntryKind::File => PayloadEntryKind::File,
                    EngineEntryKind::Directory => PayloadEntryKind::Directory,
                },
                blob_id: entry.blob_id,
                content_hash: entry.content_hash,
                size: entry.size,
                executable: entry.executable,
            })
            .collect(),
        payload_root_hash: manifest.payload_root_hash,
        payload_id: manifest.payload_id,
    }
}

fn unique_total_bytes(manifest: &EngineManifest) -> u64 {
    manifest
        .entries
        .iter()
        .filter_map(|entry| entry.blob_id.as_ref().map(|blob| (blob, entry.size)))
        .collect::<std::collections::BTreeMap<_, _>>()
        .values()
        .copied()
        .sum()
}

fn retain_external(report: &mut PayloadCleanupResponse, path: &Path, name: &str, code: &str) {
    retain_external_with_details(report, path, name, code, None);
}

fn retain_external_with_details(
    report: &mut PayloadCleanupResponse,
    path: &Path,
    name: &str,
    code: &str,
    technical_details: Option<String>,
) {
    report.cleanup_blocked = true;
    match directory_size(path) {
        Ok(size) => {
            report.retained_external_bytes = report.retained_external_bytes.saturating_add(size)
        }
        Err(_) => report.warnings.push(PayloadCleanupWarning {
            code: "sizeUnavailable".to_string(),
            candidate_name: Some(name.to_string()),
            technical_details: None,
        }),
    }
    report.warnings.push(PayloadCleanupWarning {
        code: code.to_string(),
        candidate_name: Some(name.to_string()),
        technical_details,
    });
}

fn directory_size(path: &Path) -> Result<u64, std::io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut size = metadata.len();
    for entry in fs::read_dir(path)? {
        size = size.saturating_add(directory_size(&entry?.path())?);
    }
    Ok(size)
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}
