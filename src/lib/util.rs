use anyhow::Result;
use std::env;
// use std::io;
use std::path::PathBuf;

fn get_cwd() -> Result<PathBuf> {
    let ctx = env::current_dir()?;
    Ok(ctx)
}

pub fn full_path(path: PathBuf) -> Result<PathBuf> {
    let cwd = get_cwd()?;
    let out_dir = cwd.join(path);
    Ok(std::fs::canonicalize(out_dir)?)
}

pub async fn fetch_url(url: &str) -> Result<Vec<u8>> {
    let response = reqwest::get(format!("https://{}", url))
        .await?
        .error_for_status()?;
    let bytes = response.bytes().await?;

    Ok(bytes.to_vec())
}

pub async fn write_file(full_path: PathBuf, data: &[u8]) -> Result<()> {
    // get last directory of path
    if let Some(parent) = full_path.parent() {
        // if it doesn't exist, create it
        tokio::fs::create_dir_all(parent).await?;
    }

    // write the file
    tokio::fs::write(full_path, data).await?;

    Ok(())
}

pub fn ensure_suffix(src: String, suffix: String) -> String {
    if src.ends_with(&suffix) {
        src
    } else {
        format!("{src}{suffix}")
    }
}

pub fn ensure_js_ext(src: String) -> String {
    ensure_suffix(src, String::from(".js"))
}

// strips one layer of surrounding `"`/`'` from a string literal's raw source text
pub fn strip_quotes(raw: &str) -> String {
    raw.strip_prefix(['"', '\''])
        .and_then(|s| s.strip_suffix(['"', '\'']))
        .unwrap_or(raw)
        .to_string()
}

// formats a js file in place with prettier; blocking, meant to be run on its own thread
pub fn format_with_prettier(path: &PathBuf) -> Result<()> {
    std::process::Command::new("npx")
        .arg("prettier")
        .arg("--write")
        .arg(path)
        .status()?;

    Ok(())
}

// resolves a discovered source url (relative, root-relative, protocol-relative, or
// fully-absolute) against the site's domain, returning the resolved host and the
// path (with query string, if any) relative to that host - callers decide whether
// to keep it based on whether the host matches the domain being crawled
pub fn resolve_source_url(domain: &str, src: &str) -> Result<(String, String)> {
    let base = reqwest::Url::parse(&format!("https://{domain}"))?;
    let resolved = base.join(src)?;

    let host = resolved.host_str().unwrap_or(domain).to_string();

    let path = match resolved.query() {
        Some(query) => format!("{}?{}", resolved.path(), query),
        None => resolved.path().to_string(),
    };

    Ok((host, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_root_relative_path() {
        let (host, path) = resolve_source_url("example.com", "/static/foo.js").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(path, "/static/foo.js");
    }

    #[test]
    fn resolves_bare_relative_path() {
        let (host, path) = resolve_source_url("example.com", "static/foo.js").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(path, "/static/foo.js");
    }

    #[test]
    fn resolves_protocol_relative_same_host() {
        let (host, path) = resolve_source_url("example.com", "//example.com/foo.js").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(path, "/foo.js");
    }

    #[test]
    fn resolves_protocol_relative_different_host() {
        let (host, path) =
            resolve_source_url("example.com", "//cdn.example.com/foo.js").unwrap();
        assert_eq!(host, "cdn.example.com");
        assert_eq!(path, "/foo.js");
    }

    #[test]
    fn resolves_absolute_same_host() {
        let (host, path) =
            resolve_source_url("example.com", "https://example.com/foo.js").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(path, "/foo.js");
    }

    #[test]
    fn resolves_absolute_different_host() {
        let (host, path) =
            resolve_source_url("example.com", "https://cdn.example.com/foo.js").unwrap();
        assert_eq!(host, "cdn.example.com");
        assert_eq!(path, "/foo.js");
    }

    #[test]
    fn preserves_query_string() {
        let (host, path) =
            resolve_source_url("example.com", "/foo.js?v=123").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(path, "/foo.js?v=123");
    }
}
