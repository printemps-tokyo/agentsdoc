//! Dependency-free text analysis shared by the checks.
//!
//! Everything here is line-based on purpose. A Markdown parser crate would buy
//! very little: agentsdoc never renders Markdown, it only needs headings,
//! fenced-code boundaries, inline links and `@path` imports, all of which are
//! unambiguous at the line level. Staying parser-free keeps the dependency tree
//! at three direct crates, which matters for a tool whose whole subject is
//! keeping things small and legible.

use std::collections::BTreeMap;

/// Divisor used by the token estimate.
///
/// Four characters per token is the rule of thumb published for English prose
/// in OpenAI-family BPE tokenizers. It is an *estimate* and is labeled as one
/// everywhere it surfaces; the real number depends on the tokenizer your agent
/// uses, on code-vs-prose, and on non-ASCII text (CJK in particular runs far
/// denser than 4 chars/token). agentsdoc never claims otherwise.
pub const CHARS_PER_TOKEN: usize = 4;

/// Estimated token count of `text`: `ceil(chars / 4)`.
///
/// Counts `char`s, not bytes, so a multi-byte document is not double-counted.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(CHARS_PER_TOKEN)
}

/// Truncate a line for use as report evidence, appending an ellipsis marker.
pub fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("...");
    out
}

/// Walk a document's lines, reporting for each whether it sits inside a fenced
/// code block. The fence line itself is reported as fenced.
///
/// Both ``` and ~~~ fences are recognized. Info strings are ignored; a closing
/// fence is any line whose first non-space run is at least as long as the
/// opening one and uses the same character.
fn fenced_flags(text: &str) -> Vec<bool> {
    let mut flags = Vec::new();
    let mut open: Option<(char, usize)> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let fence = fence_run(trimmed);
        match (open, fence) {
            (None, Some((ch, len))) => {
                open = Some((ch, len));
                flags.push(true);
            }
            (Some((open_ch, open_len)), Some((ch, len)))
                if ch == open_ch && len >= open_len && trimmed[len..].trim().is_empty() =>
            {
                open = None;
                flags.push(true);
            }
            (state, _) => flags.push(state.is_some()),
        }
    }
    flags
}

/// Length of a leading ``` / ~~~ run, if the line opens or closes a fence.
fn fence_run(trimmed: &str) -> Option<(char, usize)> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|c| *c == first).count();
    if len >= 3 {
        Some((first, len))
    } else {
        None
    }
}

