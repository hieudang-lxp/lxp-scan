use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, Theme};
use syntect::parsing::SyntaxSet;
use syntect::util::{LinesWithEndings, as_24_bit_terminal_escaped};

/// One source line as (style, text) runs; text excludes the trailing newline.
pub type StyledLine = Vec<(Style, String)>;

/// Stock syntect has no TSX grammar; two-face bundles bat's syntax set.
fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme() -> &'static Theme {
    static SET: OnceLock<two_face::theme::EmbeddedLazyThemeSet> = OnceLock::new();
    SET.get_or_init(two_face::theme::extra)
        .get(two_face::theme::EmbeddedThemeName::MonokaiExtended)
}

/// Highlights `code` as TSX into per-line styled runs (for the TUI renderer).
/// Highlighting failure of a line degrades to an unstyled run — never an error.
pub fn highlight_lines(code: &str) -> Vec<StyledLine> {
    let set = syntax_set();
    let syntax = set
        .find_syntax_by_extension("tsx")
        .unwrap_or_else(|| set.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme());
    code.lines()
        .map(|line| {
            // highlight_line needs the newline back for correct state tracking
            let with_newline = format!("{line}\n");
            match highlighter.highlight_line(&with_newline, set) {
                Ok(runs) => runs
                    .into_iter()
                    .map(|(style, text)| (style, text.trim_end_matches('\n').to_string()))
                    .filter(|(_, text)| !text.is_empty())
                    .collect(),
                Err(_) => vec![(Style::default(), line.to_string())],
            }
        })
        .collect()
}

/// TSX-highlighted copy of `code` using 24-bit ANSI escapes, for TTY output.
pub fn highlight_ansi(code: &str) -> String {
    let set = syntax_set();
    let syntax = set
        .find_syntax_by_extension("tsx")
        .unwrap_or_else(|| set.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme());
    let mut out = String::new();
    for line in LinesWithEndings::from(code) {
        match highlighter.highlight_line(line, set) {
            Ok(runs) => out.push_str(&as_24_bit_terminal_escaped(&runs, false)),
            Err(_) => out.push_str(line),
        }
    }
    out.push_str("\x1b[0m");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "const Button = (props: ButtonProps) => {\n  return <div className=\"btn\" />;\n};";

    #[test]
    fn ansi_output_is_colored_and_keeps_the_source_text() {
        let out = highlight_ansi(SRC);
        assert!(out.contains("\x1b["), "expected ANSI escapes");
        assert!(out.ends_with("\x1b[0m"), "must reset at the end");
        // stripping escapes yields the original text
        let stripped: String = strip_ansi(&out);
        assert_eq!(stripped.trim_end(), SRC);
    }

    #[test]
    fn styled_lines_reassemble_to_the_source() {
        let lines = highlight_lines(SRC);
        assert_eq!(lines.len(), 3);
        let rebuilt: Vec<String> = lines
            .iter()
            .map(|runs| runs.iter().map(|(_, t)| t.as_str()).collect())
            .collect();
        assert_eq!(rebuilt.join("\n"), SRC);
    }

    #[test]
    fn empty_input_yields_no_lines() {
        assert!(highlight_lines("").is_empty());
    }

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for e in chars.by_ref() {
                    if e == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
