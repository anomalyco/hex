//! TypeScript to JavaScript by erasure: every type-only piece of the source is
//! replaced with spaces, so the JavaScript QuickJS runs keeps the original
//! lines and columns and its errors point at the user's own file. Syntax that
//! would need generated code (enums, namespaces, parameter properties,
//! `import =`) is rejected instead of compiled.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Class, ClassElement, Declaration, ExportDefaultDeclarationKind, ExportSpecifier,
    FormalParameter, ImportDeclarationSpecifier, ImportOrExportKind, MethodDefinition,
    MethodDefinitionType, PropertyDefinition, PropertyDefinitionType, Statement, TSAsExpression,
    TSClassImplements, TSNonNullExpression, TSSatisfiesExpression, TSThisParameter,
    TSTypeAnnotation, TSTypeAssertion, TSTypeParameterDeclaration, TSTypeParameterInstantiation,
    VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, SourceType, Span};

/// Words that only TypeScript allows before a class member's key.
const MEMBER_MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "readonly",
    "override",
    "abstract",
    "declare",
];

/// `source` as JavaScript with its types blanked, or the first syntax or
/// unsupported-syntax error as `line:column: message`.
pub fn strip(source: &str) -> Result<String, String> {
    let allocator = Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, SourceType::ts()).parse();
    if let Some(diagnostic) = parsed.diagnostics.first() {
        let offset = diagnostic.labels.first().map_or(0, |label| label.offset());
        return Err(format!(
            "{}: {}",
            location(source, offset),
            diagnostic.message
        ));
    }
    let mut eraser = Eraser {
        source: source.as_bytes(),
        blanks: Vec::new(),
        error: None,
    };
    eraser.visit_program(&parsed.program);
    if let Some((offset, message)) = eraser.error {
        return Err(format!("{}: {message}", location(source, offset)));
    }
    let mut output = source.as_bytes().to_vec();
    for (start, end) in eraser.blanks {
        for byte in &mut output[start as usize..end as usize] {
            if *byte != b'\n' && *byte != b'\r' {
                *byte = b' ';
            }
        }
    }
    // Only whole UTF-8 sequences inside spans were replaced, byte for byte, by
    // ASCII spaces, so the output is still UTF-8.
    String::from_utf8(output).map_err(|_| "type stripping produced invalid UTF-8".into())
}

fn location(source: &str, offset: u32) -> String {
    let offset = (offset as usize).min(source.len());
    let before = &source.as_bytes()[..offset];
    let line = before.iter().filter(|byte| **byte == b'\n').count() + 1;
    let column = offset
        - before
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |at| at + 1)
        + 1;
    format!("{line}:{column}")
}

struct Eraser<'s> {
    source: &'s [u8],
    blanks: Vec<(u32, u32)>,
    error: Option<(u32, &'static str)>,
}