/// Replace the contents of inline code spans with spaces, preserving the byte
/// length of the line so column offsets stay meaningful.
///
/// Claude Code documents exactly this behavior for imports: "Import parsing
/// skips Markdown code spans and fenced code blocks. To mention a path in your
/// CLAUDE.md without importing it, wrap it in backticks."
fn mask_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_span = false;
    for c in line.chars() {
        if c == '`' {
            in_span = !in_span;
            out.push(' ');
        } else if in_span {
            // Preserve width without preserving content.
            for _ in 0..c.len_utf8() {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Every ATX heading (`# ...`) in the document, lowercased and trimmed, with
/// the leading `#`s removed. Headings inside fenced blocks are skipped.
pub fn headings(text: &str) -> Vec<String> {
    let fenced = fenced_flags(text);
    text.lines()
        .enumerate()
        .filter(|(i, _)| !fenced.get(*i).copied().unwrap_or(false))
        .filter_map(|(_, line)| {
            let t = line.trim_start();
            if !t.starts_with('#') {
                return None;
            }
            let title = t.trim_start_matches('#').trim();
            if title.is_empty() {
                None
            } else {
                Some(title.to_ascii_lowercase())
            }
        })
        .collect()
}

/// An `@path` import found in a memory file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// 1-based line number.
    pub line: usize,
    /// The path exactly as written, without the leading `@`.
    pub path: String,
}

/// Find `@path` imports, using Claude Code's documented parsing rules: code
/// spans and fenced code blocks are skipped.
///
/// Source: <https://code.claude.com/docs/en/memory> (read 2026-07-29) --
/// "CLAUDE.md files can import additional files using @path/to/import syntax"
/// and "Import parsing skips Markdown code spans and fenced code blocks."
pub fn imports(text: &str) -> Vec<Import> {
    let fenced = fenced_flags(text);
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        if fenced.get(i).copied().unwrap_or(false) {
            continue;
        }
        let line = mask_code_spans(raw);
        let bytes = line.as_bytes();
        let mut idx = 0;
        while idx < bytes.len() {
            if bytes[idx] != b'@' {
                idx += 1;
                continue;
            }
            // An `@` glued to the end of a word is an email or a handle in
            // prose, not an import.
            let prev_ok = idx == 0 || matches!(bytes[idx - 1], b' ' | b'\t' | b'(' | b'[');
            if !prev_ok {
                idx += 1;
                continue;
            }
            let start = idx + 1;
            let mut end = start;
            while end < bytes.len() && is_path_byte(bytes[end]) {
                end += 1;
            }
            // Trailing sentence punctuation is not part of the path.
            while end > start && matches!(bytes[end - 1], b'.' | b',' | b';' | b':' | b')') {
                end -= 1;
            }
            if end > start {
                let path = line[start..end].to_string();
                // A bare `@word` with no path-ish shape is still a valid import
                // in Claude Code (`@README` imports README), so no extension is
                // required; only obvious non-paths are dropped.
                if !path.starts_with('@') {
                    out.push(Import { line: i + 1, path });
                }
            }
            idx = end.max(idx + 1);
        }
    }
    out
}

fn is_path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'/' | b'_' | b'-' | b'~' | b'+')
}

/// A relative reference to another file found in an instruction file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// 1-based line number.
    pub line: usize,
    /// Target as written, with any `#fragment` or `?query` removed.
    pub target: String,
}

/// Inline Markdown links `[text](target)` whose target is a repository-relative
/// path. Absolute URLs, protocol-relative URLs, `mailto:`, bare fragments and
/// rooted paths are all skipped, as are links inside fenced code blocks (those
/// are examples, not references).
pub fn relative_links(text: &str) -> Vec<Reference> {
    let fenced = fenced_flags(text);
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        if fenced.get(i).copied().unwrap_or(false) {
            continue;
        }
        let line = mask_code_spans(raw);
        let bytes = line.as_bytes();
        let mut idx = 0;
        while let Some(open) = find_from(bytes, idx, b'[') {
            let Some(close) = find_from(bytes, open + 1, b']') else {
                break;
            };
            if bytes.get(close + 1) != Some(&b'(') {
                idx = close + 1;
                continue;
            }
            let Some(paren) = find_from(bytes, close + 2, b')') else {
                break;
            };
            let target = line[close + 2..paren].trim();
            // Markdown allows a title after the target: `(path "title")`.
            let target = target.split_whitespace().next().unwrap_or("");
            if let Some(t) = normalize_reference(target) {
                out.push(Reference {
                    line: i + 1,
                    target: t,
                });
            }
            idx = paren + 1;
        }
    }
    out
}

fn find_from(bytes: &[u8], from: usize, needle: u8) -> Option<usize> {
    bytes
        .get(from..)?
        .iter()
        .position(|b| *b == needle)
        .map(|p| p + from)
}

/// Strip fragments/queries and reject anything that is not a repository-relative
/// path.
pub fn normalize_reference(target: &str) -> Option<String> {
    let target = target.trim().trim_matches('<').trim_matches('>');
    if target.is_empty() || target.starts_with('#') || target.starts_with('/') {
        return None;
    }
    if target.starts_with("//") || target.contains("://") {
        return None;
    }
    let lower = target.to_ascii_lowercase();
    for scheme in ["mailto:", "tel:", "data:", "javascript:"] {
        if lower.starts_with(scheme) {
            return None;
        }
    }
    // `~/...` is a home-directory path, outside the repository by definition.
    if target.starts_with('~') {
        return None;
    }
    let cut = target
        .find(['#', '?'])
        .map(|i| &target[..i])
        .unwrap_or(target);
    let cut = cut.trim();
    if cut.is_empty() {
        return None;
    }
    Some(cut.to_string())
}

