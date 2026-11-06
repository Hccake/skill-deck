//! 下载来源的只读展示，不授予重新获取或更新能力。
use url::Url;

pub(crate) struct DownloadSourceDisplay {
    pub label: String,
    pub origin: Option<String>,
    pub openable_url: Option<String>,
}

pub(crate) fn download_source_display(
    source: &str,
    source_url: Option<&str>,
) -> DownloadSourceDisplay {
    let raw = source_url
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(source);
    let Ok(url) = Url::parse(raw) else {
        return DownloadSourceDisplay {
            label: String::new(),
            origin: None,
            openable_url: None,
        };
    };
    if url.scheme() == "notion" && url.host_str() == Some("packs") {
        return DownloadSourceDisplay {
            label: "Notion".into(),
            origin: Some("notion:packs".into()),
            openable_url: None,
        };
    }
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return DownloadSourceDisplay {
            label: String::new(),
            origin: None,
            openable_url: None,
        };
    }
    let origin = url.origin().ascii_serialization();
    let label = origin
        .split_once("://")
        .map_or(origin.as_str(), |(_, host)| host)
        .to_owned();
    let openable = url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    DownloadSourceDisplay {
        label,
        origin: Some(origin),
        openable_url: openable.then(|| url.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn download_display_never_opens_a_redacted_or_unsupported_locator() {
        for address in [
            "https://user:secret@example.com/tool.zip",
            "https://example.com/tool.zip?token=secret",
            "https://example.com/tool.zip#secret",
        ] {
            let display = download_source_display(address, None);
            assert_eq!(display.label, "example.com");
            assert!(display.openable_url.is_none());
            assert_eq!(display.origin.as_deref(), Some("https://example.com"));
        }
        assert_eq!(
            download_source_display("https://example.com/tool.zip", None)
                .openable_url
                .as_deref(),
            Some("https://example.com/tool.zip")
        );
        assert_eq!(
            download_source_display("notion", Some("notion://packs/secret-id")).label,
            "Notion"
        );
        for address in ["file:///private/skill", "not a URL", "javascript:secret"] {
            let display = download_source_display(address, None);
            assert!(display.label.is_empty() && display.openable_url.is_none());
        }
    }
}
