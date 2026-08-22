use anyhow::{Result, anyhow};
use oxc::{
    allocator::Allocator,
    ast::ast::{AssignmentExpression, AssignmentTarget, Expression},
    ast_visit::Visit,
    parser::Parser,
    span::{SourceType, Span},
};

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
                "{:?}",
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
//
impl<'a> Visit<'a> for JsWalker<'a> {
    // we need a visit_assignment_expression to visit all the assignments
    // in order to locate
    /// __webpack__require.u = chunkId =>
    /// which will give us the chunkIds
    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        // println!("{:?}", it.span);

        match it.right {
            Expression::ArrowFunctionExpression(_) => {
                match &it.left {
                    AssignmentTarget::StaticMemberExpression(s) => {
                        if s.property.name == "u" {
                            self.spans.push(it.span);
                        }
                    }
                    _ => (),
                };
            }
            _ => (),
        };
    }
}
