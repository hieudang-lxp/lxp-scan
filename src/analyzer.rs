use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{anyhow, bail};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    IdentifierReference, ImportDeclarationSpecifier, JSXAttributeItem, JSXAttributeName,
    JSXElementName, JSXMemberExpression, JSXMemberExpressionObject, JSXOpeningElement, Statement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

#[derive(Debug)]
pub struct FileFindings {
    /// 1-based line of the matching import statement
    pub line: usize,
    /// raw import specifier as written in the source
    pub source: String,
    /// Identifier references to the local binding, excluding the import
    /// binding itself and plain JSX tag names (those are `jsx_uses`).
    /// INCLUDES the root of member-expression JSX tags (`<Button.Icon />`
    /// counts one ref to `Button`) and type-position references (`import
    /// type`, type annotations) — for an impact scanner, type usage IS impact.
    /// No scope analysis: same-name shadowing counts.
    pub refs: usize,
    /// number of times used as a JSX element with a plain-identifier tag
    /// (`<Button />`; member-expression tags like `<Button.Icon />` are refs)
    pub jsx_uses: usize,
    /// union of prop names across JSX uses
    pub jsx_props: BTreeSet<String>,
    /// 1-based line of each `jsx_uses` occurrence, in source order — lets
    /// consumers excerpt the render site instead of the import line
    pub jsx_lines: Vec<usize>,
}

/// One FileFindings per matching import of `symbol` (named import matching the
/// SOURCE name incl. `as` renames, or default import matching the local name).
/// Namespace imports (import * as X) are skipped (v1 limitation).
/// Files that fail to parse yield Err.
pub fn analyze_file(
    path: &Path,
    source_text: &str,
    symbol: &str,
) -> anyhow::Result<Vec<FileFindings>> {
    let allocator = Allocator::default();
    let mut source_type = SourceType::from_path(path)
        .map_err(|e| anyhow!("unsupported source type for {}: {e}", path.display()))?;
    // Legacy .js files in the target repos routinely contain JSX, but oxc's
    // default SourceType for .js has JSX disabled. Enable it for .js only:
    // .jsx/.tsx already have it, and .ts must keep it off (JSX syntax is
    // ambiguous with generics/type assertions in .ts).
    if path.extension().and_then(|e| e.to_str()) == Some("js") {
        source_type = source_type.with_jsx(true);
    }
    let ret = Parser::new(&allocator, source_text, source_type).parse();

    // Err criterion (verified against oxc 0.139 behavior): the parser is
    // error-tolerant, so recoverable syntax errors still yield a usable AST and
    // must NOT abort analysis. We treat a file as broken only when the parser
    // gave up: `panicked` is true (unrecoverable error, program is empty), or
    // it reported error-severity diagnostics AND produced no statements at all
    // (no usable AST). `has_errors()` checks Severity::Error only, so files
    // with mere warnings are never classified as Err. For the probe input
    // `import { from 'nope`, oxc 0.139 returns panicked=true with 1 error
    // diagnostic and an empty program body.
    if ret.panicked || (ret.diagnostics.has_errors() && ret.program.body.is_empty()) {
        bail!(
            "failed to parse {}: {} parser diagnostic(s)",
            path.display(),
            ret.diagnostics.len()
        );
    }

    // Pass 1: find imports of `symbol`, recording the local binding name.
    // (local name, import source, 1-based line of the import statement)
    let mut matches: Vec<(String, String, usize)> = Vec::new();
    for stmt in &ret.program.body {
        let Statement::ImportDeclaration(decl) = stmt else {
            continue;
        };
        let Some(specifiers) = &decl.specifiers else {
            continue;
        };
        for spec in specifiers {
            let local = match spec {
                // Named import: match on the SOURCE name (handles `as` renames),
                // then track the local binding for usage counting.
                ImportDeclarationSpecifier::ImportSpecifier(s)
                    if s.imported.name().as_str() == symbol =>
                {
                    s.local.name.as_str()
                }
                // Default import: only the local name exists.
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s)
                    if s.local.name.as_str() == symbol =>
                {
                    s.local.name.as_str()
                }
                // Namespace imports (`import * as X`) are skipped: v1 limitation.
                _ => continue,
            };
            let line = line_of(source_text, decl.span.start);
            matches.push((local.to_string(), decl.source.value.to_string(), line));
        }
    }

    // Pass 2: one usage-collection walk per matched import.
    let findings = matches
        .into_iter()
        .map(|(local, source, line)| {
            let mut collector = UsageCollector {
                local: &local,
                refs: 0,
                jsx_uses: 0,
                jsx_props: BTreeSet::new(),
                jsx_spans: Vec::new(),
            };
            collector.visit_program(&ret.program);
            FileFindings {
                line,
                source,
                refs: collector.refs,
                jsx_uses: collector.jsx_uses,
                jsx_props: collector.jsx_props,
                jsx_lines: collector
                    .jsx_spans
                    .iter()
                    .map(|offset| line_of(source_text, *offset))
                    .collect(),
            }
        })
        .collect();
    Ok(findings)
}

