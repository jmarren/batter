use std::path::PathBuf;

use anyhow::Result;

use crate::util;

pub struct Writer {
    base_path: PathBuf,
}

impl Writer {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    pub async fn write(&self, path: impl Into<PathBuf>, data: &[u8]) -> Result<()> {
        let full_path = self.base_path.join(path.into());
        // let full
        util::write_file(full_path, data).await
    }

    pub async fn write_js(&self, path: impl Into<PathBuf>, data: &[u8]) -> Result<()> {
        self.write_ext(path, data, "js").await
    }

    pub async fn write_txt(&self, path: impl Into<PathBuf>, data: &[u8]) -> Result<()> {
        self.write_ext(path, data, "txt").await
    }

    pub async fn write_html(&self, path: impl Into<PathBuf>, data: &[u8]) -> Result<()> {
        self.write_ext(path, data, "html").await
    }

    pub async fn write_ext(&self, _path: impl Into<PathBuf>, data: &[u8], ext: &str) -> Result<()> {
        let mut path: PathBuf = _path.into();
        // set js extension
        path.set_extension(ext);
        // write
        self.write(path, data).await
    }

    // async fn write_file(&self, data: &[u8], path: &str) -> Result<()> {
    //     let full_path = self
    //         .out_dir
    //         .join(PathBuf::from(&self.domain))
    //         .join(PathBuf::from(path));
    //
    //     util::write_file(full_path.clone(), data).await?;
    //
    //     if full_path.extension().is_some_and(|ext| ext == "js") {
    //         let handle = std::thread::spawn(move || {
    //             if let Err(err) = util::format_with_prettier(&full_path) {
    //                 tracing::warn!("failed to format {:?}: {}", full_path, err);
    //             }
    //         });
    //
    //         self.prettier_handles.lock().unwrap().push(handle);
    //     }
    //
    //     Ok(())
    // }
}
