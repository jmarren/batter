use anyhow::{Result, anyhow};
use oxc::{
    allocator::Allocator,
    ast::ast::{
        ArrowFunctionExpression, AssignmentExpression, AssignmentTarget, BindingPattern,
        ConditionalExpression, Expression, ObjectExpression, ObjectPropertyKind, PropertyKey,
        Statement,
    },
    ast_visit::Visit,
    parser::Parser,
    span::{GetSpan, SourceType, Span},
};
use std::collections::HashMap;

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

        walker.spans.iter().for_each(|span| {
            println!(
                "{}",
                &self.source_text[span.start as usize..span.end as usize]
            );
        });

        Ok(())
    }
}

struct JsWalker<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
    spans: Vec<Span>,
}

impl<'a> JsWalker<'a> {
    fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            spans: vec![],
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
                            chunk_urls(arrow_fn);
                            // self.spans.push(it.right.span());

                            // self.chunk_urls.extend(chunk_urls_from_arrow(arrow_fn));
                        }
                    }
                    _ => (),
                };
            }
            _ => (),
        };
    }
}

fn chunk_urls<'a>(arrow_fn: &oxc::allocator::Box<'a, ArrowFunctionExpression<'a>>) {
    let Some(body) = arrow_body_expression(&arrow_fn) else {
        return;
    };
    // println!("body = {:?}", body);

    let mut out = vec![];
    flatten_binary_plus_chain(body, &mut out);

    out.iter().for_each(|e| {
        match e {
            Expression::StringLiteral(s) => match s.raw {
                Some(raw) if raw == "." => {
                    println!("got dot");
                }
                Some(raw) => {
                    println!("raw = {:?}", raw);
                }
                _ => (),
            },
            Expression::ConditionalExpression(c) => {
                println!("c = {:?}", c);
            }
            Expression::ParenthesizedExpression(p) => match &p.expression {
                Expression::ConditionalExpression(c) => match &c.consequent {
                    Expression::StringLiteral(s) => {
                        println!("consequent = {:?}", s.raw);
                    }
                    _ => (),
                },
                _ => (),
            },
            _ => (),
        }
        // println!("{}", e);
    });
}

