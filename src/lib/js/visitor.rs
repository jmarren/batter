use std::collections::HashSet;

use oxc::{
    ast::ast::{
        AssignmentExpression, AssignmentTarget, CallExpression, Expression, Program, StringLiteral,
        VariableDeclarator,
    },
    ast_visit::{Visit, walk},
};

use crate::js::{buildmanifest, endpoints, source::ParseResult, turbopack, urls, vite, webpack};

pub struct JsVisitor<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
    pub chunk_urls: HashSet<String>,
    pub endpoints: HashSet<String>,
    pub urls: HashSet<String>,
}

impl<'a> JsVisitor<'a> {
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            chunk_urls: HashSet::new(),
            endpoints: HashSet::new(),
            urls: HashSet::new(),
        }
    }

    pub fn parse(mut self, parsed_program: Program<'a>) -> ParseResult {
        // visit the program and return fields as ParseResult
        self.visit_program(&parsed_program);
        ParseResult {
            chunk_urls: self.chunk_urls,
            endpoints: self.endpoints,
            urls: self.urls,
        }
    }
}

impl<'a> Visit<'a> for JsVisitor<'a> {
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
                            self.chunk_urls.extend(webpack::chunk_urls(arrow_fn));
                        }
                    }
                    _ => (),
                };
            }
            // next.js `self.__BUILD_MANIFEST = function(...) { return {...} }(...)`
            Expression::CallExpression(call) => {
                self.chunk_urls
                    .extend(buildmanifest::build_manifest_chunk_urls(&it.left, call));
            }
            _ => (),
        };
    }

    // turbopack registers chunks via a `.push([scriptEl, {otherChunks: [...]}])`
    // call, e.g. `(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push(...)`
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        self.chunk_urls.extend(turbopack::turbopack_chunk_urls(it));

        if let Some(endpoint) = endpoints::extract_endpoint(it) {
            self.endpoints.insert(endpoint);
        }

        walk::walk_call_expression(self, it);
    }

    fn visit_string_literal(&mut self, it: &StringLiteral<'a>) {
        if urls::is_url(&it.value) {
            self.urls.insert(it.value.to_string());
        }
    }

    // vite's `const __vite__mapDeps = (i, m = ..., d = m.f || (m.f = [...])) => ...`
    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        self.chunk_urls.extend(vite::vite_chunk_urls(it));
        walk::walk_variable_declarator(self, it);
    }
}
