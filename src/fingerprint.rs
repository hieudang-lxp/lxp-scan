use anyhow::{anyhow, bail};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BigIntLiteral, BindingIdentifier, Declaration, ExportDefaultDeclarationKind, Expression,
    FormalParameters, Function, IdentifierName, IdentifierReference, NumericLiteral,
    PrivateIdentifier, RegExpLiteral, Statement, StringLiteral, TSTypeAnnotation, TemplateElement,
};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use std::path::Path;

use crate::analyzer::line_of;

/// Floor below which a candidate body is too trivial to cluster (passthroughs
/// like `x => !!x`). Tuned against the fixture pair in the tests below: the
/// 2-line email-validator body lands well above it, `() => true` well below.
pub const DEFAULT_MIN_TOKENS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CandidateKind {
    /// `function f() {}` declaration (incl. `export` / `export default`)
    Fn,
    /// `const f = () => {}` / `= function () {}` variable form
    Const,
}

/// One top-level function-shaped declaration, fingerprinted for clustering.
#[derive(Debug)]
pub struct Candidate {
    pub name: String,
    pub exported: bool,
    pub kind: CandidateKind,
    /// 1-based line of the declaration
    pub line: usize,
    /// display-only signature: raw param list + return type when annotated
    pub sig: String,
    /// normalized body token sequence, joined with spaces — the cluster key
    pub fingerprint: String,
    /// number of normalized body tokens (the `--min-tokens` floor input)
    pub token_count: usize,
    /// string/regex literal texts found in the body, for the cluster note
    pub literals: Vec<String>,
}

