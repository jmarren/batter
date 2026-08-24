use anyhow::{Context, Result, anyhow};
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

fn get_cwd() -> Result<PathBuf> {
    let ctx = env::current_dir()?;
    Ok(ctx)
}

pub fn full_path(path: PathBuf) -> Result<PathBuf> {
    let cwd = get_cwd()?;
    let out_dir = cwd.join(path);
    Ok(std::fs::canonicalize(out_dir)?)
}

pub async fn fetch_url(url: reqwest::Url) -> Result<Vec<u8>> {
    let response = reqwest::get(url).await?.error_for_status()?;

    // reqwest::get
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

// formats `data` with prettier via stdin/stdout, without ever touching disk -
// `path` is only used so prettier can infer which parser to use from the
// extension (it never needs to exist). blocking; meant to be run on its own
// thread/via spawn_blocking.
pub fn format_with_prettier_stdin(path: &Path, data: &[u8]) -> Result<Vec<u8>> {
    let mut child = std::process::Command::new("npx")
        .arg("prettier")
        .arg("--stdin-filepath")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn prettier")?;

    // write on a separate thread so a large payload can't deadlock against
    // prettier also trying to flush its own stdout back to us
    let mut stdin = child.stdin.take().expect("child stdin was requested");
    let data = data.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&data));

    let output = child
        .wait_with_output()
        .context("failed to read prettier output")?;

    writer
        .join()
        .map_err(|_| anyhow!("prettier stdin writer thread panicked"))??;

    if !output.status.success() {
        return Err(anyhow!(
            "prettier exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(output.stdout)
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
        let (host, path) = resolve_source_url("example.com", "//cdn.example.com/foo.js").unwrap();
        assert_eq!(host, "cdn.example.com");
        assert_eq!(path, "/foo.js");
    }

    #[test]
    fn resolves_absolute_same_host() {
        let (host, path) = resolve_source_url("example.com", "https://example.com/foo.js").unwrap();
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
        let (host, path) = resolve_source_url("example.com", "/foo.js?v=123").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(path, "/foo.js?v=123");
    }

    #[test]
    fn formats_js_via_stdin() {
        let formatted =
            format_with_prettier_stdin(Path::new("test.js"), b"const x={a:1,b:2}").unwrap();

        // don't assert on prettier's exact formatting choices, just that it
        // actually reformatted rather than passing the input through as-is
        assert_ne!(formatted, b"const x={a:1,b:2}");
        assert_eq!(
            String::from_utf8(formatted).unwrap(),
            "const x = { a: 1, b: 2 };\n"
        );
    }

    #[test]
    fn falls_back_on_prettier_failure_for_unparseable_input() {
        let result = format_with_prettier_stdin(Path::new("test.js"), b"const x = {{{{");

        assert!(result.is_err());
    }
}
