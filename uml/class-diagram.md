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
        -domain: String
        -site: Site
        +from_cli(cli: Cli) Result~App~
        +run() Result~()~
    }

    class Site {
        -domain: String
        -out_dir: PathBuf
        -allow_external_sources: bool
        -prettier_handles: Mutex~Vec~JoinHandle~~
        -all_sources: Mutex~Vec~String~~
        +new(out_dir, domain, allow_external_sources) Site
        +enumerate() Result~()~
        -resolve_source(src) Option~String~
        -write_all_sources() Result~()~
        -join_prettier_handles()
        -fetch_html() Result~Document~
        -handle_sources(sources) Result~()~
        -handle_source_response(task_result) Result~HashSet~String~~
        -write_index(data) Result~()~
        -write_file(data, path) Result~()~
    }

    class Document {
        -document: scraper::Html
        +new(bytes) Result~Document~
        -script_sources() HashSet~String~
        -link_sources() HashSet~String~
        +sources() Vec~String~
    }

    class JsSource {
        -source_text: String
        +new(source_text) JsSource
        +parse() Result~HashSet~String~~
    }

    class JsWalker {
        <<implements oxc::Visit>>
        -chunk_urls: HashSet~String~
        +visit_assignment_expression(it) "webpack .u detection"
        +visit_call_expression(it) "turbopack .push() detection"
    }

    class webpack_module {
        <<DEAD CODE - declared, never called>>
        +extract_chunk_map(source) Option~(ChunkMap, Option~String~)~
        +chunk_filenames(chunk_map, public_path) Vec~String~
    }

    class util {
        <<free functions, stateless>>
        +full_path(path) Result~PathBuf~
        +fetch_url(url) Result~Vec~u8~~
        +write_file(full_path, data) Result~()~
        +ensure_js_ext(src) String
        +strip_quotes(raw) String
        +format_with_prettier(path) Result~()~
        +resolve_source_url(domain, src) Result~(String,String)~
    }

    class cli_bin {
        <<binary entrypoint>>
        +main()
    }

    cli_bin ..> App : "calls batter::run()"
    App "1" *-- "1" Cli : constructed from
    App "1" *-- "1" Site : owns
    Site ..> Document : creates in fetch_html()
    Site ..> JsSource : creates per fetched chunk
    Site ..> util : delegates fetch/write/resolve
    JsSource "1" *-- "1" JsWalker : creates in parse()
    JsWalker ..> util : strip_quotes()
    App ..> webpack_module : mod declared, unused
```

## Notes

Produced independently by a fresh agent reading the repository directly, with no
communication with the agents that produced the sequence or component diagrams.

`webpack.rs` is declared as a module in `app.rs` but its functions
(`extract_chunk_map`, `chunk_filenames`) are never called anywhere in the live
crawl path — its responsibility was reimplemented directly inside `js.rs`'s
`JsWalker`, which additionally covers Turbopack's chunk-registration pattern.
