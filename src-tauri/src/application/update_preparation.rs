use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::application::library_update::PreparedLibraryUpdate;
use crate::application::skill_libraries::UpdateLibrarySkillsRequest;
use crate::application::update::PreparedUpdate;
use crate::core::mutation::CancellationSignal;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;

pub enum PreparedUpdates {
    Direct(PreparedUpdate),
    Library(PreparedLibraryUpdate),
}

impl PreparedUpdates {
    fn expire_payloads(&mut self, now: u64) -> Option<u64> {
        match self {
            Self::Direct(value) => {
                value.expire_payloads(now);
                value.expires_at_epoch_ms()
            }
            Self::Library(value) => {
                value.expire_payloads(now);
                value.expires_at_epoch_ms()
            }
        }
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone)]
pub enum PreparationTarget {
    Direct(SkillLocationRef),
    Library(UpdateLibrarySkillsRequest),
}

struct Operation {
    owner: String,
    nonce: uuid::Uuid,
    cancellation: CancellationSignal,
    prepared: Option<PreparedUpdates>,
}

#[derive(Clone, Default)]
pub struct UpdatePreparations {
    operations: Arc<Mutex<BTreeMap<String, Operation>>>,
}

pub struct PreparationTicket {
    registry: UpdatePreparations,
    id: String,
    nonce: uuid::Uuid,
    pub cancellation: CancellationSignal,
    published: bool,
}

impl UpdatePreparations {
    pub fn begin(&self, id: String, owner: &str) -> Result<PreparationTicket, AppError> {
        uuid::Uuid::parse_str(&id).map_err(|_| AppError::Validation {
            field: Some("operationId".into()),
            message: "invalid preparation ID".into(),
        })?;
        let mut operations = self.operations.lock().map_err(|_| AppError::StaleContext)?;
        if operations.contains_key(&id) {
            return Err(AppError::StaleContext);
        }
        operations.retain(|_, operation| {
            let keep = operation.owner != owner;
            if !keep {
                operation.cancellation.cancel();
            }
            keep
        });
        let nonce = uuid::Uuid::new_v4();
        let cancellation = CancellationSignal::default();
        operations.insert(
            id.clone(),
            Operation {
                owner: owner.into(),
                nonce,
                cancellation: cancellation.clone(),
                prepared: None,
            },
        );
        drop(operations);
        Ok(PreparationTicket {
            registry: self.clone(),
            id,
            nonce,
            cancellation,
            published: false,
        })
    }

    pub fn target(&self, id: &str, owner: &str) -> Result<PreparationTarget, AppError> {
        let operations = self.operations.lock().map_err(|_| AppError::StaleContext)?;
        let operation = operations
            .get(id)
            .filter(|operation| operation.owner == owner)
            .ok_or(AppError::StalePayload)?;
        match operation.prepared.as_ref().ok_or(AppError::StalePayload)? {
            PreparedUpdates::Direct(prepared) => {
                Ok(PreparationTarget::Direct(prepared.request.context.clone()))
            }
            PreparedUpdates::Library(prepared) => {
                Ok(PreparationTarget::Library(prepared.request.clone()))
            }
        }
    }

    pub fn take(&self, id: &str, owner: &str) -> Result<PreparedUpdates, AppError> {
        let mut operations = self.operations.lock().map_err(|_| AppError::StaleContext)?;
        operations
            .get(id)
            .filter(|operation| operation.owner == owner && operation.prepared.is_some())
            .ok_or(AppError::StalePayload)?;
        let mut operation = operations.remove(id).ok_or(AppError::StalePayload)?;
        if let Some(prepared) = &mut operation.prepared {
            prepared.expire_payloads(epoch_ms());
        }
        operation.prepared.ok_or(AppError::StalePayload)
    }

    pub fn cancel(&self, id: &str, owner: &str) -> Result<(), AppError> {
        let mut operations = self.operations.lock().map_err(|_| AppError::StaleContext)?;
        if operations
            .get(id)
            .is_some_and(|operation| operation.owner != owner)
        {
            return Err(AppError::StaleContext);
        }
        if let Some(operation) = operations.remove(id) {
            operation.cancellation.cancel();
        }
        Ok(())
    }

    pub fn close_window(&self, owner: &str) {
        if let Ok(mut operations) = self.operations.lock() {
            operations.retain(|_, operation| {
                if operation.owner != owner {
                    return true;
                }
                operation.cancellation.cancel();
                false
            });
        }
    }

    fn discard(&self, id: &str, nonce: uuid::Uuid) {
        if let Ok(mut operations) = self.operations.lock() {
            if operations
                .get(id)
                .is_some_and(|operation| operation.nonce == nonce)
            {
                if let Some(operation) = operations.remove(id) {
                    operation.cancellation.cancel();
                }
            }
        }
    }

    fn schedule_payload_expiry(&self, id: &str, nonce: uuid::Uuid, mut due: u64) {
        let operations = Arc::downgrade(&self.operations);
        let id = id.to_string();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(due.saturating_sub(epoch_ms()).max(1)))
                    .await;
                let next = {
                    let Some(operations) = operations.upgrade() else {
                        return;
                    };
                    let Ok(mut operations) = operations.lock() else {
                        return;
                    };
                    let Some(operation) = operations
                        .get_mut(&id)
                        .filter(|operation| operation.nonce == nonce)
                    else {
                        return;
                    };
                    operation
                        .prepared
                        .as_mut()
                        .and_then(|prepared| prepared.expire_payloads(epoch_ms()))
                };
                let Some(next) = next else {
                    return;
                };
                due = next;
            }
        });
    }
}

impl PreparationTicket {
    pub fn publish(mut self, mut prepared: PreparedUpdates) -> Result<(), AppError> {
        let next_expiry = prepared.expire_payloads(epoch_ms());
        let mut operations = self
            .registry
            .operations
            .lock()
            .map_err(|_| AppError::StaleContext)?;
        let operation = operations
            .get_mut(&self.id)
            .filter(|operation| {
                operation.nonce == self.nonce && !operation.cancellation.is_cancelled()
            })
            .ok_or(AppError::MutationCancelled)?;
        operation.prepared = Some(prepared);
        self.published = true;
        drop(operations);
        if let Some(expiry) = next_expiry {
            self.registry
                .schedule_payload_expiry(&self.id, self.nonce, expiry);
        }
        Ok(())
    }
}

impl Drop for PreparationTicket {
    fn drop(&mut self) {
        if !self.published {
            self.registry.discard(&self.id, self.nonce);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abandoning_preparation_cancels_work_and_releases_the_slot() {
        let registry = UpdatePreparations::default();
        let id = uuid::Uuid::new_v4().to_string();
        let ticket = registry.begin(id.clone(), "main").unwrap();
        let cancellation = ticket.cancellation.clone();
        drop(ticket);
        assert!(cancellation.is_cancelled());
        assert!(registry.begin(id, "main").is_ok());
    }
}
