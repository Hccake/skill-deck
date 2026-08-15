use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::application::payload_session::{DiscoverySessionHandle, PayloadSessionManager};
use crate::core::source_identity::{AcquisitionTransportIdentity, NormalizedRef, SourceIdentity};
use crate::environment::types::{EnvironmentKey, EnvironmentRef};

pub const SOURCE_SNAPSHOT_REUSE_TTL_MS: u64 = 5 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PayloadAcquisitionKey {
    pub acquisition_transport_identity: AcquisitionTransportIdentity,
    pub normalized_ref: NormalizedRef,
    pub environment: EnvironmentKey,
}

impl PayloadAcquisitionKey {
    pub fn new(
        acquisition_transport_identity: AcquisitionTransportIdentity,
        normalized_ref: NormalizedRef,
        environment: &EnvironmentRef,
    ) -> Self {
        Self {
            acquisition_transport_identity,
            normalized_ref,
            environment: EnvironmentKey::from_ref(environment),
        }
    }

    pub fn from_identity(identity: &SourceIdentity, environment: &EnvironmentRef) -> Self {
        Self {
            acquisition_transport_identity: identity.acquisition_transport().clone(),
            normalized_ref: identity.normalized_ref().clone(),
            environment: EnvironmentKey::from_ref(environment),
        }
    }
}

#[derive(Clone)]
pub struct SourceSnapshotReuseIndex {
    entries: Arc<Mutex<HashMap<PayloadAcquisitionKey, SourceSnapshotReuseEntry>>>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

#[derive(Clone)]
struct SourceSnapshotReuseEntry {
    ref_revision: String,
    discovery: DiscoverySessionHandle,
    reuse_deadline_epoch_ms: u64,
}

impl Default for SourceSnapshotReuseIndex {
    fn default() -> Self {
        Self::with_clock(|| chrono::Utc::now().timestamp_millis().max(0) as u64)
    }
}

impl SourceSnapshotReuseIndex {
    pub fn with_clock(now: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            now: Arc::new(now),
        }
    }