impl Eraser<'_> {
    fn blank(&mut self, span: Span) {
        if span.start < span.end {
            self.blanks.push((span.start, span.end));
        }
    }

    fn unsupported(&mut self, span: Span, message: &'static str) {
        self.error.get_or_insert((span.start, message));
    }

    /// Blanks `span` and the comma that follows it, as for an element of a
    /// list (`this: T,`, `type A,`).
    fn blank_list_item(&mut self, span: Span) {
        let mut end = span.end as usize;
        while self.source.get(end).is_some_and(u8::is_ascii_whitespace) {
            end += 1;
        }
        if self.source.get(end) == Some(&b',') {
            end += 1;
        }
        self.blank(Span::new(span.start, end as u32));
    }

    /// Blanks the `?` or `!` that follows a key or binding at `after`,
    /// skipping whitespace and a computed key's closing bracket.
    fn blank_marker(&mut self, after: u32, marker: u8) {
        let mut at = after as usize;
        while let Some(byte) = self.source.get(at) {
            if byte.is_ascii_whitespace() || *byte == b']' {
                at += 1;
            } else {
                break;
            }
        }
        if self.source.get(at) == Some(&marker) {
            self.blank(Span::new(at as u32, at as u32 + 1));
        }
    }

    /// Blanks each TypeScript-only modifier word in `[start, end)`.
    fn blank_modifiers(&mut self, start: u32, end: u32) {
        let mut at = start as usize;
        let end = end as usize;
        while at < end {
            if !is_word_byte(self.source[at]) {
                at += 1;
                continue;
            }
            let word_start = at;
            while at < end && is_word_byte(self.source[at]) {
                at += 1;
            }
            let word = &self.source[word_start..at];
            if MEMBER_MODIFIERS
                .iter()
                .any(|modifier| modifier.as_bytes() == word)
            {
                self.blank(Span::new(word_start as u32, at as u32));
            }
        }
    }

    /// The offset of `keyword` as a whole word in `[start, end)`.
    fn find_word(&self, keyword: &str, start: u32, end: u32) -> Option<u32> {
        let keyword = keyword.as_bytes();
        let (start, end) = (start as usize, end as usize);
        (start..end.saturating_sub(keyword.len()) + 1).find_map(|at| {
            let matches = self.source.get(at..at + keyword.len()) == Some(keyword)
                && (at == 0 || !is_word_byte(self.source[at - 1]))
                && !self
                    .source
                    .get(at + keyword.len())
                    .is_some_and(|byte| is_word_byte(*byte));
            matches.then_some(at as u32)
        })
    }

    /// Whether a statement-level declaration exists only for the type checker.
    fn type_only(&mut self, declaration: &Declaration<'_>) -> bool {
        match declaration {
            Declaration::TSTypeAliasDeclaration(_)
            | Declaration::TSInterfaceDeclaration(_)
            | Declaration::TSGlobalDeclaration(_)
            | Declaration::TSExternalModuleDeclaration(_) => true,
            Declaration::TSEnumDeclaration(declaration) => {
                if !declaration.declare {
                    self.unsupported(
                        declaration.span,
                        "TypeScript enums are not supported; use an object of constants",
                    );
                }
                true
            }
            Declaration::TSNamespaceDeclaration(declaration) => {
                if !declaration.declare {
                    self.unsupported(
                        declaration.span,
                        "TypeScript namespaces are not supported; use a module or an object",
                    );
                }
                true
            }
            Declaration::TSImportEqualsDeclaration(declaration) => {
                if declaration.import_kind != ImportOrExportKind::Type {
                    self.unsupported(
                        declaration.span,
                        "`import x = require(...)` is not supported; use `import`",
                    );
                }
                true
            }
            Declaration::VariableDeclaration(declaration) => declaration.declare,
            Declaration::FunctionDeclaration(function) => {
                function.declare || function.body.is_none()
            }
            Declaration::ClassDeclaration(class) => class.declare,
        }
    }
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

