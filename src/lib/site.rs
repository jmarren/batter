use std::collections::HashSet;
use std::sync::Mutex;

use anyhow::{Result, anyhow};
use tokio::task::{JoinError, JoinSet};

use crate::{
    html,
    js::JsSource,
    util::{self},
    writer::Writer,
};

pub struct Site {
    url: reqwest::Url,
    // domain: String,
    // out_dir: PathBuf,
    allow_external_sources: bool,
    // document: Option<html::Document>,
    // prettier_handles: Mutex<Vec<JoinHandle<()>>>,
    all_sources: Mutex<Vec<String>>,
    writer: Writer,

    client: reqwest::Client,
}

impl Site {
    pub fn new(writer: Writer, url: reqwest::Url) -> Self {
        Self {
            url,
            // url: reqwest::Url::parse(format!(")
            // domain,
            writer,
            client: reqwest::Client::new(),
            allow_external_sources: false,
            all_sources: Mutex::new(vec![]),
        }
    }

    pub fn allow_external_sources(mut self) -> Self {
        self.allow_external_sources = true;
        self
    }
    // write every source url discovered during the crawl to js-sources.txt
    async fn write_all_sources(&self) -> Result<()> {
        let sources = self.all_sources.lock().unwrap().clone();
        let contents: String = sources
            .iter()
            .map(|src| format!("https://{src}\n"))
            .collect();

        self.writer
            .write("js-sources.txt", contents.as_bytes())
            .await
    }

    // fetch html from domain, write it to file, and return document struct
    async fn fetch_html(&mut self) -> Result<html::Document> {
        let res = self
            .client
            .get(self.url.join("")?)
            .send()
            .await?
            .bytes()
            .await?;

        self.writer.write("index.html", &res).await?;

        // create document
        Ok(html::Document::new(&res)?)
    }

    // run enumeration on the site
    pub async fn enumerate(&mut self) -> Result<()> {
        // fetch html
        let doc = self.fetch_html().await?;

        let mut source_strs = vec![];

        // get sources and join with self.url
        let all_sources: Vec<reqwest::Url> = doc
            .sources()
            .iter()
            // join with base url.
            .filter_map(|src| match self.url.join(src) {
                Ok(x) => {
                    source_strs.push(x.to_string());
                    Some(x)
                }
                // filter out and report if failed
                Err(e) => {
                    tracing::error!("{:?}", e);
                    None
                }
            })
            .collect();

        // filter out cross origin if we are not allowing them
        let sources = {
            if self.allow_external_sources {
                all_sources
            } else {
                all_sources
                    .into_iter()
                    .filter(|src| src.domain() == self.url.domain())
                    .collect()
            }
        };

        // handle sources
        self.handle_sources(&sources).await?;

        // write every discovered source url to js-sources.txt
        // self.write_all_sources().await?;

        Ok(())
    }

    // handle source urls returned from document
    async fn handle_sources(&self, sources: &[reqwest::Url]) -> Result<()> {
        let mut join_set = JoinSet::new();
        let mut seen: HashSet<reqwest::Url> = HashSet::new();

        sources.into_iter().for_each(|src| {
            if !seen.insert(*src) {
                return;
            }

            tracing::info!("{}", src);

            join_set.spawn(async move {
                let res = util::fetch_url(src).await;
                (res, src)
            });
        });

        while let Some(task_result) = join_set.join_next().await {
            // parse chunk_urls from the source
            let chunk_urls = match self.parse_chunk_urls(task_result).await {
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
    async fn parse_chunk_urls(
        &self,
        task_result: Result<(Result<Vec<u8>>, &reqwest::Url), JoinError>,
    ) -> Result<HashSet<String>> {
        let (res, src) = task_result.map_err(|err| anyhow!("fetch task panicked: {err}"))?;
        let bytes = res.map_err(|err| anyhow!("failed to fetch {src}: {err}"))?;

        // write the data to the source path
        self.writer.write_js(src.to_string(), &bytes).await?;

        // create new JsSource object from data
        let js_source = JsSource::new(String::from_utf8(bytes)?);

        // parse the source and return any chunk urls found
        js_source.parse()
    }
}

// spawn a task to fetch each source found
// for source in sources {
//     let Some(src) = self.resolve_source(source) else {
//         continue;
//     };
//
//     if !seen.insert(src.clone()) {
//         continue;
//     }
//
//     tracing::info!("discovered {}", src);
//
//     join_set.spawn(async move {
//         let res = util::fetch_url(&src).await;
//         (res, src)
//     });
// }
//
//
//
// // resolves `src` against the site's domain, records it in `all_sources`
// // regardless of the outcome, and returns the fetchable url unless it's
// // cross-origin and external sources aren't allowed
// fn resolve_source(&self, src: &str) -> Option<String> {
//     // let (host, path) = match util::resolve_source_url(&self.domain, src) {
//     //     Ok(resolved) => resolved,
//     //     Err(err) => {
//     //         tracing::error!("failed to resolve source url {src}: {err:#}");
//     //         return None;
//     //     }
//     // };
//
//     let full_url = format!("{host}{path}");
//     self.all_sources.lock().unwrap().push(full_url.clone());
//
//     tracing::debug!("resolved {} to {}", src, full_url);
//
//     if host != self.domain && !self.allow_external_sources {
//         tracing::info!("discarding cross-origin source: {full_url}");
//         return None;
//     }
//
//     Some(full_url)
// }
