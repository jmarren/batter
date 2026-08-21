use oxc::allocator::Allocator;
use oxc::ast::ast::{
    AssignmentExpression, AssignmentTarget, Expression, ObjectExpression, ObjectPropertyKind,
    PropertyKey,
};
use oxc::ast_visit::{Visit, walk};
use oxc::parser::Parser;
use oxc::span::SourceType;
use std::collections::HashMap;

/// Maps webpack chunk id -> content hash, extracted from a bundle's
/// `__webpack_require__.u = (chunkId) => ... { <id>: "<hash>", ... }[chunkId] ...` assignment.
type ChunkMap = HashMap<String, String>;

struct ChunkMapVisitor<'a> {
    chunk_map: Option<ChunkMap>,
    public_path: Option<String>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> ChunkMapVisitor<'a> {
    fn new() -> Self {
        Self {
            chunk_map: None,
            public_path: None,
            _marker: std::marker::PhantomData,
        }
    }
}

fn property_key_to_string(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => Some(n.raw?.to_string()),
        _ => None,
    }
}

/// If `expr` is an object literal made entirely of string-valued properties keyed by
/// chunk id (webpack's `.u` filename template), extract it as a chunk map.
fn object_expression_to_chunk_map(obj: &ObjectExpression) -> Option<ChunkMap> {
    let mut map = HashMap::new();

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            return None;
        };

        let key = property_key_to_string(&prop.key)?;
        let Expression::StringLiteral(value) = &prop.value else {
            return None;
        };

        map.insert(key, value.value.to_string());
    }

    if map.is_empty() { None } else { Some(map) }
}

/// Is the assignment target `something.u` / `something["u"]`, i.e. webpack's chunk
/// filename resolver (`__webpack_require__.u`)?
fn is_chunk_filename_target(target: &AssignmentTarget, member_name: &str) -> bool {
    match target {
        AssignmentTarget::StaticMemberExpression(m) => m.property.name == member_name,
        AssignmentTarget::ComputedMemberExpression(m) => {
            matches!(&m.expression, Expression::StringLiteral(s) if s.value == member_name)
        }
        _ => false,
    }
}

fn is_public_path_target(target: &AssignmentTarget) -> bool {
    is_chunk_filename_target(target, "p")
}

/// Walk every object literal nested in `expr` and return the first one that looks
/// like a chunk id -> hash map.
struct ObjectLiteralFinder {
    found: Option<ChunkMap>,
}

impl<'a> Visit<'a> for ObjectLiteralFinder {
    fn visit_object_expression(&mut self, it: &ObjectExpression<'a>) {
        if self.found.is_none() {
            self.found = object_expression_to_chunk_map(it);
        }
        walk::walk_object_expression(self, it);
    }
}

fn find_chunk_map_in_expression(expr: &Expression) -> Option<ChunkMap> {
    let mut finder = ObjectLiteralFinder { found: None };
    finder.visit_expression(expr);
    finder.found
}

impl<'a> Visit<'a> for ChunkMapVisitor<'a> {
    // we need a visit_assignment_expression to visit all the assignments
    // in order to locate
    /// __webpack__require.u = chunkId =>
    /// which will give us the chunkIds
    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        if self.chunk_map.is_none() && is_chunk_filename_target(&it.left, "u") {
            self.chunk_map = find_chunk_map_in_expression(&it.right);
        }

        if self.public_path.is_none()
            && is_public_path_target(&it.left)
            && let Expression::StringLiteral(s) = &it.right
        {
            self.public_path = Some(s.value.to_string());
        }

        walk::walk_assignment_expression(self, it);
    }
}

/// Extract webpack's chunk id -> hash map and public path from a bundle's source,
/// by locating the `__webpack_require__.u` filename-resolver assignment.
///
/// Returns `None` if the source doesn't contain a recognizable chunk filename map
/// (e.g. it isn't the webpack runtime chunk, or the build uses a codegen shape
/// other than an inline chunk-id -> hash object literal).
pub fn extract_chunk_map(source: &str) -> Option<(ChunkMap, Option<String>)> {
    // create allocator
    let allocator = Allocator::default();
    // use default source_type
    let source_type = SourceType::default();
    // parse source string
    let parsed = Parser::new(&allocator, source, source_type).parse();
    // none if panicked
    if parsed.panicked {
        return None;
    }
    // create a visitor
    let mut visitor = ChunkMapVisitor::new();
    visitor.visit_program(&parsed.program);

    let chunk_map = visitor.chunk_map?;
    Some((chunk_map, visitor.public_path))
}

/// Given a chunk map extracted from a bundle, reconstruct the filenames webpack
/// would request for every known chunk, using the common `{id}.{hash}.js` naming
/// convention (webpack's default `chunkFilename` template).
pub fn chunk_filenames(chunk_map: &ChunkMap, public_path: Option<&str>) -> Vec<String> {
    let public_path = public_path.unwrap_or("");

    chunk_map
        .iter()
        .map(|(id, hash)| format!("{public_path}{id}.{hash}.js"))
        .collect()
}
