pub fn serialize_project_source(project_root: &str, source: &str) -> String {
    let project_root = normalize_separators(project_root);
    let source = normalize_separators(source);
    if source.starts_with("//") || project_root.starts_with("//") {
        return normalize_lexical(&source);
    }
    match (windows_drive(&project_root), windows_drive(&source)) {
        (Some((project_drive, project_path)), Some((source_drive, source_path))) => {
            if !project_drive.eq_ignore_ascii_case(source_drive) {
                return normalize_lexical(&source);
            }
            relative_path(project_path, source_path, true)
        }
        (None, None) if project_root.starts_with('/') && source.starts_with('/') => {
            relative_path(&project_root, &source, false)
        }
        _ => normalize_lexical(&source),
    }
}

pub fn resolve_project_source(project_root: &str, source: &str) -> String {
    let project_root = normalize_separators(project_root);
    let source = normalize_separators(source);
    if is_absolute_portable(&source) {
        return normalize_lexical(&source);
    }
    normalize_lexical(&format!(
        "{}/{}",
        project_root.trim_end_matches('/'),
        source
    ))
}

fn relative_path(root: &str, source: &str, case_insensitive: bool) -> String {
    let root = normalize_lexical(root);
    let source = normalize_lexical(source);
    let root = root.trim_matches('/').split('/').collect::<Vec<_>>();
    let source = source.trim_matches('/').split('/').collect::<Vec<_>>();
    let shared = root
        .iter()
        .zip(&source)
        .take_while(|(left, right)| {
            if case_insensitive {
                left.eq_ignore_ascii_case(right)
            } else {
                left == right
            }
        })
        .count();
    let mut relative = vec![".."; root.len().saturating_sub(shared)];
    relative.extend_from_slice(&source[shared..]);
    if relative.is_empty() {
        ".".to_string()
    } else {
        relative.join("/")
    }
}

fn windows_drive(path: &str) -> Option<(&str, &str)> {
    let bytes = path.as_bytes();
    (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/')
        .then(|| (&path[..2], &path[2..]))
}

fn is_absolute_portable(path: &str) -> bool {
    path.starts_with('/') || path.starts_with("//") || windows_drive(path).is_some()
}

fn normalize_separators(path: &str) -> String {
    path.replace('\\', "/")
}

fn normalize_lexical(path: &str) -> String {
    let path = normalize_separators(path);
    let (prefix, remainder, absolute) = if let Some(remainder) = path.strip_prefix("//") {
        ("//".to_string(), remainder.trim_start_matches('/'), true)
    } else if let Some((drive, remainder)) = windows_drive(&path) {
        (format!("{drive}/"), remainder.trim_start_matches('/'), true)
    } else if let Some(remainder) = path.strip_prefix('/') {
        ("/".to_string(), remainder.trim_start_matches('/'), true)
    } else {
        (String::new(), path.as_str(), false)
    };
    let mut components = Vec::new();
    for component in remainder.split('/') {
        match component {
            "" | "." => {}
            ".." if components.last().is_some_and(|value| *value != "..") => {
                components.pop();
            }
            ".." if !absolute => components.push(component),
            ".." => {}
            value => components.push(value),
        }
    }
    format!("{prefix}{}", components.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_posix_and_wsl_sources_relative_to_the_project() {
        assert_eq!(
            serialize_project_source("/work/app", "/work/shared/skills"),
            "../shared/skills"
        );
        assert_eq!(
            resolve_project_source("/mnt/c/work/app", "../shared/skills"),
            "/mnt/c/work/shared/skills"
        );
    }

    #[test]
    fn serializes_windows_same_drive_with_posix_separators() {
        assert_eq!(
            serialize_project_source(r"c:\work\app", r"C:\WORK\shared\skills"),
            "../shared/skills"
        );
        assert_eq!(
            resolve_project_source(r"C:\work\app", "../shared/skills"),
            "C:/work/shared/skills"
        );
    }

    #[test]
    fn windows_cross_drive_source_stays_absolute() {
        assert_eq!(
            serialize_project_source(r"C:\work\app", r"D:\skills\demo"),
            "D:/skills/demo"
        );
    }

    #[test]
    fn windows_unc_source_keeps_its_absolute_share_prefix() {
        assert_eq!(
            serialize_project_source(r"C:\work\app", r"\\server\share\skills\demo"),
            "//server/share/skills/demo"
        );
        assert_eq!(
            resolve_project_source(r"C:\work\app", r"\\server\share\skills\demo"),
            "//server/share/skills/demo"
        );
    }

    #[test]
    fn relative_lock_value_moves_with_the_project_root() {
        let stored = serialize_project_source("/old/work/app", "/old/work/shared/skills");

        assert_eq!(
            resolve_project_source("/new/work/app", &stored),
            "/new/work/shared/skills"
        );
    }
}
