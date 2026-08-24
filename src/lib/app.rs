use anyhow::Result;
use clap::Parser;
use std::fmt::Debug;
use std::path::PathBuf;
use std::time::Duration;

use crate::site::Site;

mod domain;
mod html;
mod js;
mod site;
mod util;
mod webpack;
mod writer;

#[derive(Parser, Debug)]
struct Cli {
    domain: String,
    #[arg(default_value = ".")]
    out_dir: PathBuf,
    // by default, sources not on the site's domain are discarded; pass this to
    // fetch them anyway
    #[arg(long)]
    allow_external_sources: bool,
}

struct App {
    domain: String,
    site: Site,
}

impl App {
    fn from_cli(cli: Cli) -> Result<Self> {
        let out_dir = util::full_path(cli.out_dir)?;

        let site_writer = writer::Writer::new(out_dir.join(&cli.domain), Duration::from_secs(10));

        let url = reqwest::Url::parse(&format!("https://{}", &cli.domain))?;

        let mut site = Site::new(site_writer, url);

        if cli.allow_external_sources {
            site = site.allow_external_sources();
        }

        Ok(Self {
            domain: cli.domain,
            site,
        })
    }

    async fn run(&mut self) -> Result<()> {
        self.site.enumerate().await?;

        Ok(())
    }
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
