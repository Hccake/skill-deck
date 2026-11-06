//! 下载维护使用明确保存的原始地址，展示值和临时跳转地址不参与资格推断。

use url::Url;

pub(crate) fn maintenance_url(value: Option<&str>) -> Option<Url> {
    let url = Url::parse(value?).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

pub(crate) fn valid_member_path(value: Option<&str>) -> bool {
    value.is_some_and(|path| {
        !path.is_empty()
            && !path.starts_with('/')
            && !path.contains(['\\', '\0'])
            && path
                .split('/')
                .all(|part| !matches!(part, "" | "." | "..") && !part.contains(':'))
    })
}

pub(crate) fn valid_content_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