/// A line reduced to the form used for overlap and contradiction comparison.
///
/// Normalization: trim, strip leading heading markers, list bullets and
/// blockquote markers, collapse internal whitespace, lowercase. Lines shorter
/// than [`MIN_SIGNIFICANT_CHARS`] after normalization, and lines inside fenced
/// code blocks, are dropped -- they carry no directive content and would make
/// every pair of Markdown files look similar.
pub const MIN_SIGNIFICANT_CHARS: usize = 16;

/// Normalized, comparable lines of a document.
pub fn significant_lines(text: &str) -> Vec<String> {
    let fenced = fenced_flags(text);
    text.lines()
        .enumerate()
        .filter(|(i, _)| !fenced.get(*i).copied().unwrap_or(false))
        .filter_map(|(_, line)| normalize_line(line))
        .collect()
}

/// Normalize one line, or `None` when it carries no directive content.
pub fn normalize_line(line: &str) -> Option<String> {
    let mut t = line.trim();
    loop {
        let before = t;
        t = t.trim_start_matches('#').trim_start();
        t = t.trim_start_matches('>').trim_start();
        for bullet in ["- ", "* ", "+ "] {
            t = t.strip_prefix(bullet).unwrap_or(t).trim_start();
        }
        // Ordered list markers: `1. `, `12) `.
        let digits = t.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            let rest = &t[digits..];
            if let Some(r) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) {
                if r.starts_with(' ') {
                    t = r.trim_start();
                }
            }
        }
        if t == before {
            break;
        }
    }
    let squeezed = squeeze(&t.to_ascii_lowercase());
    if squeezed.chars().count() < MIN_SIGNIFICANT_CHARS {
        return None;
    }
    Some(squeezed)
}

/// Collapse runs of whitespace to a single space.
fn squeeze(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            in_ws = true;
            continue;
        }
        if in_ws && !out.is_empty() {
            out.push(' ');
        }
        in_ws = false;
        out.push(c);
    }
    out
}

/// Result of comparing two documents' significant lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Overlap {
    /// Number of significant lines present in both documents (multiset
    /// intersection, so a line repeated twice in both counts twice).
    pub shared: usize,
    /// Significant lines in the smaller document.
    pub smaller: usize,
    /// `shared / smaller`, in 0.0..=1.0. `0.0` when either side has none.
    pub ratio: f64,
    /// Up to a handful of shared lines, sorted, for use as evidence.
    pub samples: Vec<String>,
}

/// Number of shared significant lines below which overlap is never reported,
/// regardless of ratio. Two files that share three boilerplate sentences are
/// not "duplicated".
pub const MIN_SHARED_LINES: usize = 5;

/// Fraction of the smaller file that must be shared before duplication is
/// reported.
pub const DUPLICATION_RATIO: f64 = 0.30;

/// Compare two documents.
///
/// The denominator is the *smaller* line count on purpose: the risk agentsdoc
/// is measuring is "one file restates the other and the two will drift", and
/// that risk is fully present when a short CLAUDE.md is a verbatim subset of a
/// long AGENTS.md. Dividing by the union would hide exactly that case.
pub fn overlap(a: &[String], b: &[String]) -> Overlap {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for line in a {
        *counts.entry(line.as_str()).or_insert(0) += 1;
    }
    let mut shared = 0usize;
    let mut samples: Vec<String> = Vec::new();
    for line in b {
        if let Some(n) = counts.get_mut(line.as_str()) {
            if *n > 0 {
                *n -= 1;
                shared += 1;
                if !samples.contains(line) {
                    samples.push(line.clone());
                }
            }
        }
    }
    samples.sort();
    samples.truncate(3);
    let smaller = a.len().min(b.len());
    let ratio = if smaller == 0 {
        0.0
    } else {
        shared as f64 / smaller as f64
    };
    Overlap {
        shared,
        smaller,
        ratio,
        samples,
    }
}

