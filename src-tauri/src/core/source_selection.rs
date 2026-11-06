use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceSelectionResult {
    pub matched_names: Vec<String>,
    pub unmatched_inputs: Vec<String>,
    pub ambiguous_inputs: Vec<AmbiguousSourceSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AmbiguousSourceSelection {
    pub input: String,
    pub candidate_names: Vec<String>,
}

fn normalized_name(name: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for c in name.to_lowercase().chars() {
        if c.is_whitespace() || c == '_' {
            if !separator {
                result.push('-');
            }
            separator = true;
        } else {
            result.push(c);
            separator = false;
        }
    }
    result
}

/// 只决定来源预选，不改变原名、安装目录和更新身份。
pub(crate) fn match_names(names: &BTreeSet<String>, available: &[String]) -> SourceSelectionResult {
    let mut result = SourceSelectionResult::default();
    for input in names {
        let exact = available
            .iter()
            .filter(|name| name.to_lowercase() == input.to_lowercase())
            .cloned()
            .collect::<Vec<_>>();
        let matches = if exact.is_empty() {
            available
                .iter()
                .filter(|name| normalized_name(name) == normalized_name(input))
                .cloned()
                .collect::<Vec<_>>()
        } else {
            exact
        };
        match matches.as_slice() {
            [] => result.unmatched_inputs.push(input.clone()),
            [name] => {
                if !result.matched_names.contains(name) {
                    result.matched_names.push(name.clone());
                }
            }
            _ => result.ambiguous_inputs.push(AmbiguousSourceSelection {
                input: input.clone(),
                candidate_names: matches,
            }),
        }
    }
    result
}

impl SourceSelectionResult {
    pub(crate) fn includes(&self, name: &str) -> bool {
        self.matched_names.iter().any(|item| item == name)
            || self
                .ambiguous_inputs
                .iter()
                .any(|item| item.candidate_names.iter().any(|item| item == name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_names_win_and_ambiguous_normalization_does_not_preselect() {
        let names = ["code_review".into(), "code-review".into(), "missing".into()].into();
        let result = match_names(&names, &["Code Review".into(), "code_review".into()]);
        assert_eq!(result.matched_names, ["code_review"]);
        assert_eq!(result.unmatched_inputs, ["missing"]);
        assert_eq!(
            result.ambiguous_inputs[0].candidate_names,
            ["Code Review", "code_review"]
        );
    }
}
