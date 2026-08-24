# Class Diagram

```mermaid
classDiagram
    class Cli {
        <<clap Parser>>
        +domain: String
        +out_dir: PathBuf
        +allow_external_sources: bool
    }

    class App {
        -domain: String "unused"
        -site: Site
        +from_cli(cli: Cli) Result~App~
        +run() Result~()~
    }

    class Site {
        -url: reqwest::Url
        -allow_external_sources: bool
        -writer: Writer
        -client: reqwest::Client
        -endpoints: Mutex~HashSet~String~~
        +new(writer, url) Site
        +allow_external_sources() Site
        +enumerate() Result~()~
        -fetch_html() Result~Document~
        -resolve_sources(source_strs) Vec~Url~
        -apply_source_filters(sources) Vec~Url~
        -filter_external(sources) Vec~Url~
        -handle_sources(sources) Result~()~
        -handle_js_payload(join_set, task_result, seen) Result~()~
        -parse_chunk_urls(task_result) Result~Vec~Url~~
        -write_endpoints() Result~()~
    }

    class Writer {
        -base_path: PathBuf
        -tx: mpsc::UnboundedSender~Task~
        -consumer: Mutex~Option~JoinHandle~~
        +new(base_path, deadline) Writer
        +write(path, data) Result~()~
        +write_js(path, data) Result~()~ "formats via prettier first"
        +write_txt(path, data) Result~()~
        +write_html(path, data) Result~()~
        +write_ext(path, data, ext) Result~()~
        +join() "awaits background consumer, deadline-bounded"
        -write_with(path, data, format) Result~()~
    }

    class run_consumer {
        <<free fn, background task spawned by Writer::new>>
        "fans Task::Write jobs into an internal JoinSet;\non Shutdown, drains with a timeout,\naborting stragglers past the deadline"
    }

    class Document {
        -document: scraper::Html
        +new(bytes) Result~Document~
        -script_sources() HashSet~String~
        -link_sources() HashSet~String~
        +sources() Vec~String~
    }

    class ParseResult {
        <<js::source::ParseResult - single shared type,\nreturned by both JsSource::parse()\nand JsVisitor::parse()>>
        +chunk_urls: HashSet~String~
        +endpoints: HashSet~String~
    }

    class JsSource {
        <<js::source, pub>>
        -source_text: String
        -url: reqwest::Url
        +new(source_text, url) JsSource
        +parse() Result~ParseResult~ "re-resolves chunk_urls in place\nagainst this source's mount"
        -resolve_chunk_url(raw) String
    }

    class JsVisitor {
        <<js::visitor, implements oxc::Visit>>
        +chunk_urls: HashSet~String~
        +endpoints: HashSet~String~
        +new() JsVisitor
        +parse(program) ParseResult
        +visit_assignment_expression(it) "dispatches to js::webpack"
        +visit_call_expression(it) "dispatches to js::turbopack + js::endpoints"
    }

    class js_webpack {
        <<js::webpack, free functions>>
        +chunk_urls(arrow_fn) Vec~String~
        -unwrap_literal_chunk_override(expr, out) Expression
        -conditional_override(c) (Option~String~,String)
        -object_props(computed) Vec~(String,String)~
        -flatten_binary_plus_chain(expr, out)
        -arrow_body_expression(arrow_fn) Option~Expression~
    }

    class js_turbopack {
        <<js::turbopack, free functions>>
        +turbopack_chunk_urls(call) Vec~String~
        -find_other_chunks(obj) Option~ArrayExpression~
    }

    class js_endpoints {
        <<js::endpoints, free functions>>
        +extract_endpoint(call) Option~String~
        -find_property_string(obj, key) Option~Expression~
        -url_expression_to_string(expr) Option~String~
        -argument_url_string(arg) Option~String~
        -template_literal_skeleton(t) String
    }

    class webpack_module {
        <<src/lib/webpack.rs - DEAD CODE, declared, never called>>
        +extract_chunk_map(source) Option~(ChunkMap, Option~String~)~
        +chunk_filenames(chunk_map, public_path) Vec~String~
    }

    class util {
        <<free functions, stateless>>
        +full_path(path) Result~PathBuf~
        +fetch_url(url) Result~Vec~u8~~
        +write_file(full_path, data) Result~()~
        +strip_quotes(raw) String
        +format_with_prettier_stdin(path, data) Result~Vec~u8~~
        +format_with_prettier(path) Result~()~ "dead: superseded by stdin variant"
        +resolve_source_url(domain, src) Result~(String,String)~ "dead: superseded by Site's own url resolution"
    }

    class cli_bin {
        <<binary entrypoint>>
        +main()
    }

    cli_bin ..> App : "calls batter::start()"
    App "1" *-- "1" Cli : constructed from
    App "1" *-- "1" Site : owns
    Site "1" *-- "1" Writer : owns
    Writer ..> run_consumer : "spawns in new()"
    run_consumer ..> util : write_file(), format_with_prettier_stdin()
    Site ..> Document : creates in fetch_html()
    Site ..> JsSource : creates per fetched js source
    Site ..> util : fetch_url()
    JsSource ..> ParseResult : returns from parse()
    JsSource ..> JsVisitor : creates in parse()
    JsVisitor ..> ParseResult : returns from its own parse()
    JsVisitor ..> js_webpack : chunk_urls()
    JsVisitor ..> js_turbopack : turbopack_chunk_urls()
    JsVisitor ..> js_endpoints : extract_endpoint()
    js_webpack ..> util : strip_quotes()
    js_turbopack ..> util : strip_quotes()
    js_endpoints ..> util : strip_quotes()
    App ..> webpack_module : "mod declared, unused"
```

## Notes

Updated after commit `c2c5607` ("refactoring some js/ stuff") and a couple of
follow-up fixes made directly on top of it while producing this diagram. The
public surface (`Site`, `Writer`, and the crate-level module wiring) is unchanged
from before; what moved is entirely internal to `js/`:

- **`js/mod.rs`** is now pure module declarations: `mod endpoints; pub mod
  source; mod turbopack; mod visitor; mod webpack;`.
- **`JsSource` and `ParseResult` live in `js/source.rs`**, and `JsSource` is no
  longer re-exported at the `js` module root — callers need
  `crate::js::source::JsSource` (confirmed via `site.rs`'s import). The old
  `crate::js::JsSource` path no longer resolves.
- **`JsWalker` was renamed to `JsVisitor` and moved to `js/visitor.rs`**, with a
  `parse(program) -> ParseResult` method doing the `visit_program` +
  result-extraction step previously inlined in `JsSource::parse`. `ParseResult`
  is a single shared type (`js::source::ParseResult`, imported into `visitor.rs`
  via `use crate::js::source::ParseResult`) rather than two separate structs —
  `JsSource::parse` takes the `ParseResult` `JsVisitor::parse` returns and
  mutates `chunk_urls` in place to re-resolve each one against the source's
  mount, leaving `endpoints` untouched.
- **Found and fixed while updating this diagram**: `js/webpack.rs` and
  `js/turbopack.rs`'s test modules still imported the now-nonexistent
  `crate::js::JsSource` (a leftover from before `JsSource` moved to
  `js::source`), which broke `cargo test --lib` on `main`. Updated both to
  `crate::js::source::JsSource`; all 27 tests pass again.
- Same two dead-code pockets as before: the top-level `src/lib/webpack.rs` module
  (superseded by `js::webpack`, never called from anywhere), and two functions in
  `util.rs` — `format_with_prettier` (superseded by the stdin-based variant used
  by `Writer`) and `resolve_source_url` (superseded by `Site`'s own
  `reqwest::Url`-based resolution).
