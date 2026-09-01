use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use tokio::task::{JoinError, JoinSet};

use crate::{
    html,
    js::source::JsSource,
    util::{self},
    writer::Writer,
};

// reqwest's default redirect policy (10 hops, loop detection) but recording
// each hop's status and target url as it's followed, so the caller can
// report the full chain instead of only ever seeing the final response.
const MAX_REDIRECTS: usize = 10;

fn redirect_policy(log: Arc<Mutex<Vec<String>>>) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        log.lock()
            .unwrap()
            .push(format!("{} -> {}", attempt.status(), attempt.url()));

        if attempt.previous().len() >= MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    })
}

pub struct Site {
    // where the initial html request is actually sent - may carry a path
    // (e.g. a site that only serves its app under "/app") if the caller
    // wants the crawl to start somewhere other than the root.
    fetch_url: reqwest::Url,
    // root of the site (fetch_url with path/query/fragment stripped) - every
    // other url (js source resolution, external-source filtering, chunk
    // rerooting) stays anchored here regardless of where the initial fetch
    // started, so a non-root start path doesn't throw off downstream logic.
    base_url: reqwest::Url,
    allow_external_sources: bool,
    writer: Writer,
    client: reqwest::Client,
    // hops recorded by the initial html request's redirect policy, in the
    // order they were followed. populated synchronously during fetch_html,
    // before any concurrent js-fetching starts, so this doesn't need a mutex
    // the way endpoints/urls do.
    redirects: Arc<Mutex<Vec<String>>>,
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
    // `fetch_url` is requested as-is for the initial html fetch, so it may
    // carry a path/query if the caller wants the crawl to start somewhere
    // other than the root. everything downstream (source resolution,
    // external-source filtering, chunk rerooting) is anchored to its root
    // instead, so a non-root start path never throws off that logic.
    pub fn new(writer: Writer, fetch_url: reqwest::Url) -> Self {
        let mut base_url = fetch_url.clone();
        base_url.set_path("");
        base_url.set_query(None);
        base_url.set_fragment(None);

        let redirects = Arc::new(Mutex::new(Vec::new()));

        let client = reqwest::Client::builder()
            .redirect(redirect_policy(redirects.clone()))
            .build()
            // only fails on tls backend init, which reqwest::Client::new()
            // would also panic on - matches its own documented behavior
            .expect("failed to build http client");

        Self {
            fetch_url,
            base_url,
            writer,
            client,
            redirects,
            allow_external_sources: false,
            endpoints: Mutex::new(HashSet::new()),
            urls: Mutex::new(HashSet::new()),
        }
    }

    pub fn allow_external_sources(mut self) -> Self {
        self.allow_external_sources = true;
        self
    }

    // fetch html from domain, write it (and the response headers/any
    // redirects followed to get it) to file, and return document struct
    async fn fetch_html(&mut self) -> Result<html::Document> {
        let response = self.client.get(self.fetch_url.clone()).send().await?;

        self.write_headers(response.headers())?;
        self.write_redirects()?;

        let res = response.bytes().await?;

        self.writer.write("index.html", &res)?;

        // create document
        Ok(html::Document::new(&res)?)
    }

    // writes the initial html request's response headers to headers.txt, one
    // "name: value" per line
    fn write_headers(&self, headers: &reqwest::header::HeaderMap) -> Result<()> {
        let lines: Vec<String> = headers
            .iter()
            .map(|(name, value)| format!("{name}: {}", value.to_str().unwrap_or("<invalid>")))
            .collect();

        self.writer
            .write("headers.txt", lines.join("\n").as_bytes())
    }

    // writes every redirect hop followed while fetching the initial html to
    // redirects.txt, one "<status> -> <location>" per line in the order they
    // were followed. skipped if the request landed with no redirects.
    fn write_redirects(&self) -> Result<()> {
        let redirects = self.redirects.lock().unwrap();

        if redirects.is_empty() {
            return Ok(());
        }

        self.writer
            .write("redirects.txt", redirects.join("\n").as_bytes())
    }

    /// resolves a list of js source urls to reqwest::Urls
    /// using self.base_url
    fn resolve_sources(&self, source_strs: Vec<String>) -> Vec<reqwest::Url> {
        source_strs
            .iter()
            // join with base url.
            .filter_map(|src| match self.base_url.join(src) {
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
            .filter(|src| src.domain() == self.base_url.domain())
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
