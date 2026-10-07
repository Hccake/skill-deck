use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[specta(tag = "kind", rename_all = "camelCase")]
pub enum TranslationServiceRef {
    Google,
    #[default]
    Microsoft,
    OpenAiCompatible {
        #[serde(rename = "profileId")]
        #[specta(rename = "profileId")]
        profile_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct TranslationProfile {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct TranslationSettings {
    pub revision: String,
    pub service_revision: String,
    pub service: TranslationServiceRef,
    pub profiles: Vec<TranslationProfile>,
    pub auto_translate: bool,
    pub notice_accepted: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredTranslationProfile {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub credential_ref: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredTranslationSettings {
    pub revision: String,
    pub service: TranslationServiceRef,
    pub profiles: Vec<StoredTranslationProfile>,
    pub auto_translate: bool,
    pub notice_accepted: bool,
}

impl Default for StoredTranslationSettings {
    fn default() -> Self {
        Self {
            revision: "initial".into(),
            service: TranslationServiceRef::Microsoft,
            profiles: Vec::new(),
            auto_translate: false,
            notice_accepted: false,
        }
    }
}

impl StoredTranslationSettings {
    pub fn snapshot(&self) -> TranslationSettings {
        use sha2::Digest;
        let profile = match &self.service {
            TranslationServiceRef::OpenAiCompatible { profile_id } => {
                self.profiles.iter().find(|p| &p.id == profile_id)
            }
            _ => None,
        };
        let identity =
            serde_json::to_vec(&(&self.service, profile)).expect("translation service identity");
        TranslationSettings {
            revision: format!(
                "{:x}",
                sha2::Sha256::digest(
                    serde_json::to_vec(self).expect("translation settings identity")
                )
            ),
            service_revision: format!("{:x}", sha2::Sha256::digest(identity)),
            service: self.service.clone(),
            profiles: self
                .profiles
                .iter()
                .map(|p| TranslationProfile {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    base_url: p.base_url.clone(),
                    model: p.model.clone(),
                    has_api_key: p.credential_ref.is_some(),
                })
                .collect(),
            auto_translate: self.auto_translate,
            notice_accepted: self.notice_accepted,
        }
    }
}

// 输入密钥不派生 Debug，避免错误诊断记录密钥。
#[derive(Clone, Deserialize, Type)]
#[serde(tag = "action", rename_all = "camelCase")]
#[specta(tag = "action", rename_all = "camelCase")]
pub enum TranslationKeyChange {
    Keep,
    Replace { value: String },
    Clear,
}

#[derive(Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct TranslationProfileDraft {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub key: TranslationKeyChange,
}

#[derive(Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct TranslationSettingsDraft {
    pub expected_revision: String,
    pub service: TranslationServiceRef,
    pub profiles: Vec<TranslationProfileDraft>,
    pub auto_translate: bool,
    pub notice_accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum TranslationError {
    InvalidSettings,
    StaleSettings,
    CredentialUnavailable,
    Authentication,
    RateLimited,
    Network,
    Timeout,
    UnsupportedResponse,
    InvalidSegments,
    Cancelled,
    CacheUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct TranslationUnit {
    pub id: String,
    pub text: String,
    pub context: String,
    pub protected_tokens: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
#[specta(tag = "status", rename_all = "camelCase")]
pub enum TranslationResult {
    Translated { id: String, text: String },
    Failed { id: String, code: TranslationError },
}

#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct TranslationCacheInfo {
    pub bytes: f64,
    pub entries: u32,
}
