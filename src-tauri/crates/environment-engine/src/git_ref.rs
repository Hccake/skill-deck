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
