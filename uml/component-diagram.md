# Component Diagram

```mermaid
flowchart TD
    subgraph BIN["binary crate"]
        cli["bin/cli.rs<br/>main() entrypoint"]
    end

    subgraph LIB["batter lib crate (src/lib/app.rs = crate root)"]
        app["app.rs<br/>Cli (clap), App<br/>composition root / mod tree owner"]
        site["site.rs<br/>Site<br/>crawl orchestration, source dedup, file writing"]
        html["html.rs<br/>Document<br/>HTML parsing, script/link src extraction"]
        js["js.rs<br/>JsSource, JsWalker<br/>JS AST walk, webpack+turbopack chunk URL discovery"]
        util["util.rs<br/>path/fetch/write/url helpers"]
        webpack["webpack.rs<br/>extract_chunk_map, chunk_filenames<br/>(DEAD CODE - orphaned)"]
    end

    subgraph EXT["external crates"]
        clap["clap (derive)"]
        anyhow["anyhow"]
        dotenvy["dotenvy"]
        tracing["tracing"]
        tracingsub["tracing-subscriber"]
        tokio["tokio (full)"]
        scraper["scraper"]
        reqwest["reqwest"]
        oxc["oxc (parser, ast, ast_visit, allocator, span)"]
        serde["serde / serde_json"]
    end

    cli -->|"calls batter::run()"| app

    app -->|"declares mod; constructs Site"| site
    app --> clap
    app --> anyhow
    app --> dotenvy
    app --> tracing
    app --> tracingsub
    app --> tokio

    site -->|"html::Document::new()"| html
    site -->|"js::JsSource::new().parse()"| js
    site -->|"util::fetch_url, write_file, resolve_source_url, ensure_js_ext, full_path"| util
    site --> anyhow
    site --> tokio

    html --> scraper
    html --> anyhow

    js --> oxc
    js -->|"util::strip_quotes"| util
    js --> anyhow

    util --> reqwest
    util --> tokio
    util --> anyhow

    webpack -.->|"unused: not referenced by site/js/html/util/cli"| oxc

    serde -.->|"declared in Cargo.toml, no use found in src/"| LIB

    style webpack fill:#4a1a1a,stroke:#ff4444,stroke-width:2px,color:#ffdddd
```

## Notes

Produced independently by a fresh agent reading the repository directly, with no
communication with the agents that produced the class or sequence diagrams.

Two dead/unused points identified:
- `webpack.rs` is declared as a module in `app.rs` but its public functions
  (`extract_chunk_map`, `chunk_filenames`) are never called anywhere — its
  chunk-map-extraction responsibility was reimplemented and extended (to also
  cover Turbopack) directly inside `js.rs`'s `JsWalker`.
- `serde`/`serde_json` are declared as dependencies in `Cargo.toml` but no
  `use serde` or `#[derive(Serialize/Deserialize)]` appears anywhere in `src/`.
