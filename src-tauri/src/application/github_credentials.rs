use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;
use specta::Type;

use crate::core::{GithubApiClient, GithubTokenProvider, GithubTokenValidation};

pub type GithubCredentialFuture<'a> =
    Pin<Box<dyn Future<Output = GithubTokenValidation> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubCredentialStoreError {
    Unavailable,
}

pub trait GithubCredentialStore: Send + Sync {
    fn read(&self) -> Result<Option<String>, GithubCredentialStoreError>;
    fn write(&self, token: &str) -> Result<(), GithubCredentialStoreError>;
    fn delete(&self) -> Result<(), GithubCredentialStoreError>;
}

pub trait GithubTokenValidator: Send + Sync {
    fn validate<'a>(&'a self, token: &'a str) -> GithubCredentialFuture<'a>;
}

impl GithubTokenValidator for GithubApiClient {
    fn validate<'a>(&'a self, token: &'a str) -> GithubCredentialFuture<'a> {
        Box::pin(async move { self.validate_token(token).await })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum GithubCredentialSource {
    Keyring,
    GithubTokenEnv,
    GhTokenEnv,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum GithubCredentialStorageStatus {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum GithubCredentialValidationStatus {
    Unconfigured,
    Verified,
    Invalid,
    RateLimited,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct GithubCredentialStatus {
    pub source: GithubCredentialSource,
    pub storage: GithubCredentialStorageStatus,
    pub validation: GithubCredentialValidationStatus,
    pub account: Option<String>,
    pub rate_limit_remaining: Option<u64>,
    pub rate_limit_limit: Option<u64>,
    pub rate_limit_reset_at_epoch_ms: Option<u64>,
    pub retry_at_epoch_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct GithubCredentialSaveResult {
    pub saved: bool,
    pub status: GithubCredentialStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct GithubCredentialClearResult {
    pub cleared: bool,
    pub status: GithubCredentialStatus,
}

type EnvironmentTokenResolver = dyn Fn() -> Option<(GithubCredentialSource, String)> + Send + Sync;

pub fn resolve_environment_github_token() -> Option<(GithubCredentialSource, String)> {
    for (name, source) in [
        ("GITHUB_TOKEN", GithubCredentialSource::GithubTokenEnv),
        ("GH_TOKEN", GithubCredentialSource::GhTokenEnv),
    ] {
        if let Ok(token) = std::env::var(name) {
            if !token.trim().is_empty() {
                return Some((source, token));
            }
        }
    }
    None
}

#[derive(Clone)]
pub struct GithubCredentialService {
    store: Arc<dyn GithubCredentialStore>,
    validator: Arc<dyn GithubTokenValidator>,
    environment_token: Arc<EnvironmentTokenResolver>,
}

impl GithubCredentialService {
    pub fn new(
        store: Arc<dyn GithubCredentialStore>,
        validator: Arc<dyn GithubTokenValidator>,
        environment_token: Arc<EnvironmentTokenResolver>,
    ) -> Self {
        Self {
            store,
            validator,
            environment_token,
        }
    }

    pub async fn status(&self) -> GithubCredentialStatus {
        let (source, token, storage) = self.resolve_active_token();
        let Some(token) = token else {
            return empty_status(
                source,
                storage,
                GithubCredentialValidationStatus::Unconfigured,
            );
        };
        status_from_validation(source, storage, self.validator.validate(&token).await)
    }

    pub async fn save(&self, token: &str) -> GithubCredentialSaveResult {
        let token = token.trim();
        if token.is_empty() {
            return GithubCredentialSaveResult {
                saved: false,
                status: empty_status(
                    GithubCredentialSource::None,
                    self.storage_status(),
                    GithubCredentialValidationStatus::Invalid,
                ),
            };
        }

        let validation = self.validator.validate(token).await;
        let mut status = status_from_validation(
            GithubCredentialSource::None,
            self.storage_status(),
            validation.clone(),
        );
        if !matches!(validation, GithubTokenValidation::Verified { .. }) {
            return GithubCredentialSaveResult {
                saved: false,
                status,
            };
        }
        if self.store.write(token).is_err() {
            status.storage = GithubCredentialStorageStatus::Unavailable;
            return GithubCredentialSaveResult {
                saved: false,
                status,
            };
        }
        status.source = GithubCredentialSource::Keyring;
        status.storage = GithubCredentialStorageStatus::Available;
        GithubCredentialSaveResult {
            saved: true,
            status,
        }
    }

    pub async fn clear(&self) -> GithubCredentialClearResult {
        let cleared = self.store.delete().is_ok();
        GithubCredentialClearResult {
            cleared,
            status: self.status().await,
        }
    }

    pub fn resolved_token(&self) -> Option<String> {
        self.resolve_active_token().1
    }

    fn resolve_active_token(
        &self,
    ) -> (
        GithubCredentialSource,
        Option<String>,
        GithubCredentialStorageStatus,
    ) {
        match self.store.read() {
            Ok(Some(token)) if !token.trim().is_empty() => (
                GithubCredentialSource::Keyring,
                Some(token),
                GithubCredentialStorageStatus::Available,
            ),
            Ok(_) => {
                let environment = (self.environment_token)();
                (
                    environment
                        .as_ref()
                        .map_or(GithubCredentialSource::None, |(source, _)| *source),
                    environment.map(|(_, token)| token),
                    GithubCredentialStorageStatus::Available,
                )
            }
            Err(_) => {
                let environment = (self.environment_token)();
                (
                    environment
                        .as_ref()
                        .map_or(GithubCredentialSource::None, |(source, _)| *source),
                    environment.map(|(_, token)| token),
                    GithubCredentialStorageStatus::Unavailable,
                )
            }
        }
    }

    fn storage_status(&self) -> GithubCredentialStorageStatus {
        if self.store.read().is_ok() {
            GithubCredentialStorageStatus::Available
        } else {
            GithubCredentialStorageStatus::Unavailable
        }
    }
}

impl GithubTokenProvider for GithubCredentialService {
    fn token(&self) -> Option<String> {
        self.resolved_token()
    }
}

fn empty_status(
    source: GithubCredentialSource,
    storage: GithubCredentialStorageStatus,
    validation: GithubCredentialValidationStatus,
) -> GithubCredentialStatus {
    GithubCredentialStatus {
        source,
        storage,
        validation,
        account: None,
        rate_limit_remaining: None,
        rate_limit_limit: None,
        rate_limit_reset_at_epoch_ms: None,
        retry_at_epoch_ms: None,
    }
}

fn status_from_validation(
    source: GithubCredentialSource,
    storage: GithubCredentialStorageStatus,
    validation: GithubTokenValidation,
) -> GithubCredentialStatus {
    match validation {
        GithubTokenValidation::Verified {
            login,
            rate_limit_remaining,
            rate_limit_limit,
            rate_limit_reset_at_epoch_ms,
        } => GithubCredentialStatus {
            source,
            storage,
            validation: GithubCredentialValidationStatus::Verified,
            account: Some(login),
            rate_limit_remaining,
            rate_limit_limit,
            rate_limit_reset_at_epoch_ms,
            retry_at_epoch_ms: None,
        },
        GithubTokenValidation::Invalid => {
            empty_status(source, storage, GithubCredentialValidationStatus::Invalid)
        }
        GithubTokenValidation::RateLimited { retry_at_epoch_ms } => GithubCredentialStatus {
            retry_at_epoch_ms,
            ..empty_status(
                source,
                storage,
                GithubCredentialValidationStatus::RateLimited,
            )
        },
        GithubTokenValidation::Unavailable => empty_status(
            source,
            storage,
            GithubCredentialValidationStatus::Unavailable,
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Default)]
    struct MemoryStore {
        token: Mutex<Option<String>>,
        writes: Mutex<Vec<String>>,
        unavailable: bool,
    }

    impl GithubCredentialStore for MemoryStore {
        fn read(&self) -> Result<Option<String>, GithubCredentialStoreError> {
            if self.unavailable {
                return Err(GithubCredentialStoreError::Unavailable);
            }
            Ok(self.token.lock().unwrap().clone())
        }

        fn write(&self, token: &str) -> Result<(), GithubCredentialStoreError> {
            if self.unavailable {
                return Err(GithubCredentialStoreError::Unavailable);
            }
            self.writes.lock().unwrap().push(token.to_string());
            *self.token.lock().unwrap() = Some(token.to_string());
            Ok(())
        }

        fn delete(&self) -> Result<(), GithubCredentialStoreError> {
            if self.unavailable {
                return Err(GithubCredentialStoreError::Unavailable);
            }
            *self.token.lock().unwrap() = None;
            Ok(())
        }
    }

    struct Validator {
        results: Mutex<VecDeque<GithubTokenValidation>>,
        tokens: Mutex<Vec<String>>,
    }

    impl GithubTokenValidator for Validator {
        fn validate<'a>(&'a self, token: &'a str) -> GithubCredentialFuture<'a> {
            self.tokens.lock().unwrap().push(token.to_string());
            Box::pin(async move { self.results.lock().unwrap().pop_front().unwrap() })
        }
    }

    fn validator(results: Vec<GithubTokenValidation>) -> Arc<Validator> {
        Arc::new(Validator {
            results: Mutex::new(results.into()),
            tokens: Mutex::new(Vec::new()),
        })
    }

    fn valid(login: &str) -> GithubTokenValidation {
        GithubTokenValidation::Verified {
            login: login.to_string(),
            rate_limit_remaining: Some(4_999),
            rate_limit_limit: Some(5_000),
            rate_limit_reset_at_epoch_ms: Some(2_000),
        }
    }

    #[tokio::test]
    async fn valid_token_is_verified_before_keyring_write_and_never_returned() {
        let store = Arc::new(MemoryStore::default());
        let validator = validator(vec![valid("octocat")]);
        let service =
            GithubCredentialService::new(store.clone(), validator.clone(), Arc::new(|| None));

        let result = service.save(" github_pat_secret ").await;

        assert!(result.saved);
        assert_eq!(
            store.writes.lock().unwrap().as_slice(),
            ["github_pat_secret"]
        );
        assert_eq!(
            validator.tokens.lock().unwrap().as_slice(),
            ["github_pat_secret"]
        );
        assert_eq!(result.status.source, GithubCredentialSource::Keyring);
        assert_eq!(
            result.status.validation,
            GithubCredentialValidationStatus::Verified
        );
        assert_eq!(result.status.account.as_deref(), Some("octocat"));
        let encoded = serde_json::to_string(&result).unwrap();
        assert!(!encoded.contains("github_pat_secret"));
    }

    #[tokio::test]
    async fn invalid_token_does_not_overwrite_existing_keyring_credential() {
        let store = Arc::new(MemoryStore {
            token: Mutex::new(Some("existing-token".to_string())),
            ..MemoryStore::default()
        });
        let service = GithubCredentialService::new(
            store.clone(),
            validator(vec![GithubTokenValidation::Invalid]),
            Arc::new(|| None),
        );

        let result = service.save("invalid-token").await;

        assert!(!result.saved);
        assert_eq!(
            result.status.validation,
            GithubCredentialValidationStatus::Invalid
        );
        assert_eq!(
            store.token.lock().unwrap().as_deref(),
            Some("existing-token")
        );
        assert!(store.writes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unavailable_keyring_fails_closed_after_successful_validation() {
        let store = Arc::new(MemoryStore {
            unavailable: true,
            ..MemoryStore::default()
        });
        let service = GithubCredentialService::new(
            store,
            validator(vec![valid("octocat")]),
            Arc::new(|| None),
        );

        let result = service.save("valid-token").await;

        assert!(!result.saved);
        assert_eq!(
            result.status.storage,
            GithubCredentialStorageStatus::Unavailable
        );
        assert_eq!(result.status.source, GithubCredentialSource::None);
    }

    #[tokio::test]
    async fn environment_token_remains_available_when_keyring_is_unavailable() {
        let store = Arc::new(MemoryStore {
            unavailable: true,
            ..MemoryStore::default()
        });
        let service = GithubCredentialService::new(
            store,
            validator(vec![valid("env-user")]),
            Arc::new(|| {
                Some((
                    GithubCredentialSource::GithubTokenEnv,
                    "env-token".to_string(),
                ))
            }),
        );

        let status = service.status().await;

        assert_eq!(status.storage, GithubCredentialStorageStatus::Unavailable);
        assert_eq!(status.source, GithubCredentialSource::GithubTokenEnv);
        assert_eq!(
            status.validation,
            GithubCredentialValidationStatus::Verified
        );
        assert_eq!(status.account.as_deref(), Some("env-user"));
    }

    #[tokio::test]
    async fn clearing_keyring_credential_reveals_environment_fallback() {
        let store = Arc::new(MemoryStore {
            token: Mutex::new(Some("stored-token".to_string())),
            ..MemoryStore::default()
        });
        let service = GithubCredentialService::new(
            store.clone(),
            validator(vec![valid("env-user")]),
            Arc::new(|| Some((GithubCredentialSource::GhTokenEnv, "env-token".to_string()))),
        );

        let result = service.clear().await;

        assert!(result.cleared);
        assert!(store.token.lock().unwrap().is_none());
        assert_eq!(result.status.source, GithubCredentialSource::GhTokenEnv);
        assert_eq!(
            result.status.validation,
            GithubCredentialValidationStatus::Verified
        );
    }
}
