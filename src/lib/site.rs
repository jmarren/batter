use std::path::PathBuf;

use anyhow::{Result, anyhow};
use tokio::task::{JoinError, JoinSet};

use crate::{
    html,
    js::JsSource,
    util::{self, ensure_js_ext},
};

pub struct Site {
    domain: String,
    out_dir: PathBuf,
    // document: Option<html::Document>,
}

impl Site {
    pub fn new(out_dir: PathBuf, domain: String) -> Self {
        Self { domain, out_dir }
    }

    // fetch html from domain, write it to file, and return document struct
    async fn fetch_html(&mut self) -> Result<html::Document> {
        // fetch the site and send data to sender to write
        let res = util::fetch_url(&self.domain).await?;

        // write index to file
        self.write_index(&res).await?;

        // create document
        Ok(html::Document::new(&res)?)
    }

    // run enumeration on the site
    pub async fn enumerate(&mut self) -> Result<()> {
        // fetch html
        let doc = self.fetch_html().await?;

        // get sources
        let sources = doc.sources();

        // handle sources
        self.handle_sources(&sources).await?;

        Ok(())
    }

    // handle source urls returned from document
    async fn handle_sources(&self, sources: &[String]) -> Result<()> {
        let mut join_set = JoinSet::new();

        // spawn a task to fetch each source found
        for i in 0..sources.len() {
            let src = format!("{}{}", self.domain, sources[i]);
            join_set.spawn(async move {
                let res = util::fetch_url(&src).await;
                (res, src)
            });
        }

        while let Some(task_result) = join_set.join_next().await {
            // ignore errors so we don't stop prematurely
            // TODO: log errors
            let _ = self.handle_source_response(task_result).await;
        }

        Ok(())
    }

    // handle javascript returned from source url
    // - write to file
    // - parse
    async fn handle_source_response(
        &self,
        task_result: Result<(Result<Vec<u8>>, String), JoinError>,
    ) -> Result<()> {
        let Ok((Ok(bytes), src)) = task_result else {
            return Err(anyhow!("task failed"));
        };

        // ensure source has `.js` file extension
        let file_path = ensure_js_ext(src);

        // write the data to the source path
        self.write_file(&bytes, &file_path).await?;

        // create new JsSource object from data
        let js_source = JsSource::new(String::from_utf8(bytes)?);

        // parse the source
        js_source.parse()?;

        Ok(())
    }

    async fn write_index(&self, data: &[u8]) -> Result<()> {
        self.write_file(data, "index.html").await?;
        Ok(())
    }

    async fn write_file(&self, data: &[u8], path: &str) -> Result<()> {
        let full_path = self.out_dir.join(PathBuf::from(path));

        util::write_file(full_path, data).await?;

        Ok(())
    }
}
