use std::collections::HashSet;
use std::sync::Mutex;

use anyhow::{Result, anyhow};
use tokio::task::{JoinError, JoinSet};

use crate::{
    html,
    js::source::JsSource,
    util::{self},
    writer::Writer,
};

pub struct Site {
    url: reqwest::Url,
    allow_external_sources: bool,
    writer: Writer,
    client: reqwest::Client,
    // http endpoints referenced by fetched js, accumulated across the whole
    // crawl and written to endpoints.txt at the end. needs interior
    // mutability since parse_chunk_urls is &self and runs concurrently
    // across many spawned tasks.
    endpoints: Mutex<HashSet<String>>,
    // absolute http/https urls found in string literals in fetched js,
    // accumulated across the whole crawl and written to urls.txt at the end.
    // same interior mutability rationale as endpoints.
    urls: Mutex<HashSet<String>>,
}

impl Site {
    pub fn new(writer: Writer, url: reqwest::Url) -> Self {
        Self {
            url,
            writer,
            client: reqwest::Client::new(),
            allow_external_sources: false,
            endpoints: Mutex::new(HashSet::new()),
            urls: Mutex::new(HashSet::new()),
        }
    }

    pub fn allow_external_sources(mut self) -> Self {
        self.allow_external_sources = true;
        self
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

        self.writer.write("index.html", &res)?;

        // create document
        Ok(html::Document::new(&res)?)
    }

    /// resolves a list of js source urls to reqwest::Urls
    /// using self.url
    fn resolve_sources(&self, source_strs: Vec<String>) -> Vec<reqwest::Url> {
        source_strs
            .iter()
            // join with base url.
            .filter_map(|src| match self.url.join(src) {
                Ok(x) => Some(x),
                // filter out and report if failed
                Err(e) => {
                    tracing::error!("{:?}", e);
                    None
                }
            })
            .collect()
    }

    // applies filters to a list of sources
    fn apply_source_filters(&self, sources: Vec<reqwest::Url>) -> Vec<reqwest::Url> {
        if self.allow_external_sources {
            sources
        } else {
            self.filter_external(sources)
        }
    }

    // filter cross origin urls out of a list of urls
    fn filter_external(&self, sources: Vec<reqwest::Url>) -> Vec<reqwest::Url> {
        sources
            .into_iter()
            .filter(|src| src.domain() == self.url.domain())
            .collect()
    }

    // run enumeration on the site
    pub async fn enumerate(&mut self) -> Result<()> {
        // fetch html. html::Document wraps scraper::Html, which isn't Send,
        // so it's scoped to end before the next await point - otherwise the
        // enclosing future isn't Send (needed to run each site's crawl as
        // its own spawned task when crawling multiple domains concurrently)
        let doc_sources = {
            let doc = self.fetch_html().await?;
            doc.sources()
        };

        // resolve sources to reqwest::Urls
        let all_sources = self.resolve_sources(doc_sources);

        // apply any necessary filters
        let sources = self.apply_source_filters(all_sources);

        // handle sources
        self.handle_sources(sources).await?;

        // write every discovered http endpoint to endpoints.txt
        self.write_endpoints()?;

        // write every discovered url to urls.txt
        self.write_urls()?;

        tracing::info!("joining writer");

        // wait for all background writes to finish (bounded by the writer's deadline)
        self.writer.join().await;

        Ok(())
    }

    fn write_endpoints(&self) -> Result<()> {
        let endpoints: Vec<String> = self.endpoints.lock().unwrap().iter().cloned().collect();

        if endpoints.is_empty() {
            return Ok(());
        }

        self.writer
            .write("endpoints.txt", endpoints.join("\n").as_bytes())
    }

    fn write_urls(&self) -> Result<()> {
        let urls: Vec<String> = self.urls.lock().unwrap().iter().cloned().collect();

        if urls.is_empty() {
            return Ok(());
        }

        self.writer.write("urls.txt", urls.join("\n").as_bytes())
    }

    // handle source urls returned from document
    async fn handle_sources(&self, sources: Vec<reqwest::Url>) -> Result<()> {
        let mut join_set = JoinSet::new();
        let mut seen: HashSet<String> = HashSet::new();

        // dedup sources and spawn a task for each
        sources
            .into_iter()
            .filter(|src| seen.insert(src.to_string()))
            .for_each(|src| {
                tracing::info!("discovered {}", src);
                join_set.spawn(fetch_js_source(src));
            });

        // handle each response and log any errors that occur
        while let Some(task_result) = join_set.join_next().await {
            if let Err(e) = self
                .handle_js_payload(&mut join_set, task_result, &mut seen)
                .await
            {
                tracing::error!("{:?}", e);
            }
        }

        Ok(())
    }

    async fn handle_js_payload(
        &self,
        join_set: &mut JoinSet<(Result<Vec<u8>>, reqwest::Url)>,
        task_result: Result<(Result<Vec<u8>>, reqwest::Url), JoinError>,
        seen: &mut HashSet<String>,
    ) -> Result<()> {
        let chunk_urls = self.parse_chunk_urls(task_result).await?;

        for chunk_url in chunk_urls {
            if seen.insert(chunk_url.to_string()) {
                join_set.spawn(fetch_js_source(chunk_url));
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
        task_result: Result<(Result<Vec<u8>>, reqwest::Url), JoinError>,
    ) -> Result<Vec<reqwest::Url>> {
        let (res, src_url) = task_result.map_err(|err| anyhow!("fetch task panicked: {err}"))?;
        let bytes = res.map_err(|err| anyhow!("failed to fetch {src_url}: {err}"))?;

        // write the data to the source path
        self.writer.write_js(src_url.to_string(), &bytes)?;

        // create new JsSource object from data
        let js_source = JsSource::new(String::from_utf8(bytes)?, src_url.clone());

        // parse the source, record any endpoints found, and return the
        // chunk urls found so the caller can fetch them
        let parsed = js_source.parse()?;

        self.endpoints.lock().unwrap().extend(parsed.endpoints);
        self.urls.lock().unwrap().extend(parsed.urls);

        let url_strs = parsed.chunk_urls.into_iter().collect();

        Ok(self.resolve_sources(url_strs))
    }
}

// fetches the provided url and returns the url along with the response bytes
async fn fetch_js_source(src: reqwest::Url) -> (Result<Vec<u8>>, reqwest::Url) {
    let res = util::fetch_url(src.clone()).await;
    (res, src)
}
