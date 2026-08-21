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
