/// Git 传输内部的已分类引用。`commit:` 仅用于进程适配器之间传递，持久记录保存原值与类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedReference {
    pub reference: String,
    pub revision: Option<String>,
}

pub fn is_full_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn pinned_commit(reference: &str) -> Option<&str> {
    reference
        .strip_prefix("commit:")
        .filter(|value| is_full_commit(value))
}

/// 必须先成功取得远端引用列表，才能把未分类完整 SHA 当作提交。
pub fn resolve_reference(output: &str, reference: Option<&str>) -> Option<ResolvedReference> {
    if let Some(commit) = reference.and_then(pinned_commit) {
        return Some(ResolvedReference {
            reference: format!("commit:{}", commit.to_ascii_lowercase()),
            revision: None,
        });
    }
    for candidate in remote_ref_patterns(reference) {
        if let Some(revision) = output
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .find_map(|(revision, name)| {
                (name == candidate
                    && matches!(revision.len(), 40 | 64)
                    && revision.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| revision.to_ascii_lowercase())
            })
        {
            return Some(ResolvedReference {
                reference: candidate.trim_end_matches("^{}").to_string(),
                revision: Some(revision),
            });
        }
    }
    reference
        .filter(|value| is_full_commit(value))
        .map(|value| ResolvedReference {
            reference: format!("commit:{}", value.to_ascii_lowercase()),
            revision: None,
        })
}

/// Ordered Git ref names, shared by Native and Worker probes.
pub fn remote_ref_patterns(reference: Option<&str>) -> Vec<String> {
    match reference.filter(|value| !value.is_empty()) {
        None => vec!["HEAD".into()],
        Some(value) if value.starts_with("refs/tags/") => {
            vec![format!("{value}^{{}}"), value.into()]
        }
        Some(value) if value.starts_with("refs/") => vec![value.into()],
        Some(value) => vec![
            format!("refs/heads/{value}"),
            format!("refs/tags/{value}^{{}}"),
            format!("refs/tags/{value}"),
        ],
    }
}

pub fn resolve_remote_revision(output: &str, reference: Option<&str>) -> Option<String> {
    let records = output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect::<Vec<_>>();
    remote_ref_patterns(reference).iter().find_map(|candidate| {
        let (revision, _) = records.iter().find(|(_, name)| *name == candidate)?;
        (matches!(revision.len(), 40 | 64) && revision.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| revision.to_ascii_lowercase())
    })
}

pub fn clone_branch(reference: &str) -> &str {
    reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/tags/"))
        .unwrap_or(reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_preserves_named_refs_and_only_falls_back_to_full_commits() {
        let sha = "a".repeat(40);
        let newer = "b".repeat(40);
        let output = format!("{newer}\trefs/heads/{sha}\n");
        let named = resolve_reference(&output, Some(&sha)).unwrap();
        assert_eq!(named.reference, format!("refs/heads/{sha}"));
        assert_eq!(named.revision.as_deref(), Some(newer.as_str()));
        assert_eq!(
            resolve_reference("", Some(&sha)).unwrap().reference,
            format!("commit:{sha}")
        );
        assert_eq!(
            resolve_reference(&output, Some(&format!("commit:{sha}")))
                .unwrap()
                .reference,
            format!("commit:{sha}")
        );
        assert!(resolve_reference("", Some("abcdef12")).is_none());
        assert!(resolve_reference("", Some("refs/heads/missing")).is_none());
        assert!(resolve_reference("", Some("commit:abc")).is_none());
    }

    #[test]
    fn short_names_prefer_branches_while_explicit_tags_peel_to_commits() {
        let branch = "a".repeat(40);
        let object = "b".repeat(40);
        let commit = "c".repeat(40);
        let output = format!("{object}\trefs/tags/release\n{commit}\trefs/tags/release^{{}}\n{branch}\trefs/heads/release\n");
        assert_eq!(
            resolve_remote_revision(&output, Some("release")),
            Some(branch.clone())
        );
        assert_eq!(
            resolve_remote_revision(&output, Some("refs/heads/release")),
            Some(branch)
        );
        assert_eq!(
            resolve_remote_revision(&output, Some("refs/tags/release")),
            Some(commit)
        );
        assert_eq!(resolve_remote_revision(&output, Some("missing")), None);
        assert_eq!(resolve_remote_revision(&output, None), None);
    }

    #[test]
    fn default_ref_and_lightweight_tags_are_exact_and_validate_object_ids() {
        let commit = "d".repeat(64);
        let output = format!("{commit}\tHEAD\n{commit}\trefs/tags/v1\ninvalid\trefs/heads/bad\n");
        assert_eq!(resolve_remote_revision(&output, None), Some(commit.clone()));
        assert_eq!(resolve_remote_revision(&output, Some("v1")), Some(commit));
        assert_eq!(resolve_remote_revision(&output, Some("bad")), None);
    }
}