/// Finds the top-level declaration of `name` in one file: `const/function/
/// class/interface/type/enum`, plain or behind `export`/`export default`.
/// Re-exports (`export * from`, `export { X } from`) and imports do NOT count —
/// scanning a repo with this therefore lands on the real definition, not
/// barrel files. Returns 1-based (start_line, end_line) of the declaration
/// statement (including the `export` keyword when present).
pub fn find_declaration(
    path: &Path,
    source_text: &str,
    name: &str,
) -> anyhow::Result<Option<(usize, usize)>> {
    use oxc_ast::ast::{Declaration, ExportDefaultDeclarationKind};
    use oxc_span::GetSpan;

    let allocator = Allocator::default();
    let mut source_type = SourceType::from_path(path)
        .map_err(|e| anyhow!("unsupported source type for {}: {e}", path.display()))?;
    if path.extension().and_then(|e| e.to_str()) == Some("js") {
        source_type = source_type.with_jsx(true);
    }
    let ret = Parser::new(&allocator, source_text, source_type).parse();
    if ret.panicked || (ret.diagnostics.has_errors() && ret.program.body.is_empty()) {
        bail!(
            "failed to parse {}: {} parser diagnostic(s)",
            path.display(),
            ret.diagnostics.len()
        );
    }

    fn declares(decl: &Declaration, name: &str) -> bool {
        match decl {
            Declaration::VariableDeclaration(v) => v
                .declarations
                .iter()
                .any(|d| d.id.get_identifier_name().is_some_and(|n| n == name)),
            Declaration::FunctionDeclaration(f) => {
                f.id.as_ref().is_some_and(|id| id.name.as_str() == name)
            }
            Declaration::ClassDeclaration(c) => {
                c.id.as_ref().is_some_and(|id| id.name.as_str() == name)
            }
            Declaration::TSInterfaceDeclaration(i) => i.id.name.as_str() == name,
            Declaration::TSTypeAliasDeclaration(t) => t.id.name.as_str() == name,
            Declaration::TSEnumDeclaration(e) => e.id.name.as_str() == name,
            _ => false,
        }
    }

    for stmt in &ret.program.body {
        let span = match stmt {
            Statement::ExportNamedDeclaration(e) => match &e.declaration {
                Some(decl) if declares(decl, name) => Some(e.span),
                _ => None,
            },
            Statement::ExportDefaultDeclaration(e) => match &e.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(f)
                    if f.id.as_ref().is_some_and(|id| id.name.as_str() == name) =>
                {
                    Some(e.span)
                }
                ExportDefaultDeclarationKind::ClassDeclaration(c)
                    if c.id.as_ref().is_some_and(|id| id.name.as_str() == name) =>
                {
                    Some(e.span)
                }
                _ => None,
            },
            other => match other.as_declaration() {
                Some(decl) if declares(decl, name) => Some(decl.span()),
                _ => None,
            },
        };
        if let Some(span) = span {
            return Ok(Some((
                line_of(source_text, span.start),
                line_of(source_text, span.end),
            )));
        }
    }
    Ok(None)
}

