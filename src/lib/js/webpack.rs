use oxc::{
    allocator,
    ast::ast::{
        ArrowFunctionExpression, ComputedMemberExpression, ConditionalExpression, Expression,
        ObjectPropertyKind, PropertyKey, Statement,
    },
};

use crate::util;

// webpack's chunk-loader assigns `__webpack_require__.u`, e.g.
// `c.u = (e) => "static/chunks/" + (6467 === e ? "c07e5374" : e) + "." + {558:
// "097941ffb76484a3", ...}[e] + ".js"`
// which we can statically evaluate for every entry in the id -> hash map to
// recover every chunk url this bundle could ever load.
pub(super) fn chunk_urls<'a>(
    arrow_fn: &allocator::Box<'a, ArrowFunctionExpression<'a>>,
) -> Vec<String> {
    let Some(body) = arrow_body_expression(&arrow_fn) else {
        return vec![];
    };

    // the body may itself be wrapped in a top-level ternary that hardcodes a full
    // literal path for one specific chunk id, e.g.
    // `6918 === e ? "static/chunks/6918-hash.js" : <the usual +-chain>`
    let mut literal_urls = vec![];
    let body = unwrap_literal_chunk_override(body, &mut literal_urls);

    let mut flattened = vec![];
    flatten_binary_plus_chain(body, &mut flattened);

    let mut prefix: String = "".into();
    let mut override_id: Option<String> = None;
    let mut consequent: String = "".into();
    let mut props: Vec<(String, String)> = vec![];

    flattened.iter().for_each(|e| match e {
        Expression::StringLiteral(s) => match s.raw.as_deref().map(util::strip_quotes) {
            Some(raw) if raw != "." && raw != ".js" => {
                prefix = raw;
            }
            _ => (),
        },
        Expression::ConditionalExpression(c) => {
            (override_id, consequent) = conditional_override(c);
        }
        Expression::ParenthesizedExpression(p) => match &p.expression {
            Expression::ConditionalExpression(c) => {
                (override_id, consequent) = conditional_override(c);
            }
            _ => (),
        },
        Expression::ComputedMemberExpression(m) => {
            props = object_props(m);
        }
        _ => (),
    });

    let mut urls: Vec<String> = props
        .iter()
        .map(|(id, hash)| {
            let id_part = if override_id.as_deref() == Some(id.as_str()) {
                &consequent
            } else {
                id
            };

            format!("{prefix}{id_part}.{hash}.js")
        })
        .collect();

    urls.extend(literal_urls);
    urls
}

// peels off any number of leading `<id> === e ? "<full literal path>" : <rest>`
// ternaries, collecting each hardcoded path into `out`, and returns the remaining
// expression (the actual `+`-chain) to keep evaluating
fn unwrap_literal_chunk_override<'e>(
    expr: &'e Expression,
    out: &mut Vec<String>,
) -> &'e Expression<'e> {
    let unwrapped = match expr {
        Expression::ParenthesizedExpression(p) => &p.expression,
        other => other,
    };

    let Expression::ConditionalExpression(cond) = unwrapped else {
        return unwrapped;
    };

    // a literal-path override's consequent is a bare string, unlike the
    // id/hash-fragment overrides handled by `conditional_override`
    let Expression::StringLiteral(s) = &cond.consequent else {
        return unwrapped;
    };

    if let Some(raw) = s.raw.as_deref() {
        out.push(util::strip_quotes(raw));
    }

    unwrap_literal_chunk_override(&cond.alternate, out)
}

// `<override_id> === e ? <consequent> : e` -> the chunk id that gets a special-cased
// filename fragment, and what that fragment is
fn conditional_override(c: &ConditionalExpression) -> (Option<String>, String) {
    let Expression::BinaryExpression(bin) = &c.test else {
        return (None, "".into());
    };

    let override_id = [&bin.left, &bin.right].iter().find_map(|e| match e {
        Expression::NumericLiteral(n) => n.raw.map(|r| r.to_string()),
        _ => None,
    });

    let consequent = match &c.consequent {
        Expression::StringLiteral(s) => {
            s.raw.as_deref().map(util::strip_quotes).unwrap_or_default()
        }
        _ => "".into(),
    };

    (override_id, consequent)
}

