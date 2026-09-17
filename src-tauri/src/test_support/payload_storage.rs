use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Notify, Semaphore};

use crate::application::payload_session::{
    PayloadSessionStorage, PayloadStorageFuture, PayloadStorageKey,
};
use crate::core::skill_payload::{SkillPayload, SkillPayloadManifest};
use crate::error::AppError;

pub(crate) struct PayloadIoPause {
    started: Notify,
    resume: Semaphore,
}

impl Default for PayloadIoPause {
    fn default() -> Self {
        Self {
            started: Notify::new(),
            resume: Semaphore::new(0),
        }
    }
}

impl PayloadIoPause {
    pub async fn wait_until_started(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.started.notified())
            .await
            .expect("payload I/O did not start");
    }

    pub fn resume(&self) {
        self.resume.add_permits(1);
    }

    pub async fn pause(&self) {
        self.started.notify_one();
        tokio::time::timeout(Duration::from_secs(5), self.resume.acquire())
            .await
            .expect("payload I/O was not resumed")
            .expect("resume semaphore")
            .forget();
    }
}

pub(crate) struct PausedPayloadStorage {
    pub inner: Arc<dyn PayloadSessionStorage>,
    pub store_pause: PayloadIoPause,
    pub verify_pause: Option<PayloadIoPause>,
    pub stores: AtomicUsize,
    pub fail_after_store: AtomicBool,
}

impl PausedPayloadStorage {
    pub fn new(inner: Arc<dyn PayloadSessionStorage>) -> Self {
        Self {
            inner,
            store_pause: PayloadIoPause::default(),
            verify_pause: None,
            stores: AtomicUsize::new(0),
            fail_after_store: AtomicBool::new(false),
        }
    }
}

impl PayloadSessionStorage for PausedPayloadStorage {
    fn store<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        payload: SkillPayload,
    ) -> PayloadStorageFuture<'a, Result<u64, AppError>> {
        Box::pin(async move {
            self.stores.fetch_add(1, Ordering::SeqCst);
            self.store_pause.pause().await;
            let bytes = self.inner.store(key, payload).await?;
            if self.fail_after_store.swap(false, Ordering::SeqCst) {
                return Err(AppError::Io {
                    message: "injected store failure".to_string(),
                });
            }
            Ok(bytes)
        })
    }

    fn verify<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<Option<SkillPayloadManifest>, AppError>> {
        Box::pin(async move {
            if let Some(pause) = &self.verify_pause {
                pause.pause().await;
            }
            self.inner.verify(key).await
        })
    }

    fn read_blob<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        blob_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<Option<Vec<u8>>, AppError>> {
        self.inner.read_blob(key, blob_id)
    }

    fn remove<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
        self.inner.remove(key)
    }

    fn remove_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
        self.inner.remove_session(session_id)
    }
}