/// A directive line reduced to polarity plus a comparable core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// 1-based line number.
    pub line: usize,
    /// The instruction with its leading polarity marker removed.
    pub core: String,
    /// True when the line was phrased as a prohibition.
    pub negated: bool,
}

/// Shortest core length considered for contradiction detection. Short cores
/// ("use it", "run tests") collide by accident.
const MIN_DIRECTIVE_CORE: usize = 12;

/// Leading phrases that flip a directive's polarity.
const NEGATIVE_PREFIXES: [&str; 8] = [
    "do not ",
    "don't ",
    "dont ",
    "never ",
    "avoid ",
    "no longer ",
    "must not ",
    "should not ",
];

/// Leading phrases that mark an emphatic positive directive.
const POSITIVE_PREFIXES: [&str; 5] = ["always ", "must ", "should ", "please ", "make sure to "];

/// Extract polarity-tagged directives from a document.
///
/// Deliberately narrow: only lines that open with an explicit polarity marker
/// are considered, so ordinary prose never enters the comparison.
pub fn directives(text: &str) -> Vec<Directive> {
    let fenced = fenced_flags(text);
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        if fenced.get(i).copied().unwrap_or(false) {
            continue;
        }
        let Some(norm) = normalize_line(raw) else {
            continue;
        };
        let Some((core, negated)) = strip_polarity(&norm) else {
            continue;
        };
        if core.chars().count() < MIN_DIRECTIVE_CORE {
            continue;
        }
        out.push(Directive {
            line: i + 1,
            core,
            negated,
        });
    }
    out
}

fn strip_polarity(norm: &str) -> Option<(String, bool)> {
    for p in NEGATIVE_PREFIXES {
        if let Some(rest) = norm.strip_prefix(p) {
            return Some((trim_core(rest), true));
        }
    }
    for p in POSITIVE_PREFIXES {
        if let Some(rest) = norm.strip_prefix(p) {
            return Some((trim_core(rest), false));
        }
    }
    None
}

/// Drop trailing punctuation and a leading "to " so "make sure to run x" and
/// "always run x" compare equal.
fn trim_core(rest: &str) -> String {
    let rest = rest.strip_prefix("to ").unwrap_or(rest);
    rest.trim_end_matches(['.', '!', ';', ','])
        .trim()
        .to_string()
}

