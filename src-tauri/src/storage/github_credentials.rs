use crate::application::github_credentials::{GithubCredentialStore, GithubCredentialStoreError};

const SERVICE: &str = "com.hccake.skill-deck";
const ACCOUNT: &str = "github-token";

pub struct KeyringGithubCredentialStore;

impl KeyringGithubCredentialStore {
    fn entry() -> Result<keyring::Entry, GithubCredentialStoreError> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| GithubCredentialStoreError::Unavailable)
    }
}

impl GithubCredentialStore for KeyringGithubCredentialStore {
    fn read(&self) -> Result<Option<String>, GithubCredentialStoreError> {
        match Self::entry()?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(GithubCredentialStoreError::Unavailable),
        }
    }

    fn write(&self, token: &str) -> Result<(), GithubCredentialStoreError> {
        Self::entry()?
            .set_password(token)
            .map_err(|_| GithubCredentialStoreError::Unavailable)
    }

    fn delete(&self) -> Result<(), GithubCredentialStoreError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(GithubCredentialStoreError::Unavailable),
        }
    }
}