/// Extracts every top-level function-shaped declaration of one file:
/// `function f() {}`, `const f = () => {}`, `const f = function () {}`,
/// plain or behind `export` / named `export default`. Class methods,
/// object-literal methods and nested closures are skipped (v1 limitation).
///
/// The fingerprint normalizes the BODY only: identifiers (params, locals,
/// called functions, property names) become `$ID`; comments and whitespace
/// are dropped; string/regex/number literals are kept verbatim — the literal
/// IS the discriminating signal between same-shaped functions.
pub fn file_candidates(path: &Path, source_text: &str) -> anyhow::Result<Vec<Candidate>> {
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

    // One pass over the whole program marking identifier and literal spans;
    // per-candidate tokenization then only slices its body range out of the
    // sorted mark list. Comments come from the parser trivia, not the AST.
    let mut collector = MarkCollector {
        text: source_text,
        marks: Vec::new(),
    };
    collector.visit_program(&ret.program);
    let mut marks = collector.marks;
    for comment in &ret.program.comments {
        marks.push(SpanMark {
            start: comment.span.start,
            end: comment.span.end,
            token: MarkToken::Skip,
        });
    }
    marks.sort_by_key(|m| m.start);

    let mut out = Vec::new();
    for stmt in &ret.program.body {
        match stmt {
            Statement::FunctionDeclaration(f) => {
                push_function(&mut out, f, false, stmt.span().start, source_text, &marks);
            }
            Statement::VariableDeclaration(v) => {
                push_variable_fns(&mut out, v, false, stmt.span().start, source_text, &marks);
            }
            Statement::ExportNamedDeclaration(e) => match &e.declaration {
                Some(Declaration::FunctionDeclaration(f)) => {
                    push_function(&mut out, f, true, e.span.start, source_text, &marks);
                }
                Some(Declaration::VariableDeclaration(v)) => {
                    push_variable_fns(&mut out, v, true, e.span.start, source_text, &marks);
                }
                _ => {}
            },
            Statement::ExportDefaultDeclaration(e) => {
                if let ExportDefaultDeclarationKind::FunctionDeclaration(f) = &e.declaration {
                    push_function(&mut out, f, true, e.span.start, source_text, &marks);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// `function f() {}` in any position; bodyless overloads/`declare` are skipped.
fn push_function(
    out: &mut Vec<Candidate>,
    f: &Function,
    exported: bool,
    decl_start: u32,
    text: &str,
    marks: &[SpanMark],
) {
    let (Some(id), Some(body)) = (&f.id, &f.body) else {
        return;
    };
    out.push(build_candidate(
        id,
        exported,
        CandidateKind::Fn,
        decl_start,
        &f.params,
        f.return_type.as_deref(),
        body.span(),
        text,
        marks,
    ));
}

/// `const f = () => {}` / `const f = function () {}`; one candidate per
/// matching declarator. Wrapped forms (`memo(() => ...)`) are skipped — the
/// init must BE the function (v1 limitation).
fn push_variable_fns(
    out: &mut Vec<Candidate>,
    v: &oxc_ast::ast::VariableDeclaration,
    exported: bool,
    decl_start: u32,
    text: &str,
    marks: &[SpanMark],
) {
    for d in &v.declarations {
        let Some(name) = d.id.get_binding_identifier() else {
            continue;
        };
        let (params, return_type, body_span) = match &d.init {
            Some(Expression::ArrowFunctionExpression(a)) => {
                (&a.params, a.return_type.as_deref(), a.body.span())
            }
            Some(Expression::FunctionExpression(f)) => {
                let Some(body) = &f.body else { continue };
                (&f.params, f.return_type.as_deref(), body.span())
            }
            _ => continue,
        };
        out.push(build_candidate(
            name,
            exported,
            CandidateKind::Const,
            decl_start,
            params,
            return_type,
            body_span,
            text,
            marks,
        ));
    }
}

#[expect(clippy::too_many_arguments)]
fn build_candidate(
    id: &BindingIdentifier,
    exported: bool,
    kind: CandidateKind,
    decl_start: u32,
    params: &FormalParameters,
    return_type: Option<&TSTypeAnnotation>,
    body_span: Span,
    text: &str,
    marks: &[SpanMark],
) -> Candidate {
    let (tokens, literals) = tokenize(text, body_span.start, body_span.end, marks);
    Candidate {
        name: id.name.to_string(),
        exported,
        kind,
        line: line_of(text, decl_start),
        sig: render_sig(params, return_type, text),
        token_count: tokens.len(),
        fingerprint: tokens.join(" "),
        literals,
    }
}

/// Display-only: raw param list plus return type when annotated. Not part of
/// the cluster key — the fingerprint covers the body only, so a `.ts` and a
/// `.js` copy of the same body still cluster despite annotation differences.
fn render_sig(
    params: &FormalParameters,
    return_type: Option<&TSTypeAnnotation>,
    text: &str,
) -> String {
    let mut parts: Vec<&str> = params
        .items
        .iter()
        .map(|p| span_text(text, p.span()))
        .collect();
    if let Some(rest) = &params.rest {
        parts.push(span_text(text, rest.span));
    }
    let mut sig = format!("({})", parts.join(", "));
    if let Some(ret) = return_type {
        // the annotation span starts at the `:` — strip it for display
        let ty = span_text(text, ret.span).trim_start_matches(':').trim();
        sig.push_str(&format!(" => {ty}"));
    }
    sig
}

fn span_text(text: &str, span: Span) -> &str {
    &text[span.start as usize..span.end as usize]
}

struct SpanMark {
    start: u32,
    end: u32,
    token: MarkToken,
}

enum MarkToken {
    /// any identifier — normalized to `$ID`
    Id,
    /// literal kept verbatim; `note` marks string/regex literals worth
    /// surfacing in the cluster report
    Text { text: String, note: bool },
    /// comment — dropped
    Skip,
}

/// Marks every identifier and literal span in the program. Node kinds not
/// visited here fall through to the raw scan in `tokenize`, which keeps their
/// text verbatim — unknown constructs can only make fingerprints MORE
/// specific (false negatives), never collide unrelated bodies.
struct MarkCollector<'s> {
    text: &'s str,
    marks: Vec<SpanMark>,
}

impl MarkCollector<'_> {
    fn mark(&mut self, span: Span, token: MarkToken) {
        self.marks.push(SpanMark {
            start: span.start,
            end: span.end,
            token,
        });
    }
}

impl<'a> Visit<'a> for MarkCollector<'_> {
    fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
        self.mark(it.span, MarkToken::Id);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        self.mark(it.span, MarkToken::Id);
    }

    fn visit_identifier_name(&mut self, it: &IdentifierName<'a>) {
        self.mark(it.span, MarkToken::Id);
    }

    fn visit_private_identifier(&mut self, it: &PrivateIdentifier<'a>) {
        self.mark(it.span, MarkToken::Id);
    }

    fn visit_string_literal(&mut self, it: &StringLiteral<'a>) {
        // canonical quotes so 'x' and "x" compare equal
        self.mark(
            it.span,
            MarkToken::Text {
                text: format!("\"{}\"", it.value),
                note: true,
            },
        );
    }

    fn visit_reg_exp_literal(&mut self, it: &RegExpLiteral<'a>) {
        self.mark(
            it.span,
            MarkToken::Text {
                text: span_text(self.text, it.span).to_string(),
                note: true,
            },
        );
    }

    fn visit_template_element(&mut self, it: &TemplateElement<'a>) {
        self.mark(
            it.span,
            MarkToken::Text {
                text: it.value.raw.to_string(),
                note: false,
            },
        );
    }

    fn visit_numeric_literal(&mut self, it: &NumericLiteral<'a>) {
        self.mark(
            it.span,
            MarkToken::Text {
                text: span_text(self.text, it.span).to_string(),
                note: false,
            },
        );
    }

    fn visit_big_int_literal(&mut self, it: &BigIntLiteral<'a>) {
        self.mark(
            it.span,
            MarkToken::Text {
                text: span_text(self.text, it.span).to_string(),
                note: false,
            },
        );
    }
}

/// Normalized token sequence of `text[start..end]` given the sorted marks:
/// identifier spans → `$ID`, literal spans → verbatim text, comment spans →
/// dropped, whitespace dropped, remaining alphanumeric runs (keywords) kept
/// as one token each, remaining punctuation one token per char.
fn tokenize(text: &str, start: u32, end: u32, marks: &[SpanMark]) -> (Vec<String>, Vec<String>) {
    let mut tokens = Vec::new();
    let mut literals = Vec::new();
    let mut next_mark = marks.partition_point(|m| m.start < start);
    let end = end as usize;
    let mut cursor = start as usize;
    while cursor < end {
        if let Some(mark) = marks.get(next_mark)
            && mark.start as usize == cursor
        {
            match &mark.token {
                MarkToken::Id => tokens.push("$ID".to_string()),
                MarkToken::Text { text, note } => {
                    if !text.is_empty() {
                        tokens.push(text.clone());
                    }
                    if *note {
                        literals.push(text.clone());
                    }
                }
                MarkToken::Skip => {}
            }
            cursor = mark.end as usize;
            next_mark += 1;
            continue;
        }
        // a mark starting before the cursor was swallowed by an outer span —
        // skip it so the position check above stays aligned
        if marks
            .get(next_mark)
            .is_some_and(|m| (m.start as usize) < cursor)
        {
            next_mark += 1;
            continue;
        }
        let run_stop = marks
            .get(next_mark)
            .map_or(end, |m| (m.start as usize).min(end));
        let c = text[cursor..].chars().next().unwrap_or(' ');
        if c.is_whitespace() {
            cursor += c.len_utf8();
        } else if c.is_alphanumeric() || c == '_' || c == '$' {
            let mut run_end = cursor;
            for ch in text[cursor..run_stop].chars() {
                if ch.is_alphanumeric() || ch == '_' || ch == '$' {
                    run_end += ch.len_utf8();
                } else {
                    break;
                }
            }
            tokens.push(text[cursor..run_end].to_string());
            cursor = run_end;
        } else {
            tokens.push(c.to_string());
            cursor += c.len_utf8();
        }
    }
    (tokens, literals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ts(name: &str) -> PathBuf {
        PathBuf::from(format!("/fixtures/{name}.ts"))
    }

    /// The motivating pair: same body, different declaration form, different
    /// function and param names. MUST hash equal with literals kept verbatim.
    const CLEAN_PAIR: &str = r#"
export const isEmail = (email: string) => {
  const re = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
  return re.test(String(email).toLowerCase())
}
export function validateEmail(value: string) {
  const re = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
  return re.test(String(value).toLowerCase())
}
"#;

    #[test]
    fn arrow_and_function_with_identical_bodies_share_a_fingerprint() {
        let cands = file_candidates(&ts("pair"), CLEAN_PAIR).unwrap();
        assert_eq!(cands.len(), 2, "both declaration forms are candidates");
        let is_email = cands.iter().find(|c| c.name == "isEmail").unwrap();
        let validate = cands.iter().find(|c| c.name == "validateEmail").unwrap();
        assert_eq!(
            is_email.fingerprint, validate.fingerprint,
            "identifier names must be normalized away"
        );
        assert!(is_email.exported && validate.exported);
        assert_eq!(is_email.kind, CandidateKind::Const);
        assert_eq!(validate.kind, CandidateKind::Fn);
        assert_eq!(is_email.line, 2);
        assert_eq!(validate.line, 6);
    }

    #[test]
    fn the_email_validator_body_clears_the_default_floor() {
        let cands = file_candidates(&ts("floor-keep"), CLEAN_PAIR).unwrap();
        assert_eq!(cands.len(), 2, "floor test must not pass vacuously");
        for c in &cands {
            assert!(
                c.token_count >= DEFAULT_MIN_TOKENS,
                "{} has {} tokens, must clear the floor of {DEFAULT_MIN_TOKENS}",
                c.name,
                c.token_count
            );
        }
    }

    #[test]
    fn trivial_passthroughs_fall_below_the_default_floor() {
        let src = "export const identity = (x: unknown) => !!x\nexport const yes = () => true\n";
        let cands = file_candidates(&ts("floor-drop"), src).unwrap();
        assert_eq!(cands.len(), 2);
        for c in &cands {
            assert!(
                c.token_count < DEFAULT_MIN_TOKENS,
                "{} has {} tokens, must fall below the floor",
                c.name,
                c.token_count
            );
        }
    }

    #[test]
    fn different_regex_literals_produce_different_fingerprints() {
        let src = r#"
export const isEmail = (v: string) => {
  const re = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
  return re.test(String(v).toLowerCase())
}
export const isPhone = (v: string) => {
  const re = /^\+?[0-9]{7,15}$/
  return re.test(String(v).toLowerCase())
}
"#;
        let cands = file_candidates(&ts("regex"), src).unwrap();
        let email = cands.iter().find(|c| c.name == "isEmail").unwrap();
        let phone = cands.iter().find(|c| c.name == "isPhone").unwrap();
        assert_ne!(
            email.fingerprint, phone.fingerprint,
            "regex literals are the discriminating signal and must be kept"
        );
    }

    #[test]
    fn string_literals_discriminate_but_quote_style_does_not() {
        let a =
            file_candidates(&ts("qa"), "const f = (a: string) => a.replace('x', 'y')\n").unwrap();
        let b = file_candidates(
            &ts("qb"),
            "const g = (b: string) => b.replace(\"x\", \"y\")\n",
        )
        .unwrap();
        let c =
            file_candidates(&ts("qc"), "const h = (c: string) => c.replace('x', 'z')\n").unwrap();
        assert_eq!(
            a[0].fingerprint, b[0].fingerprint,
            "quote style is not a semantic difference"
        );
        assert_ne!(
            a[0].fingerprint, c[0].fingerprint,
            "different string contents must not collide"
        );
    }

    #[test]
    fn comments_and_whitespace_do_not_affect_the_fingerprint() {
        let plain = "const f = (a: number) => {\n  return a * 2\n}\n";
        let noisy =
            "const g = (b: number) => {\n  // double it\n  return b   * /* inline */ 2\n}\n";
        let a = file_candidates(&ts("plain"), plain).unwrap();
        let b = file_candidates(&ts("noisy"), noisy).unwrap();
        assert_eq!(a[0].fingerprint, b[0].fingerprint);
    }

    #[test]
    fn non_exported_top_level_functions_are_candidates_marked_unexported() {
        let src = "function collapse(s: string) { return s.replace(/\\s+/g, ' ').trim() }\nconst local = (n: number) => { return n.toFixed(2).padStart(8, '0') }\n";
        let cands = file_candidates(&ts("plainfns"), src).unwrap();
        assert_eq!(cands.len(), 2);
        assert!(cands.iter().all(|c| !c.exported));
    }

    #[test]
    fn named_default_export_function_is_a_candidate() {
        let src = "export default function widget(input: string) { return input.split(',').map(Number).filter(Boolean) }\n";
        let cands = file_candidates(&ts("defexp"), src).unwrap();
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].name, "widget");
        assert!(cands[0].exported);
    }

    #[test]
    fn class_methods_object_methods_and_nested_closures_are_skipped() {
        let src = r#"
export class Store {
  load(key: string) { return this.cache.get(key.trim().toLowerCase()) }
}
export const handlers = {
  onSave(data: string) { return data.trim().toLowerCase().padEnd(10) }
}
export function outer(items: string[]) {
  const inner = (s: string) => s.trim().toLowerCase().padEnd(10)
  return items.map(inner)
}
"#;
        let cands = file_candidates(&ts("nested"), src).unwrap();
        let names: Vec<&str> = cands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["outer"], "only the top-level function counts");
    }

    #[test]
    fn substring_names_do_not_leak_between_candidates() {
        let src = r#"
export const isEmailFlow = (flow: string) => flow.startsWith('email:')
"#;
        let cands = file_candidates(&ts("flow"), src).unwrap();
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].name, "isEmailFlow");
    }

    #[test]
    fn sig_records_params_and_return_type_literals_record_body_literals() {
        let cands = file_candidates(&ts("sig"), CLEAN_PAIR).unwrap();
        let is_email = cands.iter().find(|c| c.name == "isEmail").unwrap();
        assert_eq!(is_email.sig, "(email: string)");
        assert!(
            is_email
                .literals
                .iter()
                .any(|l| l.contains(r"^[^\s@]+@[^\s@]+\.[^\s@]+$")),
            "body regex must be recorded for the cluster note: {:?}",
            is_email.literals
        );
        let src = "export function toCount(input: string): number { return input.split(',').map(Number).length }\n";
        let typed = file_candidates(&ts("sig2"), src).unwrap();
        assert_eq!(typed[0].sig, "(input: string) => number");
    }

    #[test]
    fn broken_file_is_an_error_not_a_panic() {
        assert!(file_candidates(&ts("broken"), "import { from 'nope").is_err());
    }
}
