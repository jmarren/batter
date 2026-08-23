# Sequence Diagram

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant Bin as bin/cli.rs::main
    participant Lib as lib::run
    participant App
    participant Site
    participant Util as util (free fns)
    participant Doc as html::Document
    participant JoinSet as tokio::JoinSet
    participant Js as JsSource
    participant Thread as OS thread (prettier)

    User->>Bin: batter <domain> [out_dir] [--allow-external-sources]
    Bin->>Lib: run()
    Lib->>Lib: dotenvy::dotenv(), tracing_subscriber::init()
    Lib->>Lib: Cli::parse()
    Lib->>App: App::from_cli(cli)
    App->>Util: full_path(out_dir)
    Util-->>App: canonicalized PathBuf
    App->>Site: Site::new(out_dir, domain, allow_external_sources)
    App-->>Lib: App { domain, site }
    Lib->>App: app.run().await

    App->>Site: enumerate().await

    Site->>Site: fetch_html()
    Site->>Util: fetch_url(domain)
    Util->>Util: reqwest::get(https://domain)
    Util-->>Site: Result<Vec<u8>> (html bytes)
    Site->>Site: write_index(bytes) -> write_file(..., "index.html")
    Site->>Util: util::write_file(full_path, data)
    Util-->>Site: Ok(())
    Site->>Doc: Document::new(bytes)
    Doc->>Doc: scraper::Html::parse_document
    Doc-->>Site: Document

    Site->>Doc: sources()
    Doc->>Doc: script_sources() + link_sources()
    Doc-->>Site: Vec<String> source urls

    Site->>Site: handle_sources(&sources).await

    loop for each initial source
        Site->>Site: resolve_source(src)
        Site->>Util: resolve_source_url(domain, src)
        alt resolve fails
            Util-->>Site: Err
            Site->>Site: tracing::error! (log, return None)
        else cross-origin & not allowed
            Site->>Site: all_sources.push(full_url); tracing::info!("discarding cross-origin"); return None
        else ok
            Site->>Site: all_sources.push(full_url); return Some(full_url)
        end
        Site->>JoinSet: spawn(util::fetch_url(src))
    end

    loop while join_set.join_next() returns Some (drains until empty)
        JoinSet-->>Site: task_result: (Result<Vec<u8>>, src)
        Site->>Site: handle_source_response(task_result).await

        alt fetch task panicked or fetch_url errored
            Site->>Site: tracing::error!("failed to handle source: ...")
            Note over Site: continue loop (error swallowed, crawl not aborted)
        else fetch succeeded
            Site->>Util: ensure_js_ext(src)
            Site->>Site: write_file(bytes, file_path)
            Site->>Util: util::write_file(full_path, bytes)
            Site->>Thread: std::thread::spawn(format_with_prettier(full_path))
            Thread->>Thread: npx prettier --write <path>
            Site->>Site: prettier_handles.push(handle) (detached from tokio; joined later)

            Site->>Js: JsSource::new(source_text)
            Site->>Js: parse()
            Js->>Js: oxc Parser::parse(source_text)
            alt parser panicked
                Js-->>Site: Err("parser panicked")
                Site->>Site: tracing::error!("failed to handle source: ...")
                Note over Site: continue loop
            else parsed ok
                Js->>Js: JsWalker.visit_program()
                Note right of Js: visit_assignment_expression looks for<br/>`X.u = e => ...` (webpack chunk map);<br/>visit_call_expression looks for<br/>`(...).push([el,{otherChunks:[...]}])` (turbopack)
                Js-->>Site: HashSet<chunk_url>
            end

            loop for each newly discovered chunk_url
                Site->>Site: resolve_source("_next/" + chunk_url)
                alt already seen or cross-origin/unresolvable
                    Site->>Site: skip (continue)
                else new, resolvable
                    Site->>JoinSet: spawn(util::fetch_url(resolved_src))
                    Note over JoinSet: feeds back into the same<br/>join_next() loop above
                end
            end
        end
    end

    Note over Site: JoinSet empty, no more new URLs -> handle_sources returns

    Site->>Site: join_prettier_handles()
    loop for each prettier JoinHandle
        Site->>Thread: handle.join()
        Thread-->>Site: (blocks until OS thread finishes)
    end

    Site->>Site: write_all_sources().await
    Site->>Site: format all_sources entries as "https://{src}\n"
    Site->>Site: write_file(contents, "js-sources.txt")
    Site->>Util: util::write_file(full_path, data)
    Util-->>Site: Ok(())

    Site-->>App: Ok(())
    App-->>Lib: Ok(())
    Lib->>Lib: .unwrap()
    Lib-->>User: process exits
```

## Notes

Produced independently by a fresh agent reading the repository directly, with no
communication with the agents that produced the class or component diagrams.

`webpack.rs` was confirmed unused/dead code (declared as a module but never
called in the live crawl path) and was omitted from this diagram since it does
not execute at runtime.
