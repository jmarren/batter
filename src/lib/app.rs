use anyhow::Result;
use clap::Parser;
use std::fmt::Debug;
use std::path::PathBuf;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::JoinSet;

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

// tracing::info!("args = {:?}", args);
//
// args.out_dir = util::full_path(args.out_dir).unwrap();

// tracing::info!("cwd = {:?}", cwd);
//
//
//    // biome_js_formatter::format_node(options, root)

// let source = biome_js_syntax::JsFileSource::js_module();
// let opts = biome_js_formatter::context::JsFormatOptions::new(source);
//
// let ctx = biome_js_formatter::context::JsFormatContext::new(
//     opts,
//     biome_js_formatter::comments::JsComments::default(),
// );
//
// let mut state = FormatState::new(SimpleFormatContext::default());
// let mut buffer = VecBuffer::new(&mut state);
//
// write!(, args)
//
// // buffer.write_fmt(format_args!(text(bytes.into()))).unwrap();
//
// let mut formatter = biome_formatter::formatter::Formatter::new(&mut buffer);
//
// let x = ctx.fmt(&mut formatter.into());
//
// println!("formatted = {:?}", x);

// formatter.write_fmt(arguments)

// write!(&mut buffer, [text("Hello"), space()])?;
// write!(&mut buffer, [text("World")])?;
// formatter.write_fmt(arguments);

// let opts = biome_js_formatter::JsFormatLanguage::new(source);

// let source = biome_js_syntax::JsFileSource::from(bytes);

// source.type_id

// let source_text = String::from_utf8(bytes)?;
//
// let source_type = SourceType::default();
//
// let parser_return = oxc::parser::Parser::new(&alloc, &source_text, source_type).parse();
//
// // oxc::
//
// // oxfmt::core
//
// // oxfmt::
//
// // parser_return.
//
// println!("parser_return = {:?}", parser_return);
//
//
// async fn fetch_js_sources(&self, sources: &[String]) -> Result<Vec<(String, Vec<u8>)>> {
//     let mut join_set = JoinSet::new();
//
//     // spawn a task to fetch each source found
//     for i in 0..sources.len() {
//         let src = format!("{}{}", self.domain, sources[i]);
//         join_set.spawn(async move {
//             let res = util::fetch_url(&src).await;
//             (res, src)
//         });
//     }
//
//     // let alloc = oxc::allocator::Allocator::new();
//
//     let mut prettier_set = JoinSet::new();
//     let mut fetched = Vec::new();
//
//     // iterate over results and write to file if ok
//     while let Some(task_result) = join_set.join_next().await {
//         let Ok(out) = task_result else {
//             continue;
//         };
//
//         let (res, src) = out;
//
//         let Ok(bytes) = res else {
//             continue;
//         };
//
//         // TODO: pass string to prettier command before writing instead of editing file in
//         // place
//
//         let src = if src.ends_with(".js") {
//             src
//         } else {
//             format!("{src}.js")
//         };
//
//         self.write_file(src.clone().into(), &bytes).await?;
//
//         let written_path = self.out_dir.join(&src);
//
//         tracing::info!("written_path = {:?}", written_path);
//
//         // spawn a task to run prettier on this file so files can be formatted in parallel
//         prettier_set.spawn(async move {
//             tokio::process::Command::new("npx")
//                 .arg("prettier")
//                 .arg("--write")
//                 .arg(&written_path)
//                 .status()
//                 .await
//         });
//
//         fetched.push((src, bytes));
//     }
//
//     // // join all prettier tasks so none are left running as orphans
//     while let Some(task_result) = prettier_set.join_next().await {
//         println!("task_result = {:?}", task_result);
//         let _ = task_result?;
//     }
//
//     Ok(fetched)
// }
// //
//     // scans fetched js bundles for a webpack chunk map, and logs any chunk whose
//     // filename wasn't among the sources we already fetched
//     fn report_missing_chunks(&self, fetched: &[(String, Vec<u8>)]) {
//         // compare by basename since fetched sources are full (domain-prefixed) urls
//         // while reconstructed chunk filenames are not
//         let fetched_basenames: std::collections::HashSet<&str> = fetched
//             .iter()
//             .filter_map(|(src, _)| src.rsplit('/').next())
//             .collect();
//
//         for (src, bytes) in fetched {
//             // convert bytes to string
//             let Ok(text) = std::str::from_utf8(bytes) else {
//                 continue;
//             };
//
//             //
//             let Some((chunk_map, public_path)) = webpack::extract_chunk_map(text) else {
//                 continue;
//             };
//
//             tracing::info!(
//                 "found webpack chunk map with {} entries in {}",
//                 chunk_map.len(),
//                 src
//             );
//
//             let known = webpack::chunk_filenames(&chunk_map, public_path.as_deref());
//
//             let missing: Vec<&String> = known
//                 .iter()
//                 .filter(|filename| {
//                     let basename = filename.rsplit('/').next().unwrap_or(filename.as_str());
//                     !fetched_basenames.contains(basename)
//                 })
//                 .collect();
//
//             if !missing.is_empty() {
//                 tracing::warn!("missing chunks referenced by {}: {:?}", src, missing);
//             }
//         }
//     }
//
