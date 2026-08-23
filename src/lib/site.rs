use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread::JoinHandle;

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
    allow_external_sources: bool,
    // document: Option<html::Document>,
    prettier_handles: Mutex<Vec<JoinHandle<()>>>,
    all_sources: Mutex<Vec<String>>,
}

impl Site {
    pub fn new(out_dir: PathBuf, domain: String, allow_external_sources: bool) -> Self {
        Self {
            domain,
            out_dir,
            allow_external_sources,
            prettier_handles: Mutex::new(vec![]),
            all_sources: Mutex::new(vec![]),
        }
    }

    // resolves `src` against the site's domain, records it in `all_sources`
    // regardless of the outcome, and returns the fetchable url unless it's
    // cross-origin and external sources aren't allowed
    fn resolve_source(&self, src: &str) -> Option<String> {
        let (host, path) = match util::resolve_source_url(&self.domain, src) {
            Ok(resolved) => resolved,
            Err(err) => {
                tracing::error!("failed to resolve source url {src}: {err:#}");
                return None;
            }
        };

        let full_url = format!("{host}{path}");
        self.all_sources.lock().unwrap().push(full_url.clone());

        if host != self.domain && !self.allow_external_sources {
            tracing::info!("discarding cross-origin source: {full_url}");
            return None;
        }

        Some(full_url)
    }

    // write every source url discovered during the crawl to js-sources.txt
    async fn write_all_sources(&self) -> Result<()> {
        let sources = self.all_sources.lock().unwrap().clone();
        let contents: String = sources
            .iter()
            .map(|src| format!("https://{src}\n"))
            .collect();

        self.write_file(contents.as_bytes(), "js-sources.txt")
            .await
    }

    // wait for every spawned prettier thread to finish
    fn join_prettier_handles(&self) {
        let handles = std::mem::take(&mut *self.prettier_handles.lock().unwrap());

        for handle in handles {
            let _ = handle.join();
        }
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

        // wait for all formatting to finish before returning
        self.join_prettier_handles();

        // write every discovered source url to js-sources.txt
        self.write_all_sources().await?;

        Ok(())
    }

    // handle source urls returned from document
    async fn handle_sources(&self, sources: &[String]) -> Result<()> {
        let mut join_set = JoinSet::new();
        let mut seen: HashSet<String> = HashSet::new();

        // spawn a task to fetch each source found
        for source in sources {
            let Some(src) = self.resolve_source(source) else {
                continue;
            };

            if !seen.insert(src.clone()) {
                continue;
            }

            tracing::info!("discovered {}", src);

            join_set.spawn(async move {
                let res = util::fetch_url(&src).await;
                (res, src)
            });
        }

        while let Some(task_result) = join_set.join_next().await {
            let chunk_urls = match self.handle_source_response(task_result).await {
                Ok(chunk_urls) => chunk_urls,
                Err(err) => {
                    // ignore errors so we don't stop prematurely, but report them
                    tracing::error!("failed to handle source: {err:#}");
                    continue;
                }
            };

            // spawn a task for any newly discovered chunk we haven't already seen
            for chunk_url in chunk_urls {
                let Some(src) = self.resolve_source(&format!("_next/{chunk_url}")) else {
                    continue;
                };

                if !seen.insert(src.clone()) {
                    continue;
                }

                tracing::info!("discovered {}", src);

                join_set.spawn(async move {
                    let res = util::fetch_url(&src).await;
                    (res, src)
                });
            }
        }

        Ok(())
    }

    // handle javascript returned from source url
    // - write to file
    // - parse
    // - return any chunk urls discovered while parsing
    async fn handle_source_response(
        &self,
        task_result: Result<(Result<Vec<u8>>, String), JoinError>,
    ) -> Result<HashSet<String>> {
        let (res, src) = task_result.map_err(|err| anyhow!("fetch task panicked: {err}"))?;
        let bytes = res.map_err(|err| anyhow!("failed to fetch {src}: {err}"))?;

        // ensure source has `.js` file extension
        let file_path = ensure_js_ext(src);

        // write the data to the source path
        self.write_file(&bytes, &file_path).await?;

        // create new JsSource object from data
        let js_source = JsSource::new(String::from_utf8(bytes)?);

        // parse the source and return any chunk urls found
        js_source.parse()
    }

    async fn write_index(&self, data: &[u8]) -> Result<()> {
        self.write_file(data, "index.html").await?;
        Ok(())
    }

    async fn write_file(&self, data: &[u8], path: &str) -> Result<()> {
        let full_path = self
            .out_dir
            .join(PathBuf::from(&self.domain))
            .join(PathBuf::from(path));

        util::write_file(full_path.clone(), data).await?;

        if full_path.extension().is_some_and(|ext| ext == "js") {
            let handle = std::thread::spawn(move || {
                if let Err(err) = util::format_with_prettier(&full_path) {
                    tracing::warn!("failed to format {:?}: {}", full_path, err);
                }
            });

            self.prettier_handles.lock().unwrap().push(handle);
        }

        Ok(())
    }
}
