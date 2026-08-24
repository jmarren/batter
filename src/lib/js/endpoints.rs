use oxc::ast::ast::{
    Argument, CallExpression, Expression, ObjectPropertyKind, PropertyKey, TemplateLiteral,
};

use crate::util;

// http client verb methods that take the url as their first argument, e.g.
// axios.get(url), axios.post(url, body), $.get(url)
const URL_FIRST_ARG_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "request"];

// recognizes a fixed set of http-request call shapes and extracts whatever
// static url content was passed to them (see js.rs plan / doc comment on
// url_expression_to_string for what counts as "static"). loose on the
// receiver (doesn't try to prove the callee is really axios/jquery/etc) for
// the same reason turbopack_chunk_urls is loose on its receiver: minified
// code can rename or restructure the surrounding object, but the call shape
// itself tends to survive.
pub(super) fn extract_endpoint(call: &CallExpression) -> Option<String> {
    match &call.callee {
        // fetch(url, ...)
        Expression::Identifier(id) if id.name == "fetch" => {
            argument_url_string(call.arguments.first()?)
        }

        // <anything>.get/post/put/patch/delete/request(url, ...)
        Expression::StaticMemberExpression(m)
            if URL_FIRST_ARG_METHODS.contains(&m.property.name.as_str()) =>
        {
            argument_url_string(call.arguments.first()?)
        }

        // <anything>.open(method, url, ...) - XMLHttpRequest.open
        Expression::StaticMemberExpression(m) if m.property.name == "open" => {
            argument_url_string(call.arguments.get(1)?)
        }

        // $.ajax({url: "...", ...})
        Expression::StaticMemberExpression(m) if m.property.name == "ajax" => {
            let Argument::ObjectExpression(obj) = call.arguments.first()? else {
                return None;
            };
            url_expression_to_string(find_property_string(obj, "url")?)
        }

        _ => None,
    }
}

// find a named property on an object expression and return its value
// expression (same pattern as find_other_chunks, generalized to any key)
fn find_property_string<'e>(
    obj: &'e oxc::ast::ast::ObjectExpression<'e>,
    key: &str,
) -> Option<&'e Expression<'e>> {
    obj.properties.iter().find_map(|prop| {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return None;
        };

        let key_matches = match &prop.key {
            PropertyKey::StaticIdentifier(id) => id.name == key,
            PropertyKey::StringLiteral(s) => s.value == key,
            _ => false,
        };

        key_matches.then_some(&prop.value)
    })
}

// extracts the static content of a url expression passed to a recognized
// request call. handles string literals directly, and template literals by
// reconstructing their static skeleton with a "{}" placeholder for each
// interpolated expression (e.g. `${base}/users/${id}` -> "{}/users/{}") -
// per the "no dynamic resolution" scope, we don't try to look up what a
// variable used in an interpolation or concatenation actually holds. any
// other expression shape (bare identifier, `+` concatenation, computed
// member access) has no literal content to extract and returns None.
fn url_expression_to_string(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => s.raw.as_deref().map(util::strip_quotes),
        Expression::TemplateLiteral(t) => Some(template_literal_skeleton(t)),
        Expression::ParenthesizedExpression(p) => url_expression_to_string(&p.expression),
        _ => None,
    }
}

// same as url_expression_to_string, but for a call argument (Argument
// flattens the same Expression variants directly rather than wrapping them,
// so this can't just delegate to url_expression_to_string by reference)
fn argument_url_string(arg: &Argument) -> Option<String> {
    match arg {
        Argument::StringLiteral(s) => s.raw.as_deref().map(util::strip_quotes),
        Argument::TemplateLiteral(t) => Some(template_literal_skeleton(t)),
        Argument::ParenthesizedExpression(p) => url_expression_to_string(&p.expression),
        _ => None,
    }
}

// reconstructs a template literal's static skeleton: quasis (literal text
// segments) interleaved with a "{}" placeholder per interpolated expression,
// in source order
fn template_literal_skeleton(t: &TemplateLiteral) -> String {
    let mut out = String::new();

    for (i, quasi) in t.quasis.iter().enumerate() {
        let text = quasi.value.cooked.as_deref().unwrap_or(&quasi.value.raw);
        out.push_str(text);

        if i < t.expressions.len() {
            out.push_str("{}");
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::js::source::JsSource;

    fn endpoints_of(source: &str) -> HashSet<String> {
        let source_url = reqwest::Url::parse("https://example.com/app.js").unwrap();
        JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .endpoints
    }

    #[test]
    fn extracts_endpoint_from_fetch() {
        let endpoints = endpoints_of(r#"fetch("/api/users");"#);
        assert_eq!(endpoints, HashSet::from(["/api/users".to_string()]));
    }

    #[test]
    fn extracts_endpoint_from_axios_verb_calls() {
        let endpoints = endpoints_of(
            r#"
            axios.get("/api/users");
            axios.post("/api/users", { name: "bob" });
            "#,
        );
        assert_eq!(endpoints, HashSet::from(["/api/users".to_string()]));
    }

    #[test]
    fn extracts_endpoint_from_xhr_open() {
        let endpoints = endpoints_of(r#"xhr.open("GET", "/api/users");"#);
        assert_eq!(endpoints, HashSet::from(["/api/users".to_string()]));
    }

    #[test]
    fn extracts_endpoint_from_jquery_ajax() {
        let endpoints = endpoints_of(r#"$.ajax({ url: "/api/users", method: "GET" });"#);
        assert_eq!(endpoints, HashSet::from(["/api/users".to_string()]));
    }

    #[test]
    fn extracts_endpoint_from_jquery_get() {
        let endpoints = endpoints_of(r#"$.get("/api/users", function (data) {});"#);
        assert_eq!(endpoints, HashSet::from(["/api/users".to_string()]));
    }

    #[test]
    fn extracts_template_literal_endpoint_as_static_skeleton() {
        let endpoints = endpoints_of(r#"fetch(`${base}/users/${id}`);"#);
        assert_eq!(endpoints, HashSet::from(["{}/users/{}".to_string()]));
    }

    #[test]
    fn skips_endpoint_with_no_static_content() {
        let endpoints = endpoints_of(r#"fetch(url);"#);
        assert!(endpoints.is_empty());
    }

    #[test]
    fn skips_string_concatenation_without_extracting_a_bogus_endpoint() {
        let endpoints = endpoints_of(r#"fetch(base + "/users/" + id);"#);
        assert!(endpoints.is_empty());
    }
}
