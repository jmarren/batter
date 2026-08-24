use oxc::ast::ast::{
    Argument, ArrayExpressionElement, CallExpression, Expression, ObjectPropertyKind, PropertyKey,
};

use crate::util;

// extracts chunk urls from a turbopack chunk-registration call:
// `<anything>.push([scriptEl, {otherChunks: ["url", ...], ...}])`.
//
// matched loosely on the `otherChunks` array shape alone, rather than requiring
// the callee to reference a `TURBOPACK` global - property names like
// `otherChunks` survive minification (renaming them would break other code that
// reads the property by name), while the exact guard expression around the
// global (`globalThis` vs `self`, differing `||` fallbacks) can vary.
pub(super) fn turbopack_chunk_urls(call: &CallExpression) -> Vec<String> {
    let Expression::StaticMemberExpression(callee) = &call.callee else {
        return vec![];
    };

    if callee.property.name != "push" {
        return vec![];
    }

    let Some(Argument::ArrayExpression(arr)) = call.arguments.first() else {
        return vec![];
    };

    let other_chunks = arr.elements.iter().find_map(|el| match el {
        ArrayExpressionElement::ObjectExpression(o) => find_other_chunks(o),
        _ => None,
    });

    let Some(other_chunks) = other_chunks else {
        return vec![];
    };

    other_chunks
        .elements
        .iter()
        .filter_map(|el| match el {
            ArrayExpressionElement::StringLiteral(s) => s.raw.as_deref().map(util::strip_quotes),
            _ => None,
        })
        .collect()
}

// find an `otherChunks: [...]` property on an object expression and return its
// array value
fn find_other_chunks<'e>(
    obj: &'e oxc::ast::ast::ObjectExpression<'e>,
) -> Option<&'e oxc::ast::ast::ArrayExpression<'e>> {
    obj.properties.iter().find_map(|prop| {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return None;
        };

        let key_matches = match &prop.key {
            PropertyKey::StaticIdentifier(id) => id.name == "otherChunks",
            PropertyKey::StringLiteral(s) => s.value == "otherChunks",
            _ => false,
        };

        if !key_matches {
            return None;
        }

        match &prop.value {
            Expression::ArrayExpression(arr) => Some(arr.as_ref()),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::js::source::JsSource;

    #[test]
    fn extracts_urls_from_turbopack_push() {
        let source = r#"
            (globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([
              "object" == typeof document ? document.currentScript : void 0,
              {
                otherChunks: [
                  "static/immutable/chunks/236-04af9ww4u.js",
                  "static/immutable/chunks/0ifbgewhks2yb.js",
                  "static/immutable/chunks/2ko36sx9hycil.js",
                  "static/immutable/chunks/2tt92x9jwpwmi.js",
                ],
                runtimeModuleIds: [554156],
              },
            ]);
        "#;

        // no path segment shared with the chunk urls below, so resolution
        // falls back to a plain join against this source's url
        let source_url = reqwest::Url::parse("https://example.com/entry.js").unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        assert_eq!(
            urls,
            HashSet::from([
                "https://example.com/static/immutable/chunks/236-04af9ww4u.js".to_string(),
                "https://example.com/static/immutable/chunks/0ifbgewhks2yb.js".to_string(),
                "https://example.com/static/immutable/chunks/2ko36sx9hycil.js".to_string(),
                "https://example.com/static/immutable/chunks/2tt92x9jwpwmi.js".to_string(),
            ])
        );
    }
}
