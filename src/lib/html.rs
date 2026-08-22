use anyhow::Result;
use scraper::{Html, Selector};
use std::collections::HashSet;
use tokio::sync::mpsc::Sender;

pub struct Document {
    document: Html,
}

impl Document {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        let document = scraper::Html::parse_document(&String::from_utf8(bytes.into())?);

        Ok(Self { document })
    }

    fn script_sources(&self) -> HashSet<String> {
        let script_selector = Selector::parse("script").unwrap();

        let scripts: Vec<_> = self.document.select(&script_selector).collect();
        tracing::info!("found {} <script src> sources in document", scripts.len());

        let srcs: HashSet<_> = scripts
            .iter()
            .map(|s| s.value().attr("src"))
            .filter(|s| s.is_some())
            .map(|s| s.unwrap().to_string())
            .collect();

        srcs
    }

    fn link_sources(&self) -> HashSet<String> {
        let link_script_selector = Selector::parse("link[as='script']").unwrap();

        let link_scripts: Vec<_> = self.document.select(&link_script_selector).collect();
        tracing::info!(
            "found {} <script src> sources in document",
            link_scripts.len()
        );

        let link_srcs: HashSet<_> = link_scripts
            .iter()
            .map(|s| s.value().attr("href"))
            .filter(|s| s.is_some())
            .map(|s| s.unwrap().to_string())
            .collect();

        link_srcs
    }

    // TODO:
    // these can be collected in one pass
    pub fn sources(&self) -> Vec<String> {
        let mut sources = self.script_sources();
        sources.extend(self.link_sources());

        sources.into_iter().collect()
    }
}