    pub fn remember(
        &self,
        key: PayloadAcquisitionKey,
        ref_revision: String,
        discovery: DiscoverySessionHandle,
    ) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(
                key,
                SourceSnapshotReuseEntry {
                    ref_revision,
                    discovery,
                    reuse_deadline_epoch_ms: (self.now)()
                        .saturating_add(SOURCE_SNAPSHOT_REUSE_TTL_MS),
                },
            );
        }
    }

    pub fn find(
        &self,
        key: &PayloadAcquisitionKey,
        ref_revision: &str,
        sessions: &PayloadSessionManager,
    ) -> Option<DiscoverySessionHandle> {
        let mut entries = self.entries.lock().ok()?;
        let entry = entries.get(key)?.clone();
        if entry.reuse_deadline_epoch_ms < (self.now)()
            || sessions.source_snapshot(&entry.discovery).is_err()
        {
            entries.remove(key);
            return None;
        }
        (entry.ref_revision == ref_revision).then_some(entry.discovery)
    }

    pub fn candidate(
        &self,
        key: &PayloadAcquisitionKey,
        sessions: &PayloadSessionManager,
    ) -> Option<(String, DiscoverySessionHandle)> {
        let mut entries = self.entries.lock().ok()?;
        let entry = entries.get(key)?.clone();
        if entry.reuse_deadline_epoch_ms < (self.now)()
            || sessions.source_snapshot(&entry.discovery).is_err()
        {
            entries.remove(key);
            return None;
        }
        Some((entry.ref_revision, entry.discovery))
    }

    pub fn invalidate(&self, key: &PayloadAcquisitionKey) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(key);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use super::*;
    use crate::application::payload_session::{
        DiscoverySessionHandle, DiscoverySourceDescriptor, DiscoverySourceLocation,
        PayloadSessionLimits, RetainedDiscoverySource,
    };
    use crate::core::source_identity::{
        AcquisitionTransport, AcquisitionTransportIdentity, NormalizedRef,
    };
    use crate::environment::types::EnvironmentRef;

    fn key() -> PayloadAcquisitionKey {
        PayloadAcquisitionKey::new(
            AcquisitionTransportIdentity::new(
                AcquisitionTransport::Https,
                "github.com",
                "acme/tools",
            ),
            NormalizedRef::Named("main".to_string()),
            &EnvironmentRef::Native,
        )
    }

    fn retained_source() -> RetainedDiscoverySource {
        RetainedDiscoverySource::new(
            DiscoverySourceLocation::Native {
                root: "/tmp/source".into(),
                ref_revision: None,
            },
            DiscoverySourceDescriptor {
                source: "acme/tools".to_string(),
                source_type: "github".to_string(),
                source_url: Some("https://github.com/acme/tools".to_string()),
                ref_name: Some("main".to_string()),
                redirected_download_host: None,
            },
            BTreeMap::new(),
            (),
        )
    }

    fn limits() -> PayloadSessionLimits {
        PayloadSessionLimits {
            ttl_ms: 60 * 60 * 1_000,
            max_sessions: 8,
            max_bytes: 1024 * 1024,
        }
    }

    #[tokio::test]
    async fn valid_session_handle_is_reused_until_the_five_minute_deadline() {
        let now = Arc::new(AtomicU64::new(1_000));
        let sessions = PayloadSessionManager::in_memory(limits(), {
            let now = now.clone();
            move || now.load(Ordering::SeqCst)
        });
        let handle = sessions
            .discover_with_retained_source(EnvironmentRef::Native, "source", retained_source())
            .await
            .unwrap();
        let index = SourceSnapshotReuseIndex::with_clock({
            let now = now.clone();
            move || now.load(Ordering::SeqCst)
        });
        index.remember(key(), "revision-1".to_string(), handle.clone());

        assert_eq!(
            index
                .find(&key(), "revision-1", &sessions)
                .map(|found| found.session_id),
            Some(handle.session_id.clone())
        );
        now.store(1_000 + SOURCE_SNAPSHOT_REUSE_TTL_MS + 1, Ordering::SeqCst);
        assert!(index.find(&key(), "revision-1", &sessions).is_none());
    }

    #[test]
    fn missing_session_handle_invalidates_only_the_reuse_entry() {
        let sessions = PayloadSessionManager::in_memory(limits(), || 1_000);
        let index = SourceSnapshotReuseIndex::with_clock(|| 1_000);
        let missing = DiscoverySessionHandle {
            session_id: "missing".to_string(),
            environment: EnvironmentRef::Native,
            source_fingerprint: "source".to_string(),
            expires_at_epoch_ms: 10_000,
        };
        index.remember(key(), "revision-1".to_string(), missing);

        assert!(index.find(&key(), "revision-1", &sessions).is_none());
        assert_eq!(index.len(), 0);
    }

    #[tokio::test]
    async fn ref_revision_mismatch_is_a_reuse_miss() {
        let sessions = PayloadSessionManager::in_memory(limits(), || 1_000);
        let handle = sessions
            .discover_with_retained_source(EnvironmentRef::Native, "source", retained_source())
            .await
            .unwrap();
        let index = SourceSnapshotReuseIndex::with_clock(|| 1_000);
        index.remember(key(), "revision-1".to_string(), handle);

        assert!(index.find(&key(), "revision-2", &sessions).is_none());
        assert_eq!(index.len(), 1);
    }

    #[tokio::test]
    async fn candidate_exposes_retained_revision_for_confirm_time_probe() {
        let sessions = PayloadSessionManager::in_memory(limits(), || 1_000);
        let handle = sessions
            .discover_with_retained_source(EnvironmentRef::Native, "source", retained_source())
            .await
            .unwrap();
        let index = SourceSnapshotReuseIndex::with_clock(|| 1_000);
        index.remember(key(), "revision-1".to_string(), handle.clone());

        assert_eq!(
            index
                .candidate(&key(), &sessions)
                .map(|(revision, discovery)| (revision, discovery.session_id)),
            Some(("revision-1".to_string(), handle.session_id))
        );
    }
}