// /// The chunk id -> filename-fragment map read out of webpack's `.u` resolver body,
// /// e.g. the `{558: "097941...", ...}` object literal indexed by the chunk id param.
// fn property_key_to_string(key: &PropertyKey) -> Option<String> {
//     match key {
//         PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
//         PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
//         PropertyKey::NumericLiteral(n) => Some(n.raw?.to_string()),
//         _ => None,
//     }
// }
//
// fn object_expression_to_map(obj: &ObjectExpression) -> Option<HashMap<String, String>> {
//     let mut map = HashMap::new();
//
//     for prop in &obj.properties {
//         let ObjectPropertyKind::ObjectProperty(prop) = prop else {
//             return None;
//         };
//
//         let key = property_key_to_string(&prop.key)?;
//         let Expression::StringLiteral(value) = &prop.value else {
//             return None;
//         };
//
//         map.insert(key, value.value.to_string());
//     }
//
//     if map.is_empty() { None } else { Some(map) }
// }
//
// /// If `expr` is `<id> === <param> ? <consequent> : <param>` (webpack's special-cased
// /// "current chunk" ternary), and `<id>` matches `chunk_id`, evaluate to the consequent
// /// literal. Otherwise falls back to `chunk_id` itself, matching the alternate branch.
// fn eval_conditional(cond: &ConditionalExpression, param_name: &str, chunk_id: &str) -> String {
//     let is_match = match &cond.test {
//         Expression::BinaryExpression(bin) => {
//             let operands = [&bin.left, &bin.right];
//             let has_id = operands
//                 .iter()
//                 .any(|e| matches!(e, Expression::NumericLiteral(n) if n.raw.is_some_and(|r| r == chunk_id)));
//             let has_param = operands
//                 .iter()
//                 .any(|e| matches!(e, Expression::Identifier(i) if i.name == param_name));
//             has_id && has_param
//         }
//         _ => false,
//     };
//
//     if is_match {
//         eval_expression(&cond.consequent, param_name, chunk_id)
//             .unwrap_or_else(|| chunk_id.to_string())
//     } else {
//         chunk_id.to_string()
//     }
// }
//
// /// Evaluate one operand of the `.u` resolver's `+` chain for a specific chunk id.
// /// Handles the shapes webpack actually emits: string literals, the chunk id
// /// identifier itself, the "current chunk" ternary, and the id -> hash lookup table.
// fn eval_expression(expr: &Expression, param_name: &str, chunk_id: &str) -> Option<String> {
//     match expr {
//         Expression::StringLiteral(s) => Some(s.value.to_string()),
//         Expression::Identifier(id) if id.name == param_name => Some(chunk_id.to_string()),
//         Expression::ConditionalExpression(cond) => {
//             Some(eval_conditional(cond, param_name, chunk_id))
//         }
//         Expression::ComputedMemberExpression(member) => {
//             let Expression::Identifier(index) = &member.expression else {
//                 return None;
//             };
//             if index.name != param_name {
//                 return None;
//             }
//             let Expression::ObjectExpression(obj) = &member.object else {
//                 return None;
//             };
//             let map = object_expression_to_map(obj)?;
//             map.get(chunk_id).cloned()
//         }
//         Expression::ParenthesizedExpression(inner) => {
//             eval_expression(&inner.expression, param_name, chunk_id)
//         }
//         _ => None,
//     }
// }
//
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
//
// /// Collect every chunk id referenced by the resolver: keys of the id -> hash object
// /// literal, plus any id special-cased in a `<id> === <param>` ternary test.
// fn collect_chunk_ids(expr: &Expression, ids: &mut Vec<String>) {
//     match expr {
//         Expression::ObjectExpression(obj) => {
//             if let Some(map) = object_expression_to_map(obj) {
//                 ids.extend(map.into_keys());
//             }
//         }
//         Expression::ConditionalExpression(cond) => {
//             if let Expression::BinaryExpression(bin) = &cond.test {
//                 for operand in [&bin.left, &bin.right] {
//                     if let Expression::NumericLiteral(n) = operand
//                         && let Some(raw) = n.raw
//                     {
//                         ids.push(raw.to_string());
//                     }
//                 }
//             }
//             collect_chunk_ids(&cond.consequent, ids);
//             collect_chunk_ids(&cond.alternate, ids);
//         }
//         Expression::ComputedMemberExpression(member) => {
//             collect_chunk_ids(&member.object, ids);
//         }
//         Expression::BinaryExpression(bin) => {
//             collect_chunk_ids(&bin.left, ids);
//             collect_chunk_ids(&bin.right, ids);
//         }
//         Expression::ParenthesizedExpression(inner) => {
//             collect_chunk_ids(&inner.expression, ids);
//         }
//         _ => {}
//     }
// }
//
// fn arrow_param_name<'a>(arrow_fn: &ArrowFunctionExpression<'a>) -> Option<&'a str> {
//     let first = arrow_fn.params.items.first()?;
//     let BindingPattern::BindingIdentifier(id) = &first.pattern else {
//         return None;
//     };
//     Some(id.name.as_str())
// }
//
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
//
// /// Extract every chunk URL reachable from webpack's `.u` filename-resolver arrow
// /// function (`chunkId => "prefix/" + ... + {id: hash, ...}[chunkId] + ".js"`).
// ///
// /// Returns an empty vec if the body doesn't match a shape we know how to evaluate.
// pub fn chunk_urls_from_arrow(arrow_fn: &ArrowFunctionExpression) -> Vec<String> {
//     let Some(param_name) = arrow_param_name(arrow_fn) else {
//         return vec![];
//     };
//
//     let Some(body) = arrow_body_expression(arrow_fn) else {
//         return vec![];
//     };
//
//     let mut ids = Vec::new();
//     collect_chunk_ids(body, &mut ids);
//     ids.sort();
//     ids.dedup();
//
//     let mut operands = Vec::new();
//     flatten_binary_plus_chain(body, &mut operands);
//
//     ids.into_iter()
//         .filter_map(|id| {
//             let mut url = String::new();
//             for operand in &operands {
//                 url.push_str(&eval_expression(operand, param_name, &id)?);
//             }
//             Some(url)
//         })
//         .collect()
// }