/// Pairs of directives that state the same instruction with opposite polarity.
pub fn contradictions(a: &[Directive], b: &[Directive]) -> Vec<(Directive, Directive)> {
    let mut out = Vec::new();
    for x in a {
        for y in b {
            if x.negated != y.negated && x.core == y.core {
                out.push((x.clone(), y.clone()));
            }
        }
    }
    out.sort_by(|p, q| {
        p.0.line
            .cmp(&q.0.line)
            .then_with(|| p.1.line.cmp(&q.1.line))
    });
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_estimate_is_ceil_chars_over_four() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens(&"x".repeat(4000)), 1000);
        // Multi-byte text counts chars, not bytes.
        assert_eq!(estimate_tokens("ありがとう"), 2);
    }

    #[test]
    fn fenced_blocks_are_detected() {
        let text = "a\n```\nb\n```\nc\n";
        let flags = fenced_flags(text);
        assert_eq!(flags, vec![false, true, true, true, false]);
    }

    #[test]
    fn tilde_fences_and_info_strings_work() {
        let text = "a\n~~~rust\nlet x = 1;\n~~~\nb\n";
        let flags = fenced_flags(text);
        assert_eq!(flags, vec![false, true, true, true, false]);
    }

    #[test]
    fn imports_skip_code_spans_and_fences() {
        let text = "See @AGENTS.md for details.\n\
                    Mentioning `@README` should not import.\n\
                    ```\n@fenced.md\n```\n\
                    - git workflow @docs/git-instructions.md\n";
        let found = imports(text);
        let paths: Vec<&str> = found.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["AGENTS.md", "docs/git-instructions.md"]);
        assert_eq!(found[0].line, 1);
        assert_eq!(found[1].line, 6);
    }

    #[test]
    fn emails_and_handles_are_not_imports() {
        assert!(imports("mail develop@printemps.tokyo").is_empty());
        assert!(imports("thanks to alice@github").is_empty());
    }

    #[test]
    fn relative_links_are_extracted_and_urls_skipped() {
        let text = "[a](docs/a.md) [b](https://example.com) [c](#anchor) [d](/abs.md)\n\
                    [e](sub/dir/e.md#section)\n\
                    ```\n[f](fenced.md)\n```\n";
        let refs = relative_links(text);
        let targets: Vec<&str> = refs.iter().map(|r| r.target.as_str()).collect();
        assert_eq!(targets, ["docs/a.md", "sub/dir/e.md"]);
        assert_eq!(refs[1].line, 2);
    }

    #[test]
    fn normalize_line_strips_markup_and_drops_noise() {
        assert_eq!(
            normalize_line("  - Run `cargo test` before committing changes"),
            Some("run `cargo test` before committing changes".to_string())
        );
        assert_eq!(
            normalize_line("## Build   and    test   commands"),
            Some("build and test commands".to_string())
        );
        // Below MIN_SIGNIFICANT_CHARS: too short to be a distinguishing line.
        assert_eq!(normalize_line("## Build and test"), None);
        assert_eq!(normalize_line("# Setup"), None);
        assert_eq!(normalize_line(""), None);
        assert_eq!(
            normalize_line("1. Install the toolchain first"),
            Some("install the toolchain first".to_string())
        );
    }

    #[test]
    fn overlap_divides_by_the_smaller_document() {
        let a: Vec<String> = (0..10)
            .map(|i| format!("shared directive line {i}"))
            .collect();
        let b: Vec<String> = a.iter().take(5).cloned().collect();
        let o = overlap(&a, &b);
        assert_eq!(o.shared, 5);
        assert_eq!(o.smaller, 5);
        assert!((o.ratio - 1.0).abs() < f64::EPSILON);
        assert_eq!(o.samples.len(), 3);
    }

    #[test]
    fn overlap_is_zero_for_disjoint_documents() {
        let a = vec!["first unique directive".to_string()];
        let b = vec!["second unique directive".to_string()];
        let o = overlap(&a, &b);
        assert_eq!(o.shared, 0);
        assert_eq!(o.ratio, 0.0);
    }

    #[test]
    fn overlap_handles_an_empty_side() {
        let o = overlap(&[], &["only one directive here".to_string()]);
        assert_eq!(o.shared, 0);
        assert_eq!(o.smaller, 0);
        assert_eq!(o.ratio, 0.0);
    }

    #[test]
    fn significant_lines_ignore_code_blocks() {
        let text = "Always run the full test suite\n```\nsome very long code line here\n```\n";
        assert_eq!(
            significant_lines(text),
            vec!["always run the full test suite".to_string()]
        );
    }

    #[test]
    fn directives_carry_polarity() {
        let ds = directives("- Always use two-space indentation\n- Never use tabs for indentation\n- ordinary prose line here\n");
        assert_eq!(ds.len(), 2);
        assert_eq!(ds[0].core, "use two-space indentation");
        assert!(!ds[0].negated);
        assert!(ds[1].negated);
    }

    #[test]
    fn contradictions_need_matching_cores() {
        let a = directives("Always use two-space indentation\n");
        let b = directives("Never use two-space indentation\n");
        let c = directives("Never use four-space indentation\n");
        assert_eq!(contradictions(&a, &b).len(), 1);
        assert!(contradictions(&a, &c).is_empty());
    }

    #[test]
    fn headings_are_lowercased_and_skip_fences() {
        let text = "# Build and Test\n```\n# not a heading\n```\n## Code Style\n";
        assert_eq!(headings(text), ["build and test", "code style"]);
    }

    #[test]
    fn clip_shortens_long_lines() {
        assert_eq!(clip("  hello  ", 20), "hello");
        assert_eq!(clip("abcdefghij", 5), "abcde...");
    }
}
