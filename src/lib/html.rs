use anyhow::Result;
use scraper::{Html, Selector};

pub struct Document {
    document: Html,
}

impl Document {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        let document = scraper::Html::parse_document(&String::from_utf8(bytes.into())?);

        Ok(Self { document })
    }

    pub fn sources(&self) -> Vec<String> {
        let script_selector = Selector::parse("script").unwrap();

        let scripts: Vec<_> = self.document.select(&script_selector).collect();
        tracing::info!("found {} <script src> sources in document", scripts.len());

        let srcs: Vec<_> = scripts
            .iter()
            .map(|s| s.value().attr("src"))
            .filter(|s| s.is_some())
            .map(|s| s.unwrap().to_string())
            .collect();

        srcs
    }
}