/// Lists exported VALUE declarations (`export const/function/class`, plus
/// named `export default function/class`) as (name, 1-based line). Type-only
/// declarations (interface/type/enum) and re-exports are excluded: for
/// duplicate detection a shared interface name is noise, a shared component
/// or util implementation is the signal.
pub fn exported_values(path: &Path, source_text: &str) -> anyhow::Result<Vec<(String, usize)>> {
    use oxc_ast::ast::{Declaration, ExportDefaultDeclarationKind};

    let allocator = Allocator::default();
    let mut source_type = SourceType::from_path(path)
        .map_err(|e| anyhow!("unsupported source type for {}: {e}", path.display()))?;
    if path.extension().and_then(|e| e.to_str()) == Some("js") {
        source_type = source_type.with_jsx(true);
    }
    let ret = Parser::new(&allocator, source_text, source_type).parse();
    if ret.panicked || (ret.diagnostics.has_errors() && ret.program.body.is_empty()) {
        bail!(
            "failed to parse {}: {} parser diagnostic(s)",
            path.display(),
            ret.diagnostics.len()
        );
    }

    let mut out = Vec::new();
    for stmt in &ret.program.body {
        match stmt {
            Statement::ExportNamedDeclaration(e) => {
                let line = line_of(source_text, e.span.start);
                match &e.declaration {
                    Some(Declaration::VariableDeclaration(v)) => {
                        for d in &v.declarations {
                            if let Some(name) = d.id.get_identifier_name() {
                                out.push((name.to_string(), line));
                            }
                        }
                    }
                    Some(Declaration::FunctionDeclaration(f)) => {
                        if let Some(id) = &f.id {
                            out.push((id.name.to_string(), line));
                        }
                    }
                    Some(Declaration::ClassDeclaration(c)) => {
                        if let Some(id) = &c.id {
                            out.push((id.name.to_string(), line));
                        }
                    }
                    _ => {}
                }
            }
            Statement::ExportDefaultDeclaration(e) => {
                let line = line_of(source_text, e.span.start);
                match &e.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                        if let Some(id) = &f.id {
                            out.push((id.name.to_string(), line));
                        }
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                        if let Some(id) = &c.id {
                            out.push((id.name.to_string(), line));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Counts usages of one local binding.
///
/// Semantics contract:
/// - `refs` counts `IdentifierReference` nodes only. The import binding itself
///   is a `BindingIdentifier` (distinct node type), so it is never counted.
///   Type-position references (`import type`, `let x: Button`) DO count — for
///   an impact scanner, type usage is impact. No scope analysis is performed,
///   so a same-name shadowing binding's references also count.
/// - JSX tag semantics: a plain matching tag (`<Button>`) counts as one
///   `jsx_uses` (with its props collected); a member-expression tag whose ROOT
///   is the binding (`<Button.Icon>`) counts as one `refs` — the component
///   rendered is `Button.Icon`, not `Button`, so it is neither a `jsx_uses`
///   nor a props source (v1). Closing tags are always excluded.
/// - In oxc 0.139 a capitalized JSX tag (`<Button>`) is
///   `JSXElementName::IdentifierReference` — an actual `IdentifierReference`
///   node — so the default walk WOULD count it as a ref. We override
///   `visit_jsx_element_name` to a no-op so opening AND closing tag names
///   never reach `visit_identifier_reference`, while attribute values and
///   children are still walked normally. All tag-name accounting happens in
///   `visit_jsx_opening_element` (NOT in `visit_jsx_element_name`, which also
///   fires for closing tags and would double-count member-expression roots).
struct UsageCollector<'s> {
    local: &'s str,
    refs: usize,
    jsx_uses: usize,
    jsx_props: BTreeSet<String>,
    /// byte offset of each plain-tag JSX use; converted to lines by the caller
    jsx_spans: Vec<u32>,
}

/// 1-based line number of a byte offset.
fn line_of(source_text: &str, offset: u32) -> usize {
    source_text[..offset as usize]
        .bytes()
        .filter(|b| *b == b'\n')
        .count()
        + 1
}

impl<'a> Visit<'a> for UsageCollector<'_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if it.name.as_str() == self.local {
            self.refs += 1;
        }
    }

    fn visit_jsx_element_name(&mut self, _it: &JSXElementName<'a>) {
        // Intentionally empty: tag names must not count as identifier refs.
    }

    fn visit_jsx_opening_element(&mut self, it: &JSXOpeningElement<'a>) {
        match &it.name {
            // Plain component tag: a JSX use; collect its props.
            // (Lowercase intrinsic tags like `<div>` are the separate
            // `JSXElementName::Identifier` variant and can never be a binding.)
            JSXElementName::IdentifierReference(id) if id.name.as_str() == self.local => {
                self.jsx_uses += 1;
                self.jsx_spans.push(it.span.start);
                for attr in &it.attributes {
                    if let JSXAttributeItem::Attribute(attribute) = attr
                        && let JSXAttributeName::Identifier(name) = &attribute.name
                    {
                        self.jsx_props.insert(name.name.to_string());
                    }
                }
            }
            // Member-expression tag: the root object is a genuine reference to
            // the binding, but the component rendered is the member — count a
            // ref, not a jsx_use, and don't collect props (v1).
            JSXElementName::MemberExpression(member) => {
                if let Some(root) = jsx_member_root(member)
                    && root.name.as_str() == self.local
                {
                    self.refs += 1;
                }
            }
            _ => {}
        }
        // Keep walking: attribute values may contain identifier refs. The tag
        // name is skipped via the visit_jsx_element_name override above.
        walk::walk_jsx_opening_element(self, it);
    }
}

/// Resolves the root identifier of a (possibly nested) member-expression JSX
/// tag: `<A.B.C />` -> `A`. Returns None for `<this.X />`.
fn jsx_member_root<'a, 'b>(
    member: &'b JSXMemberExpression<'a>,
) -> Option<&'b IdentifierReference<'a>> {
    let mut object = &member.object;
    loop {
        match object {
            JSXMemberExpressionObject::IdentifierReference(id) => return Some(id),
            JSXMemberExpressionObject::MemberExpression(inner) => object = &inner.object,
            JSXMemberExpressionObject::ThisExpression(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Only the extension matters for SourceType detection.
    fn tsx(name: &str) -> PathBuf {
        PathBuf::from(format!("/fixtures/{name}.tsx"))
    }

    #[test]
    fn jsx_in_plain_js_file_parses_and_matches() {
        // lxp-web has hundreds of legacy .js files containing JSX; they must
        // parse (oxc's default SourceType for .js has JSX disabled).
        let src = r#"
import Button from 'fake-lib/components/Button'
export const P = () => <Button disabled />
"#;
        let path = PathBuf::from("/fixtures/legacy.js");
        let findings = analyze_file(&path, src, "Button").unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].jsx_uses, 1);
    }

    #[test]
    fn named_import_with_jsx_props_and_refs() {
        let src = r#"
import { Button } from 'fake-lib/components/Button'
const wrapped = Button
export const P = () => <Button variant="primary" size="large">hi</Button>
"#;
        let findings = analyze_file(&tsx("named"), src, "Button").unwrap();
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.source, "fake-lib/components/Button");
        assert_eq!(f.line, 2);
        assert_eq!(f.refs, 1, "only `wrapped = Button` counts as a ref");
        assert_eq!(f.jsx_uses, 1);
        let expected: BTreeSet<String> =
            ["variant", "size"].iter().map(|s| s.to_string()).collect();
        assert_eq!(f.jsx_props, expected);
    }

    #[test]
    fn renamed_import_matches_source_name_and_tracks_local() {
        let src = r#"
import { Button as Btn } from 'fake-lib/components/Button'
export const P = () => <Btn disabled />
"#;
        let findings = analyze_file(&tsx("renamed"), src, "Button").unwrap();
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.jsx_uses, 1);
        let expected: BTreeSet<String> = ["disabled"].iter().map(|s| s.to_string()).collect();
        assert_eq!(f.jsx_props, expected);
    }

    #[test]
    fn member_expression_jsx_tag_counts_root_as_ref() {
        let src = r#"
import { Button } from 'lib'
export const P = () => <Button.Icon color="red" />
"#;
        let findings = analyze_file(&tsx("member"), src, "Button").unwrap();
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.refs, 1, "root of <Button.Icon /> is a ref to Button");
        assert_eq!(
            f.jsx_uses, 0,
            "the component rendered is Button.Icon, not Button"
        );
        assert!(
            f.jsx_props.is_empty(),
            "props belong to Button.Icon, not Button"
        );
    }

    #[test]
    fn default_import_matches_local_name() {
        let src = r#"
import Whole from './whole'
Whole()
"#;
        let findings = analyze_file(&tsx("default"), src, "Whole").unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].refs, 1);
    }

    #[test]
    fn jsx_use_line_numbers_are_recorded_in_order() {
        let src = r#"
import { Button } from 'lib'
const pad = 1
export const P = () => (
  <Button a />
)
export const Q = () => <Button b />
"#;
        let findings = analyze_file(&tsx("lines"), src, "Button").unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].jsx_lines, vec![5, 7]);
    }

    #[test]
    fn member_expression_and_ref_only_uses_record_no_jsx_lines() {
        let src = r#"
import { Button } from 'lib'
const wrapped = Button
export const P = () => <Button.Icon />
"#;
        let findings = analyze_file(&tsx("nolines"), src, "Button").unwrap();
        assert!(findings[0].jsx_lines.is_empty());
    }

    #[test]
    fn unrelated_symbol_with_same_substring_is_not_matched() {
        let src = r#"
import { ButtonGroup } from 'fake-lib/components/ButtonGroup'
export const P = () => <ButtonGroup />
"#;
        let findings = analyze_file(&tsx("substring"), src, "Button").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn local_symbol_without_import_is_not_matched() {
        let src = r#"
const Button = 'local'
console.log(Button)
"#;
        let findings = analyze_file(&tsx("local"), src, "Button").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn broken_file_is_an_error_not_a_panic() {
        let src = "import { from 'nope";
        let result = analyze_file(&tsx("broken"), src, "Button");
        assert!(result.is_err());
    }

    #[test]
    fn exported_values_lists_const_function_class_but_not_types_or_reexports() {
        let src = r#"
export const Button = () => null
export function formatDate() {}
export class Store {}
export interface ButtonProps { a?: string }
export type Kind = 'a'
export * from './other'
export { Inner } from './inner'
const Hidden = () => null
"#;
        let exports = exported_values(&tsx("exports"), src).unwrap();
        let names: Vec<(&str, usize)> = exports
            .iter()
            .map(|(name, line)| (name.as_str(), *line))
            .collect();
        assert_eq!(
            names,
            vec![("Button", 2), ("formatDate", 3), ("Store", 4)],
            "types, re-exports and non-exported decls are excluded"
        );
    }

    #[test]
    fn exported_values_includes_named_default_export() {
        let src = "export default function Widget() {\n  return null\n}\n";
        let exports = exported_values(&tsx("defexp"), src).unwrap();
        assert_eq!(exports, vec![("Widget".to_string(), 1)]);
    }

    #[test]
    fn find_declaration_locates_exported_const_and_interface() {
        let src = r#"
export interface ButtonProps {
  variant?: 'primary' | 'ghost'
  disabled?: boolean
}

export const Button = ({ variant, disabled }: ButtonProps) => {
  return null
}
"#;
        let decl = find_declaration(&tsx("def"), src, "Button").unwrap();
        assert_eq!(decl, Some((7, 9)));
        let props = find_declaration(&tsx("def"), src, "ButtonProps").unwrap();
        assert_eq!(props, Some((2, 5)));
    }

    #[test]
    fn find_declaration_matches_function_and_default_export_forms() {
        let src = "export default function Button() {\n  return null\n}\n";
        let decl = find_declaration(&tsx("deffn"), src, "Button").unwrap();
        assert_eq!(decl, Some((1, 3)));
        let src2 = "function Card() {\n  return null\n}\nexport { Card }\n";
        let decl2 = find_declaration(&tsx("plainfn"), src2, "Card").unwrap();
        assert_eq!(decl2, Some((1, 3)));
    }

    #[test]
    fn reexports_and_imports_are_not_declarations() {
        let src = r#"
import { Button } from './inner'
export * from './Button'
export { Button as default } from './Button'
"#;
        let decl = find_declaration(&tsx("reexport"), src, "Button").unwrap();
        assert_eq!(decl, None);
    }
}
