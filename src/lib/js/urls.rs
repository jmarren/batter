// recognizes string literals that look like absolute http/https urls, e.g.
// "https://api.example.com/v1/users" - unlike endpoints.rs this isn't tied
// to a particular call shape, it just looks at every string literal in the
// source, so it also picks up urls stashed in plain variables/config objects
// rather than only ones passed directly to a request call.
pub(super) fn is_url(s: &str) -> bool {
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"));

    match rest {
        // must have some non-empty host after the scheme
        Some(rest) => !rest.is_empty() && !rest.starts_with(['/', '?', '#']),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::js::source::JsSource;

    fn urls_of(source: &str) -> HashSet<String> {
        let source_url = reqwest::Url::parse("https://example.com/app.js").unwrap();
        JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .urls
    }

    #[test]
    fn detects_https_url() {
        let urls = urls_of(r#"const x = "https://api.example.com/v1/users";"#);
        assert_eq!(
            urls,
            HashSet::from(["https://api.example.com/v1/users".to_string()])
        );
    }

    #[test]
    fn detects_http_url() {
        let urls = urls_of(r#"const x = "http://example.com";"#);
        assert_eq!(urls, HashSet::from(["http://example.com".to_string()]));
    }

    #[test]
    fn detects_url_anywhere_in_source_not_just_call_args() {
        let urls = urls_of(r#"const config = { base: "https://cdn.example.com/assets" };"#);
        assert_eq!(
            urls,
            HashSet::from(["https://cdn.example.com/assets".to_string()])
        );
    }

    #[test]
    fn skips_relative_path() {
        let urls = urls_of(r#"fetch("/api/users");"#);
        assert!(urls.is_empty());
    }

    #[test]
    fn skips_non_url_string() {
        let urls = urls_of(r#"const x = "hello world";"#);
        assert!(urls.is_empty());
    }

    #[test]
    fn skips_bare_scheme_with_no_host() {
        let urls = urls_of(r#"const x = "https://";"#);
        assert!(urls.is_empty());
    }
}
