use std::collections::HashSet;

use anyhow::{Result, anyhow};
use oxc::{
    allocator::Allocator,
    ast::ast::{
        ArrowFunctionExpression, AssignmentExpression, AssignmentTarget, ComputedMemberExpression,
        ConditionalExpression, Expression, ObjectPropertyKind, PropertyKey, Statement,
    },
    ast_visit::Visit,
    parser::Parser,
    span::SourceType,
};

use crate::util;

pub struct JsSource {
    source_text: String,
}

impl JsSource {
    pub fn new(source_text: String) -> Self {
        Self { source_text }
    }

    pub fn parse(&self) -> Result<()> {
        // create allocator
        let allocator = Allocator::default();
        // use default source_type
        let source_type = SourceType::default();
        // parse source string
        let parsed = Parser::new(&allocator, &self.source_text, source_type).parse();
        // none if panicked
        if parsed.panicked {
            return Err(anyhow!("parser panicked"));
        }

        let mut walker = JsWalker::new();

        walker.visit_program(&parsed.program);

        println!("chunk_urls = {:?}", walker.chunk_urls);

        Ok(())
    }
}

struct JsWalker<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
    chunk_urls: HashSet<String>,
}

impl<'a> JsWalker<'a> {
    fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            chunk_urls: HashSet::new(),
        }
    }
}

// (c.u = (e) =>
//   "static/chunks/" +
//   (6467 === e ? "c07e5374" : e) +
//   "." +
//   {
//     558: "097941ffb76484a3",
//     4327: "115b4e4cf5dfd289",
//     6467: "744537bcd0ee57b6",
//     6497: "746d29151a1f862f",
//     6684: "dcd4d65d61c9e94e",
//   }[e] +
//   ".js"),

impl<'a> Visit<'a> for JsWalker<'a> {
    // we need a visit_assignment_expression to visit all the assignments
    // in order to locate
    /// __webpack__require.u = chunkId =>
    /// which will give us the chunkIds
    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        match &it.right {
            Expression::ArrowFunctionExpression(arrow_fn) => {
                match &it.left {
                    AssignmentTarget::StaticMemberExpression(s) => {
                        if s.property.name == "u" {
                            self.chunk_urls.extend(chunk_urls(arrow_fn));
                        }
                    }
                    _ => (),
                };
            }
            _ => (),
        };
    }
}

fn chunk_urls<'a>(arrow_fn: &oxc::allocator::Box<'a, ArrowFunctionExpression<'a>>) -> Vec<String> {
    let Some(body) = arrow_body_expression(&arrow_fn) else {
        return vec![];
    };

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

    props
        .iter()
        .map(|(id, hash)| {
            let id_part = if override_id.as_deref() == Some(id.as_str()) {
                &consequent
            } else {
                id
            };

            format!("{prefix}{id_part}.{hash}.js")
        })
        .collect()
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
    match &computed.object {
        Expression::ParenthesizedExpression(p) => match &p.expression {
            Expression::ObjectExpression(o) => {
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
            _ => (),
        },
        _ => (),
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
