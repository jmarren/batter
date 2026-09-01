use oxc::ast::ast::{
    Argument, ArrayExpressionElement, AssignmentTarget, CallExpression, Expression,
    ObjectPropertyKind, PropertyKey, Statement,
};

// next.js emits a `_buildManifest.js` file (loaded as a plain <script src>,
// so it flows through the same JsSource::parse as everything else) shaped
// roughly like:
//
// self.__BUILD_MANIFEST = function(s, a, e, ...) {
//   return {
//     __rewrites: {...},
//     "/": ["static/chunks/pages/index-hash.js"],
//     "/admin": [e, "static/chunks/48441-hash.js", "static/chunks/pages/admin-hash.js"],
//     sortedPages: ["/", "/admin", ...]
//   }
// }("static/chunks/5457-hash.js", "static/chunks/26737-hash.js", ..., "static/css/hash.css", ...)
// (the css entries above are excluded - see shared_chunk_urls)
//
// each per-route array mixes bare identifiers (references into the IIFE's
// hoisted args, not resolved here - same "no dynamic resolution" scope as
// endpoints.rs) with string literals that are chunk paths, so we only need to
// pull out the string literals. `sortedPages` and the `__rewrites`/
// `__routerFilter*` metadata entries have non-array or non-chunk-path values
// and are skipped naturally since we only look at properties whose value is
// an array.
//
// the trailing call arguments (what the IIFE is actually invoked with) are
// the hoisted string constants those bare identifiers refer to - shared
// chunks not tied to any single page (framework/vendor bundles) live only
// here, never under a route's own array, so they're pulled out the same way.
// this list also includes stylesheet paths (.css), which are filtered out -
// we only want js chunk urls here.
pub(super) fn build_manifest_chunk_urls(
    target: &AssignmentTarget,
    call: &CallExpression,
) -> Vec<String> {
    let AssignmentTarget::StaticMemberExpression(s) = target else {
        return vec![];
    };

    if s.property.name != "__BUILD_MANIFEST" {
        return vec![];
    }

    let Expression::FunctionExpression(func) = &call.callee else {
        return vec![];
    };

    let mut urls = shared_chunk_urls(call);
    urls.extend(page_chunk_urls(func));
    urls
}

// the shared chunks passed as the IIFE's call arguments (stylesheet paths
// are also passed here, but excluded - only js chunk urls are wanted)
fn shared_chunk_urls(call: &CallExpression) -> Vec<String> {
    call.arguments
        .iter()
        .filter_map(|arg| match arg {
            Argument::StringLiteral(s) => Some(s.value.to_string()),
            _ => None,
        })
        .filter(|s| !s.ends_with(".css"))
        .collect()
}

// the per-route chunk paths from the returned object's route -> chunk-path arrays
fn page_chunk_urls(func: &oxc::ast::ast::Function) -> Vec<String> {
    let Some(body) = &func.body else {
        return vec![];
    };

    let returned = body.statements.iter().find_map(|stmt| match stmt {
        Statement::ReturnStatement(ret) => ret.argument.as_ref(),
        _ => None,
    });

    let Some(Expression::ObjectExpression(obj)) = returned else {
        return vec![];
    };

    obj.properties
        .iter()
        .filter_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return None;
            };

            // `sortedPages` holds route paths, not chunk paths - every other
            // array-valued property here is a route -> chunk-path list
            let is_sorted_pages = match &prop.key {
                PropertyKey::StaticIdentifier(id) => id.name == "sortedPages",
                PropertyKey::StringLiteral(s) => s.value == "sortedPages",
                _ => false,
            };

            if is_sorted_pages {
                return None;
            }

            match &prop.value {
                Expression::ArrayExpression(arr) => Some(arr),
                _ => None,
            }
        })
        .flat_map(|arr| {
            arr.elements.iter().filter_map(|el| match el {
                ArrayExpressionElement::StringLiteral(s) => Some(s.value.to_string()),
                _ => None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::js::source::JsSource;

    #[test]
    fn extracts_page_chunk_urls_from_build_manifest() {
        let source = r#"
            self.__BUILD_MANIFEST = function(s, a, e) {
                return {
                    __rewrites: { afterFiles: [], beforeFiles: [], fallback: [] },
                    "/": ["static/chunks/pages/index-0c908c63a8e98414.js"],
                    "/404": ["static/chunks/pages/404-5c22517a734ab15e.js"],
                    "/admin": [e, "static/chunks/48441-68c303ec46c18c38.js", "static/chunks/pages/admin-ac84cafb2ea7cc1a.js"],
                    sortedPages: ["/", "/404", "/admin"]
                }
            }("static/chunks/5457-5674eca4f7b952ff.js", "static/chunks/26737-da0e45b9cbe2aeac.js");
            self.__BUILD_MANIFEST_CB && self.__BUILD_MANIFEST_CB();
        "#;

        let source_url =
            reqwest::Url::parse("https://example.com/_next/static/xyz/_buildManifest.js").unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        assert_eq!(
            urls,
            HashSet::from([
                "https://example.com/_next/static/chunks/pages/index-0c908c63a8e98414.js"
                    .to_string(),
                "https://example.com/_next/static/chunks/pages/404-5c22517a734ab15e.js".to_string(),
                "https://example.com/_next/static/chunks/48441-68c303ec46c18c38.js".to_string(),
                "https://example.com/_next/static/chunks/pages/admin-ac84cafb2ea7cc1a.js"
                    .to_string(),
                "https://example.com/_next/static/chunks/5457-5674eca4f7b952ff.js".to_string(),
                "https://example.com/_next/static/chunks/26737-da0e45b9cbe2aeac.js".to_string(),
            ])
        );
    }

    #[test]
    fn extracts_shared_chunks_but_excludes_stylesheets_from_iife_arguments() {
        // the trailing call arguments include shared/vendor chunks that
        // never appear under any single route's own array - only referenced
        // indirectly via the bare identifiers (s, a, ...) - alongside
        // stylesheet paths, which should be excluded
        let source = r#"
            self.__BUILD_MANIFEST = function(s, a) {
                return {
                    "/": [s, a, "static/chunks/pages/index-hash.js"],
                    sortedPages: ["/"]
                }
            }("static/chunks/12345-vendorhash.js", "static/css/abcdef.css");
        "#;

        let source_url =
            reqwest::Url::parse("https://example.com/_next/static/xyz/_buildManifest.js").unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        assert_eq!(
            urls,
            HashSet::from([
                "https://example.com/_next/static/chunks/pages/index-hash.js".to_string(),
                "https://example.com/_next/static/chunks/12345-vendorhash.js".to_string(),
            ])
        );
    }

    #[test]
    fn ignores_sorted_pages_and_rewrites() {
        let source = r#"
            self.__BUILD_MANIFEST = function(s) {
                return {
                    __rewrites: { afterFiles: [], beforeFiles: [], fallback: [] },
                    "/": ["static/chunks/pages/index-hash.js"],
                    sortedPages: ["/"]
                }
            }();
        "#;

        let source_url = reqwest::Url::parse("https://example.com/_buildManifest.js").unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        assert_eq!(
            urls,
            HashSet::from(["https://example.com/static/chunks/pages/index-hash.js".to_string()])
        );
    }
}
