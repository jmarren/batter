use oxc::ast::ast::{ArrayExpressionElement, BindingPattern, Expression, VariableDeclarator};

// vite emits a `__vite__mapDeps` helper wherever dynamic imports need their
// transitive chunk/stylesheet dependencies preloaded, shaped roughly like:
//
// const __vite__mapDeps = (
//   i,
//   m = __vite__mapDeps,
//   d = m.f || (m.f = ["assets/foo-hash.js", "assets/foo-hash.css", ...]),
// ) => i.map((i) => d[i]);
//
// the dependency list lives as the default value of one of the arrow
// function's own parameters (`d` above), memoized onto the function itself
// (`m.f`) so it's only built once. matched loosely by scanning every
// parameter's default-value expression for a nested array of string
// literals, rather than requiring the exact `||`/assignment/parenthesization
// shape, since that memoization idiom could vary - but gated on the
// declared variable's name starting with "__vite__" (vite's own convention,
// and not something a minifier would rename away) so this doesn't match
// unrelated arrow functions that happen to have an array-valued default
// parameter. stylesheet paths (.css) are excluded - only js chunk urls are
// wanted here, same as buildmanifest.rs.
pub(super) fn vite_chunk_urls(declarator: &VariableDeclarator) -> Vec<String> {
    let BindingPattern::BindingIdentifier(id) = &declarator.id else {
        return vec![];
    };

    if !id.name.starts_with("__vite__") {
        return vec![];
    }

    let Some(Expression::ArrowFunctionExpression(arrow_fn)) = &declarator.init else {
        return vec![];
    };

    let mut urls = vec![];

    for param in &arrow_fn.params.items {
        if let Some(initializer) = &param.initializer {
            find_string_arrays(initializer, &mut urls);
        }
    }

    urls.retain(|url: &String| !url.ends_with(".css"));
    urls
}

// recursively walks an expression looking for any array-of-string-literals,
// collecting every string literal found. doesn't need to walk into every
// expression variant - just enough to see through the `||` / assignment /
// parenthesization wrapping this shows up in.
fn find_string_arrays(expr: &Expression, out: &mut Vec<String>) {
    match expr {
        Expression::ArrayExpression(arr) => {
            out.extend(arr.elements.iter().filter_map(|el| match el {
                ArrayExpressionElement::StringLiteral(s) => Some(s.value.to_string()),
                _ => None,
            }));
        }
        Expression::LogicalExpression(logical) => {
            find_string_arrays(&logical.left, out);
            find_string_arrays(&logical.right, out);
        }
        Expression::AssignmentExpression(assignment) => {
            find_string_arrays(&assignment.right, out);
        }
        Expression::ParenthesizedExpression(paren) => {
            find_string_arrays(&paren.expression, out);
        }
        _ => (),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::js::source::JsSource;

    #[test]
    fn extracts_chunk_urls_from_vite_map_deps() {
        let source = r#"
            const __vite__mapDeps = (
              i,
              m = __vite__mapDeps,
              d = m.f ||
                (m.f = [
                  "assets/_authenticated-DyEAWQTU.js",
                  "assets/react-vendor-bErXCNjO.js",
                  "assets/quill-editor-CTpA5PSA.css",
                ]),
            ) => i.map((i) => d[i]);
        "#;

        let source_url = reqwest::Url::parse("https://example.com/assets/index-abc123.js").unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        assert_eq!(
            urls,
            HashSet::from([
                "https://example.com/assets/_authenticated-DyEAWQTU.js".to_string(),
                "https://example.com/assets/react-vendor-bErXCNjO.js".to_string(),
            ])
        );
    }

    #[test]
    fn excludes_stylesheet_deps() {
        let source = r#"
            const __vite__mapDeps = (i, m = __vite__mapDeps, d = m.f || (m.f = [
              "assets/only-Bhash.css",
            ])) => i.map((i) => d[i]);
        "#;

        let source_url = reqwest::Url::parse("https://example.com/assets/index-abc123.js").unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        assert!(urls.is_empty());
    }

    #[test]
    fn ignores_arrow_functions_not_named_like_vites_helper() {
        let source = r#"
            const notMapDeps = (a, b = ["assets/unrelated-hash.js"]) => a;
        "#;

        let source_url = reqwest::Url::parse("https://example.com/assets/index-abc123.js").unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        // gated on the "__vite__" name prefix, so an unrelated arrow function
        // with a similarly-shaped default parameter isn't mistaken for it
        assert!(urls.is_empty());
    }

    #[test]
    fn extracts_from_a_realistic_large_deps_map() {
        let source = r#"
            const __vite__mapDeps = (
              i,
              m = __vite__mapDeps,
              d = m.f ||
                (m.f = [
                  "assets/_authenticated-DyEAWQTU.js",
                  "assets/react-vendor-bErXCNjO.js",
                  "assets/quill-editor-BXpEJkHl.js",
                  "assets/utils-vendor-Db9XJ9nq.js",
                  "assets/quill-editor-CTpA5PSA.css",
                  "assets/markdown-vendor-DSL5j5k9.js",
                  "assets/tanstack-vendor-BHBvUj2O.js",
                  "assets/ui-common-other-DjIXBuLe.js",
                  "assets/Sidebar-CPzXqfrv.js",
                  "assets/ui-common-theme-B6tDIY9f.js",
                ]),
            ) => i.map((i) => d[i]);
        "#;

        let source_url = reqwest::Url::parse("https://example.com/assets/index-abc123.js").unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        assert_eq!(
            urls,
            HashSet::from([
                "https://example.com/assets/_authenticated-DyEAWQTU.js".to_string(),
                "https://example.com/assets/react-vendor-bErXCNjO.js".to_string(),
                "https://example.com/assets/quill-editor-BXpEJkHl.js".to_string(),
                "https://example.com/assets/utils-vendor-Db9XJ9nq.js".to_string(),
                "https://example.com/assets/markdown-vendor-DSL5j5k9.js".to_string(),
                "https://example.com/assets/tanstack-vendor-BHBvUj2O.js".to_string(),
                "https://example.com/assets/ui-common-other-DjIXBuLe.js".to_string(),
                "https://example.com/assets/Sidebar-CPzXqfrv.js".to_string(),
                "https://example.com/assets/ui-common-theme-B6tDIY9f.js".to_string(),
            ])
        );
    }
}
