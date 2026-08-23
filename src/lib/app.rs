use anyhow::Result;
use clap::Parser;
use std::fmt::Debug;
use std::path::PathBuf;

use crate::site::Site;

mod html;
mod js;
mod site;
mod util;
mod webpack;

#[derive(Parser, Debug)]
struct Cli {
    domain: String,
    #[arg(default_value = ".")]
    out_dir: PathBuf,
}

struct App {
    domain: String,
    site: Site,
}

impl App {
    fn from_cli(cli: Cli) -> Result<Self> {
        let out_dir = util::full_path(cli.out_dir)?;

        let site = Site::new(out_dir, cli.domain.clone());

        Ok(Self {
            // out_dir,
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
pub async fn run() {
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
}
