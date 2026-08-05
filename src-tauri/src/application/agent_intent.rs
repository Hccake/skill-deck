use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::mutation::result::FallbackReasonCode;
use crate::core::agent_definition::AgentId;
use crate::error::AppError;
use crate::models::InstallMode;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct AdapterTargetId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentWriteIntent {
    pub agent_id: AgentId,
    pub own_directory_selected: bool,
    pub adapter_targets: Vec<AdapterTargetId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentTargetFallbackPreview {
    pub agent_id: AgentId,
    pub target_id: String,
    pub requested_mode: InstallMode,
    pub forecast_mode: InstallMode,
    pub reason: Option<FallbackReasonCode>,
}

pub fn validate_agent_intents(intents: &[AgentWriteIntent]) -> Result<(), AppError> {
    let mut agents = BTreeSet::new();
    for intent in intents {
        if !agents.insert(intent.agent_id.clone()) {
            return Err(validation("duplicate Agent write intent"));
        }
        let mut targets = BTreeSet::new();
        if intent
            .adapter_targets
            .iter()
            .any(|target| target.0.is_empty() || !targets.insert(target.clone()))
        {
            return Err(validation("invalid or duplicate adapter target"));
        }
    }
    Ok(())
}

fn validation(message: &str) -> AppError {
    AppError::Validation {
        field: Some("agentIntents".to_string()),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent_definition::AgentId;

    #[test]
    fn transient_agent_intent_has_no_install_mode() {
        let intent = AgentWriteIntent {
            agent_id: AgentId::parse("custom-agent").expect("agent"),
            own_directory_selected: true,
            adapter_targets: vec![AdapterTargetId("target-1".to_string())],
        };

        let value = serde_json::to_value(intent).expect("serialize");

        assert!(!value.as_object().expect("object").contains_key("mode"));
        assert_eq!(value["ownDirectorySelected"], true);
    }

    #[test]
    fn duplicate_agent_intents_are_rejected() {
        let duplicate = || AgentWriteIntent {
            agent_id: AgentId::parse("custom-agent").expect("agent"),
            own_directory_selected: true,
            adapter_targets: Vec::new(),
        };

        assert!(validate_agent_intents(&[duplicate(), duplicate()]).is_err());
    }
}
