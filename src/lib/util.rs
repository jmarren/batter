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