impl<'a> Visit<'a> for Eraser<'_> {
    fn visit_statement(&mut self, it: &Statement<'a>) {
        let erase = match it {
            Statement::ImportDeclaration(import) => import.import_kind == ImportOrExportKind::Type,
            Statement::ExportNamedDeclaration(export) => {
                export.export_kind == ImportOrExportKind::Type
            }
            Statement::ExportFromDeclaration(export) => {
                export.export_kind == ImportOrExportKind::Type
            }
            Statement::ExportAllDeclaration(export) => {
                export.export_kind == ImportOrExportKind::Type
            }
            Statement::ExportDeclaration(export) => self.type_only(&export.declaration),
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => true,
                ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                    function.body.is_none()
                }
                _ => false,
            },
            Statement::TSExportAssignment(assignment) => {
                self.unsupported(
                    assignment.span,
                    "`export =` is not supported; use `export default`",
                );
                true
            }
            Statement::TSNamespaceExportDeclaration(_) => true,
            _ => match it.as_declaration() {
                Some(declaration) => self.type_only(declaration),
                None => false,
            },
        };
        if erase {
            self.blank(it.span());
        } else {
            walk::walk_statement(self, it);
        }
    }

    fn visit_import_declaration_specifier(&mut self, it: &ImportDeclarationSpecifier<'a>) {
        if let ImportDeclarationSpecifier::ImportSpecifier(specifier) = it
            && specifier.import_kind == ImportOrExportKind::Type
        {
            self.blank_list_item(specifier.span);
        }
    }

    fn visit_export_specifier(&mut self, it: &ExportSpecifier<'a>) {
        if it.export_kind == ImportOrExportKind::Type {
            self.blank_list_item(it.span);
        }
    }

    fn visit_ts_type_annotation(&mut self, it: &TSTypeAnnotation<'a>) {
        self.blank(it.span);
    }

    fn visit_ts_type_parameter_declaration(&mut self, it: &TSTypeParameterDeclaration<'a>) {
        self.blank(it.span);
    }

    fn visit_ts_type_parameter_instantiation(&mut self, it: &TSTypeParameterInstantiation<'a>) {
        self.blank(it.span);
    }

    fn visit_ts_class_implements(&mut self, _it: &TSClassImplements<'a>) {}

    fn visit_ts_this_parameter(&mut self, it: &TSThisParameter<'a>) {
        self.blank_list_item(it.span);
    }

    fn visit_ts_as_expression(&mut self, it: &TSAsExpression<'a>) {
        self.blank(Span::new(it.expression.span().end, it.span.end));
        self.visit_expression(&it.expression);
    }

    fn visit_ts_satisfies_expression(&mut self, it: &TSSatisfiesExpression<'a>) {
        self.blank(Span::new(it.expression.span().end, it.span.end));
        self.visit_expression(&it.expression);
    }

    fn visit_ts_type_assertion(&mut self, it: &TSTypeAssertion<'a>) {
        self.blank(Span::new(it.span.start, it.expression.span().start));
        self.visit_expression(&it.expression);
    }

    fn visit_ts_non_null_expression(&mut self, it: &TSNonNullExpression<'a>) {
        self.blank(Span::new(it.span.end - 1, it.span.end));
        self.visit_expression(&it.expression);
    }

    fn visit_formal_parameter(&mut self, it: &FormalParameter<'a>) {
        if it.accessibility.is_some() || it.readonly || it.r#override {
            self.unsupported(
                it.span,
                "constructor parameter properties are not supported; assign fields in the constructor",
            );
        }
        if it.optional {
            self.blank_marker(it.pattern.span().end, b'?');
        }
        walk::walk_formal_parameter(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if it.definite {
            self.blank_marker(it.id.span().end, b'!');
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        if it.r#abstract
            && let Some(at) = self.find_word("abstract", it.span.start, it.body.span.start)
        {
            self.blank(Span::new(at, at + "abstract".len() as u32));
        }
        if let (Some(first), Some(last)) = (it.implements.first(), it.implements.last()) {
            let search_from = it
                .heritage
                .as_ref()
                .map(|heritage| {
                    heritage
                        .type_arguments
                        .as_ref()
                        .map_or(heritage.expression.span().end, |arguments| {
                            arguments.span.end
                        })
                })
                .or_else(|| {
                    it.type_parameters
                        .as_ref()
                        .map(|parameters| parameters.span.end)
                })
                .or_else(|| it.id.as_ref().map(|id| id.span.end))
                .unwrap_or(it.span.start);
            if let Some(at) = self.find_word("implements", search_from, first.span.start) {
                self.blank(Span::new(at, last.span.end));
            }
        }
        walk::walk_class(self, it);
    }

    fn visit_class_element(&mut self, it: &ClassElement<'a>) {
        match it {
            ClassElement::TSIndexSignature(signature) => self.blank(signature.span),
            ClassElement::MethodDefinition(method) => {
                if method.r#type == MethodDefinitionType::TSAbstractMethodDefinition
                    || method.value.body.is_none()
                {
                    self.blank(method.span);
                } else {
                    self.erase_method_modifiers(method);
                    walk::walk_method_definition(self, method);
                }
            }
            ClassElement::PropertyDefinition(property) => {
                if property.r#type == PropertyDefinitionType::TSAbstractPropertyDefinition
                    || property.declare
                {
                    self.blank(property.span);
                } else {
                    self.erase_property_modifiers(property);
                    walk::walk_property_definition(self, property);
                }
            }
            ClassElement::AccessorProperty(accessor) => {
                let start = accessor
                    .decorators
                    .last()
                    .map_or(accessor.span.start, |decorator| decorator.span.end);
                self.blank_modifiers(start, accessor.key.span().start);
                if accessor.definite {
                    self.blank_marker(accessor.key.span().end, b'!');
                }
                walk::walk_accessor_property(self, accessor);
            }
            ClassElement::StaticBlock(_) => walk::walk_class_element(self, it),
        }
    }
}

impl Eraser<'_> {
    fn erase_method_modifiers(&mut self, method: &MethodDefinition<'_>) {
        let start = method
            .decorators
            .last()
            .map_or(method.span.start, |decorator| decorator.span.end);
        self.blank_modifiers(start, method.key.span().start);
        if method.optional {
            self.blank_marker(method.key.span().end, b'?');
        }
    }

    fn erase_property_modifiers(&mut self, property: &PropertyDefinition<'_>) {
        let start = property
            .decorators
            .last()
            .map_or(property.span.start, |decorator| decorator.span.end);
        self.blank_modifiers(start, property.key.span().start);
        if property.optional {
            self.blank_marker(property.key.span().end, b'?');
        }
        if property.definite {
            self.blank_marker(property.key.span().end, b'!');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::strip;

    /// Strips `source` and compares it with `expected`, ignoring whitespace.
    fn assert_strips(source: &str, expected: &str) {
        let stripped = strip(source).unwrap();
        assert_eq!(stripped.len(), source.len(), "{stripped:?}");
        let squeeze = |text: &str| text.split_whitespace().collect::<String>();
        assert_eq!(squeeze(&stripped), squeeze(expected), "{stripped:?}");
    }

    #[test]
    fn annotations_and_generics_become_whitespace_on_the_same_lines() {
        let source = "const a: number = 1\nfunction f<T>(x: T, y?: string): T {\n  return x\n}\n";
        let stripped = strip(source).unwrap();
        assert_eq!(stripped.lines().count(), source.lines().count());
        assert_eq!(stripped.lines().nth(2), Some("  return x"));
        assert_strips(source, "const a = 1 function f(x, y) { return x }");
    }

    #[test]
    fn type_only_statements_and_specifiers_disappear() {
        assert_strips(
            "import type { A } from \"x\"\nimport { type B, c } from \"y\"\nexport type C = A\ninterface D { e: string }\nexport interface E {}\ndeclare const f: number\nexport { type G, h }\n",
            "import { c } from \"y\" export { h }",
        );
    }

    #[test]
    fn expressions_lose_assertions_and_non_null_marks() {
        assert_strips(
            "const a = (b as const)\nconst c = d satisfies E\nconst f = g!.h\nconst i = <J>k\nconst l = m<N>(o)\n",
            "const a = (b ) const c = d const f = g .h const i = k const l = m (o)",
        );
    }

    #[test]
    fn classes_lose_modifiers_implements_and_declarations() {
        assert_strips(
            "abstract class A<T> extends B<T> implements C, D<T> {\n  private readonly x?: number = 1\n  declare y: string\n  abstract z(): void\n  [key: string]: unknown\n  w!: number\n  public m(this: A, a: T): T { return a }\n  n(): void\n  n(a?: number) {}\n}\n",
            "class A extends B { x = 1 w m(a) { return a } n(a) {} }",
        );
    }

    #[test]
    fn syntax_needing_generated_code_is_rejected_with_its_location() {
        assert_eq!(
            strip("const a = 1\nenum B { C }\n").unwrap_err(),
            "2:1: TypeScript enums are not supported; use an object of constants"
        );
        assert!(strip("namespace N { export const a = 1 }").is_err());
        assert!(strip("class A { constructor(private a: number) {} }").is_err());
        assert!(strip("import fs = require(\"fs\")").is_err());
        assert!(strip("declare enum E { A }\ndeclare namespace N {}").is_ok());
    }

    #[test]
    fn syntax_errors_report_line_and_column() {
        let error = strip("const a = 1\nconst = 2\n").unwrap_err();
        assert!(error.starts_with("2:"), "{error}");
    }

    #[test]
    fn plain_javascript_is_unchanged() {
        let source = "export default { commands: { a: { phrases: [\"x\"], run: ({ hex }) => hex.press(\"a\") } } }\n";
        assert_eq!(strip(source).unwrap(), source);
    }
}
