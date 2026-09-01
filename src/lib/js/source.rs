use std::collections::HashSet;

use anyhow::{Result, anyhow};
use oxc::{allocator::Allocator, parser::Parser, span::SourceType};

use crate::js::visitor;

// results of parsing one js source: chunk urls (already resolved against the
// site), any http endpoints referenced via recognized request calls
// (fetch/axios/xhr/jquery - not resolved against anything, just the static
// skeleton of whatever was passed as the url argument), and any string
// literals in the source that look like absolute http/https urls
pub struct ParseResult {
    pub chunk_urls: HashSet<String>,
    pub endpoints: HashSet<String>,
    pub urls: HashSet<String>,
}

pub struct JsSource {
    source_text: String,
    url: reqwest::Url,
}

impl JsSource {
    pub fn new(source_text: String, url: reqwest::Url) -> Self {
        Self { source_text, url }
    }

    pub fn parse(&self) -> Result<ParseResult> {
        // create allocator with 1.5kB
        let allocator = Allocator::with_capacity(self.source_text.len() + 1);
        // use default source_type
        let source_type = SourceType::default();
        // parse source string
        let parsed = Parser::new(&allocator, &self.source_text, source_type).parse();
        // none if panicked
        if parsed.panicked {
            return Err(anyhow!("parser panicked"));
        }

        let mut parse_result = visitor::JsVisitor::new().parse(parsed.program);

        let chunk_urls = parse_result
            .chunk_urls
            .iter()
            .map(|raw| self.resolve_chunk_url(raw))
            .collect();

        parse_result.chunk_urls = chunk_urls;

        Ok(parse_result)
    }

    // re-roots a bundler-relative chunk path (e.g. "static/chunks/558.hash.js")
    // under whatever mount prefix this source file's own url implies - the
    // chunk-loader code itself only ever contains a bundler-relative path, but
    // the file we're currently parsing must itself be served from within the
    // same mount as the chunks it loads, so we can recover the mount by finding
    // where the chunk path's own first segment (e.g. "static/") shows up in
    // this source's path, then rerooting from there - matching on just the
    // first segment (rather than the raw path's full leading directory)
    // handles chunk paths nested deeper than the source file itself, e.g. a
    // next.js build manifest living at ".../static/<buildId>/_buildManifest.js"
    // referencing chunks at "static/chunks/pages/foo.js": the manifest's own
    // path only shares "static/" with the chunk path, not "static/chunks/pages/".
    // falls back to joining the raw path directly against this source's url if
    // no matching segment is found.
    fn resolve_chunk_url(&self, raw: &str) -> String {
        let resolved = match raw.find('/') {
            Some(slash_idx) => {
                let first_segment = &raw[..=slash_idx];
                let source_path = self.url.path();

                match source_path.find(first_segment) {
                    Some(mount_end) => self
                        .url
                        .join(&format!("{}{raw}", &source_path[..mount_end])),
                    None => self.url.join(raw),
                }
            }
            None => self.url.join(raw),
        };

        match resolved {
            Ok(url) => url.to_string(),
            Err(_) => raw.to_string(),
        }
    }
}
