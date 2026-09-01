use anyhow::Result;
use clap::Parser;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::site::Site;

mod domain;
mod html;
mod js;
mod site;
mod util;
mod writer;

// how many subdomains' crawls are allowed to run at once when
// --enumerate-subdomains is passed - each Site::enumerate() already fans out
// its own internal set of concurrent fetch tasks, so this bounds how many of
// those run in parallel across sites rather than letting every discovered
// subdomain's crawl start at once
const MAX_CONCURRENT_SITES: usize = 4;

// how long the writer waits, once a crawl finishes, for any still-in-flight
// background writes (disk I/O, prettier formatting) to complete before
// aborting the stragglers - configurable via --write-deadline since larger
// sites can have enough queued writes that 10s isn't always enough
const DEFAULT_WRITER_DEADLINE_SECS: u64 = 60;

#[derive(Parser, Debug)]
struct Cli {
    // target to crawl - a bare host ("example.com") or a full url, optionally
    // with a path ("example.com/app", "https://example.com/app"). when a
    // path is given, only the initial html fetch for this target starts
    // there; every other target (subdomains discovered via
    // --enumerate-subdomains) still starts at its own root, and all
    // downstream resolution (js sources, chunk urls, external-source
    // filtering) stays anchored to the bare host regardless.
    domain: String,
    #[arg(default_value = ".")]
    out_dir: PathBuf,
    // by default, sources not on the site's domain are discarded; pass this to
    // fetch them anyway
    #[arg(long)]
    allow_external_sources: bool,
    // discover subdomains via subfinder and crawl each one (plus the root
    // domain) instead of just the root domain
    #[arg(long)]
    enumerate_subdomains: bool,
    // seconds to wait for pending background writes to finish after a
    // crawl completes, before aborting whatever's left
    #[arg(long, default_value_t = DEFAULT_WRITER_DEADLINE_SECS)]
    write_deadline: u64,
}

struct App {
    // bare host parsed out of the cli's target - used for subdomain
    // enumeration, out_dir naming, and targets.txt
    domain: String,
    // the exact url the root target's initial html fetch should hit - keeps
    // whatever path/query the user passed on the cli, if any
    start_url: reqwest::Url,
    out_dir: PathBuf,
    allow_external_sources: bool,
    enumerate_subdomains: bool,
    write_deadline: Duration,
}

impl App {
    fn from_cli(cli: Cli) -> Result<Self> {
        let out_dir = util::full_path(cli.out_dir)?;
        let start_url = parse_target_url(&cli.domain)?;

        let domain = start_url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("target url {start_url} has no host"))?
            .to_string();

        Ok(Self {
            domain,
            start_url,
            out_dir,
            allow_external_sources: cli.allow_external_sources,
            enumerate_subdomains: cli.enumerate_subdomains,
            write_deadline: Duration::from_secs(cli.write_deadline),
        })
    }

    // builds the list of domains to crawl: always the root domain, plus
    // whatever subfinder reports if subdomain enumeration was requested. a
    // subfinder failure is logged and treated as "nothing extra discovered"
    // rather than aborting the run, since the user still wants the root
    // domain crawled either way.
    async fn target_domains(&self) -> Vec<String> {
        let mut domains = vec![self.domain.clone()];

        if self.enumerate_subdomains {
            match domain::enumerate_subdomains(&self.domain).await {
                Ok(subdomains) => domains.extend(subdomains),
                Err(err) => {
                    tracing::warn!(
                        "subdomain enumeration failed, continuing with just {}: {err:#}",
                        self.domain
                    );
                }
            }
        }

        domains
    }

    // writes every target domain to {out_dir}/targets.txt, one per line,
    // before any Site is built for them
    async fn write_targets(&self, targets: &[String]) -> Result<()> {
        util::write_file(
            self.out_dir.join("targets.txt"),
            targets.join("\n").as_bytes(),
        )
        .await
    }

    fn build_site(&self, target: &str) -> Result<Site> {
        let writer = writer::Writer::new(self.out_dir.join(target), self.write_deadline);

        // only the originally-typed target starts at whatever path the user
        // passed on the cli - a discovered subdomain has no relationship to
        // that path, so it always starts at its own root
        let url = if target == self.domain {
            self.start_url.clone()
        } else {
            reqwest::Url::parse(&format!("https://{target}"))?
        };

        let mut site = Site::new(writer, url);

        if self.allow_external_sources {
            site = site.allow_external_sources();
        }

        Ok(site)
    }

    async fn run(&mut self) -> Result<()> {
        tracing::info!("fetching targets...");
        let targets = self.target_domains().await;

        tracing::info!("crawling {} target(s): {:?}", targets.len(), targets);

        if let Err(err) = self.write_targets(&targets).await {
            tracing::warn!("failed to write targets.txt: {err:#}");
        }

        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_SITES));
        let mut join_set = JoinSet::new();

        for target in targets {
            let mut site = match self.build_site(&target) {
                Ok(site) => site,
                Err(err) => {
                    tracing::error!("failed to set up crawl for {target}: {err:#}");
                    continue;
                }
            };

            let semaphore = semaphore.clone();

            join_set.spawn(async move {
                // hold the permit for the duration of this site's crawl so
                // at most MAX_CONCURRENT_SITES run at once
                let _permit = semaphore.acquire_owned().await;
                let result = site.enumerate().await;
                (target, result)
            });
        }

        while let Some(task_result) = join_set.join_next().await {
            match task_result {
                Ok((target, Ok(()))) => tracing::info!("finished crawling {target}"),
                Ok((target, Err(err))) => tracing::error!("crawl of {target} failed: {err:#}"),
                Err(err) => tracing::error!("crawl task panicked: {err}"),
            }
        }

        Ok(())
    }
}

// parses the cli's target into a full url, defaulting to https when no
// scheme was given - accepts a bare host ("example.com"), a host with a path
// ("example.com/app"), or an already-complete url ("https://example.com/app")
fn parse_target_url(target: &str) -> Result<reqwest::Url> {
    if let Ok(url) = reqwest::Url::parse(target) {
        return Ok(url);
    }

    reqwest::Url::parse(&format!("https://{target}"))
        .map_err(|err| anyhow::anyhow!("invalid target {target:?}: {err}"))
}

#[tokio::main]
pub async fn start() {
    let started_at = std::time::Instant::now();

    // parse .env
    dotenvy::dotenv().ok();

    // initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // parse cli args
    let args = Cli::parse();

    // construct config
    let mut app = App::from_cli(args).unwrap();

    app.run().await.unwrap();

    tracing::info!("finished in {:?}", started_at.elapsed());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_host() {
        let url = parse_target_url("example.com").unwrap();
        assert_eq!(url.as_str(), "https://example.com/");
        assert_eq!(url.host_str(), Some("example.com"));
        assert_eq!(url.path(), "/");
    }

    #[test]
    fn parses_bare_host_with_path() {
        let url = parse_target_url("example.com/app").unwrap();
        assert_eq!(url.as_str(), "https://example.com/app");
        assert_eq!(url.host_str(), Some("example.com"));
        assert_eq!(url.path(), "/app");
    }

    #[test]
    fn preserves_explicit_scheme_and_path() {
        let url = parse_target_url("https://example.com/app?x=1").unwrap();
        assert_eq!(url.as_str(), "https://example.com/app?x=1");
    }

    #[test]
    fn preserves_explicit_http_scheme() {
        let url = parse_target_url("http://example.com").unwrap();
        assert_eq!(url.scheme(), "http");
    }
}