fn object_props(computed: &ComputedMemberExpression) -> Vec<(String, String)> {
    let mut out = vec![];

    let object = match &computed.object {
        Expression::ParenthesizedExpression(p) => &p.expression,
        other => other,
    };

    if let Expression::ObjectExpression(o) = object {
        o.properties.iter().for_each(|prop| match &prop {
            ObjectPropertyKind::ObjectProperty(object_prop) => {
                match (&object_prop.value, &object_prop.key) {
                    (Expression::StringLiteral(val), PropertyKey::NumericLiteral(k)) => {
                        if let Some(v) = val.raw {
                            out.push((k.value.round().to_string(), util::strip_quotes(&v)));
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        });
    }

    out
}

// /// Flatten webpack's left-associative `+` chain (`a + b + c + ...`) into operands
// /// in source order.
fn flatten_binary_plus_chain<'e>(expr: &'e Expression, out: &mut Vec<&'e Expression<'e>>) {
    if let Expression::BinaryExpression(bin) = expr
        && bin.operator == oxc::ast::ast::BinaryOperator::Addition
    {
        flatten_binary_plus_chain(&bin.left, out);
        flatten_binary_plus_chain(&bin.right, out);
        return;
    }

    out.push(expr);
}

fn arrow_body_expression<'a, 'b>(
    arrow_fn: &'b ArrowFunctionExpression<'a>,
) -> Option<&'b Expression<'a>> {
    // if expression `(e) => something`, return expression
    if let Some(expr) = arrow_fn.get_expression() {
        return Some(expr);
    }

    // if function body
    // `(e) => {
    //      return something
    //  }`,
    // return the returned value?
    arrow_fn
        .get_function_body()?
        .statements
        .iter()
        .find_map(|stmt| match stmt {
            Statement::ReturnStatement(ret) => ret.argument.as_ref(),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::js::source::JsSource;

    #[test]
    fn extracts_urls_from_simple_hash_map() {
        let source = r#"
            (c.u = (e) =>
              "static/chunks/" +
              (6467 === e ? "c07e5374" : e) +
              "." +
              {
                558: "097941ffb76484a3",
                4327: "115b4e4cf5dfd289",
                6467: "744537bcd0ee57b6",
                6497: "746d29151a1f862f",
                6684: "dcd4d65d61c9e94e",
              }[e] +
              ".js");
        "#;

        let source_url =
            reqwest::Url::parse("https://example.com/_next/static/chunks/webpack-abc.js").unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        assert_eq!(
            urls,
            HashSet::from([
                "https://example.com/_next/static/chunks/558.097941ffb76484a3.js".to_string(),
                "https://example.com/_next/static/chunks/4327.115b4e4cf5dfd289.js".to_string(),
                "https://example.com/_next/static/chunks/c07e5374.744537bcd0ee57b6.js".to_string(),
                "https://example.com/_next/static/chunks/6497.746d29151a1f862f.js".to_string(),
                "https://example.com/_next/static/chunks/6684.dcd4d65d61c9e94e.js".to_string(),
            ])
        );
    }

    #[test]
    fn extracts_urls_with_a_hardcoded_literal_override() {
        let source = r#"
            (c.u = (e) =>
              6918 === e
                ? "static/chunks/6918-9e9acda1c00665ed.js"
                : "static/chunks/" +
                  (3829 === e ? "a2997d9c" : e) +
                  "." +
                  {
                    2: "c1a2846f3c3641f7",
                    256: "b8179e6d733a9e5e",
                    309: "e6f4d65b96c26a32",
                    318: "f34ffe3d81016fb3",
                    342: "66246f81469c8aca",
                    360: "6f45a3ac17ff5e66",
                    678: "6af955a4ea697b5b",
                    824: "bcf588fdea08a359",
                    832: "258656724072c5ed",
                    880: "5b6719800dc6511e",
                    953: "5bd95ddfa2b55338",
                    1213: "d15dfef62684bec9",
                    1233: "3427744d2ea3d545",
                    1296: "707a578b14c18d1a",
                    1774: "7707355e1f040a9f",
                    1835: "577a8fa2357ca1ff",
                    1903: "e4b8b49c2936d85b",
                    2001: "7c4282eb189b75e5",
                    2088: "4eba9a894feeab5f",
                    2142: "82391b86c80968b5",
                    2270: "8d3afb4a2465a759",
                    2882: "cc73bfe154e3c9c1",
                    2983: "8f95794a3b23873e",
                    3041: "9c6cd85e01e4a83c",
                    3143: "5c573db873437a36",
                    3222: "fe2729b4f29352c8",
                    3375: "70784a01e3be1276",
                    3777: "32b0e4e8b835d119",
                    3829: "47f6e0543e8417ab",
                    3830: "96c33a8268218c23",
                    4005: "422eb8bad2673319",
                    4057: "919f5a31515d8726",
                    4171: "0046349166b7b2c0",
                    4313: "af7a089a40447df3",
                    4396: "b6c701f03dcf293b",
                    4410: "72aeafab866d2154",
                    4461: "91562e9d96050558",
                    4545: "f4d7181424793dfd",
                    4654: "5abb94ed3905a6f7",
                    4679: "a5b1f969a636dd50",
                    4997: "9a72e442e42d5f95",
                    5029: "8bb7b689c4c61964",
                    5186: "a5a0026379545ed5",
                    5206: "5685d0c889e7f569",
                    5345: "39c5535430242b5e",
                    5431: "3ff7121a6271c81a",
                    5488: "e5ec26c27f1a08c0",
                    5711: "567f768380d0ede8",
                    5758: "10930938d115ce0d",
                    5871: "2343d3e324530ab7",
                    6361: "b572c7fcb5346749",
                    6499: "1ec5cd5d2b222da7",
                    6611: "16afcb8e26ec8f09",
                    6637: "10c7478261f07139",
                    6746: "52d38fc80d8b573c",
                    6915: "55b7936d49f8dd05",
                    6954: "9c91a19272024c62",
                    7132: "e15c78aa0ebbf7fd",
                    7214: "ee04f69226f595e7",
                    7247: "4e09b9e06840ab8a",
                    7398: "1705ae6a08f4be28",
                    7416: "2df3853ff779c6fa",
                    7558: "670eb496e293b783",
                    7566: "6101c2fd8431a6cc",
                    7716: "559516986d5a7220",
                    7972: "f542e874beed4c0d",
                    8071: "f42fb058c26b0ce4",
                    8225: "46c6d444ccc3351e",
                    8503: "312e81a60700fa31",
                    8593: "e33a94e4b18eadc5",
                    8625: "7bd8c1a1e5bd2858",
                    8680: "5a08bf557bea3f86",
                    8783: "b20272153d35e0cd",
                    8802: "5a77c6b2ea5891c3",
                    8980: "28e01af73b20e3b1",
                    9023: "4b9b1ab789c46dc8",
                    9135: "d2492f8e0defdfb5",
                    9301: "a161aa909ea37336",
                    9360: "eb11f4eb6eb82e47",
                    9461: "6d8d33e40a81f1d8",
                    9471: "ec5e74041bd491c5",
                    9489: "ab13c35585f7a826",
                    9507: "611586227ec7a47a",
                    9580: "c7948d62fcdb4cda",
                    9874: "861437dd294ccc6a",
                    9929: "48747c558c93f645",
                    9991: "98fe12aeeb749fbc",
                  }[e] +
                  ".js");
        "#;

        let source_url =
            reqwest::Url::parse("https://example.com/_next/static/chunks/webpack-abc.js").unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        // the hardcoded literal-path override for chunk 6918
        assert!(urls.contains("https://example.com/_next/static/chunks/6918-9e9acda1c00665ed.js"));

        // an ordinary entry straight from the id -> hash map
        assert!(urls.contains("https://example.com/_next/static/chunks/2.c1a2846f3c3641f7.js"));

        // the id whose filename fragment gets substituted (3829 -> "a2997d9c")
        // while its hash still comes from the map
        assert!(
            urls.contains("https://example.com/_next/static/chunks/a2997d9c.47f6e0543e8417ab.js")
        );
        assert!(!urls.contains("https://example.com/_next/static/chunks/3829.47f6e0543e8417ab.js"));

        // 87 entries in the map, plus the 6918 literal override url (which
        // never appears in the map itself)
        assert_eq!(urls.len(), 87 + 1);
    }

    #[test]
    fn reroots_chunk_urls_under_detected_mount() {
        let source = r#"
            (c.u = (e) =>
              "static/chunks/" +
              e +
              "." +
              {
                558: "097941ffb76484a3",
              }[e] +
              ".js");
        "#;

        // source served from under a mount other than the site root - the
        // resolved chunk url should land under that same mount, not the root
        let source_url =
            reqwest::Url::parse("https://example.com/_app/immutable/static/chunks/entry.js")
                .unwrap();

        let urls = JsSource::new(source.to_string(), source_url)
            .parse()
            .unwrap()
            .chunk_urls;

        assert_eq!(
            urls,
            HashSet::from([
                "https://example.com/_app/immutable/static/chunks/558.097941ffb76484a3.js"
                    .to_string(),
            ])
        );
    }
}
