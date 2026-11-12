//! Rendering: the human report, the effective-set view, and the JSON document.
//!
//! Every renderer is a pure function of its input and wraps at a fixed width
//! rather than the terminal width, so identical input produces identical bytes
//! on every machine and in every CI run.

use crate::agents::Effective;
use crate::model::{Report, Severity, CAVEAT, SOURCES_READ};

/// Fixed wrap width. Not terminal-derived, on purpose.
const WRAP: usize = 92;

/// Column width the check id is padded to in the human report.
const ID_WIDTH: usize = 27;

/// Rendering options for the human output.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    /// Emit ANSI color codes.
    pub color: bool,
}

fn paint(opts: RenderOptions, code: &str, s: &str) -> String {
    if opts.color {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

/// Render the full human-readable report, including the trailing caveat.
pub fn render_text(report: &Report, opts: RenderOptions) -> String {
    let mut out = String::new();

    out.push_str(&format!("agentsdoc: {}\n", report.root_name));
    out.push_str(&summary_line(report, opts));
    out.push_str("\n\n");

    for cat in &report.categories {
        let score = match cat.score {
            Some(s) => format!(" (score {s})"),
            None => String::new(),
        };
        out.push_str(&format!(
            "{}{}\n",
            paint(opts, "1", cat.category.title()),
            score
        ));

        for f in report
            .findings
            .iter()
            .filter(|f| f.category == cat.category)
        {
            let label = paint(opts, severity_color(f.severity), f.severity.label());
            out.push_str(&format!(
                "  [{}] {:<width$} {}\n",
                label,
                f.id,
                f.title,
                width = ID_WIDTH
            ));
            for e in &f.evidence {
                out.push_str(&format!("         - {e}\n"));
            }
            // PASS lines stay terse; anything actionable carries its rationale
            // and, crucially, the source that justifies the severity.
            if f.severity != Severity::Pass {
                push_field(&mut out, "why", f.why);
                push_field(&mut out, "source", f.source);
                if f.severity != Severity::Info {
                    push_field(&mut out, "fix", f.fix);
                }
            }
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "{} check(s) run, {} skipped. {} instruction file(s), ~{} tokens est. in total.\n",
        report.findings.len(),
        report.skipped,
        report.files.len(),
        report.total_est_tokens(),
    ));
    for line in wrap(CAVEAT, WRAP) {
        out.push_str(&paint(opts, "2", &line));
        out.push('\n');
    }
    out
}

/// The one-line summary used as the `--quiet` output and as the report header.
pub fn summary_line(report: &Report, opts: RenderOptions) -> String {
    let score = match report.score {
        Some(s) => format!("score {s}/100"),
        None => "score n/a".to_string(),
    };
    format!(
        "grade {}  {}  |  pass {}  warn {}  fail {}  info {}",
        paint(opts, grade_color(report.grade()), report.grade()),
        score,
        report.counts.pass,
        report.counts.warn,
        report.counts.fail,
        report.counts.info,
    )
}

/// Render the `--effective` view: what each agent assembles for one directory.
pub fn render_effective_text(
    root_name: &str,
    dir: &str,
    sets: &[Effective],
    opts: RenderOptions,
) -> String {
    let shown = if dir.is_empty() { "." } else { dir };
    let mut out = format!("agentsdoc: {root_name}  effective instruction set for {shown}\n\n");

    for eff in sets {
        let flag = if eff.verified { "" } else { "  (unverified)" };
        out.push_str(&format!(
            "{}{flag}\n",
            paint(opts, "1", &format!("{} [{}]", eff.name, eff.agent))
        ));
        if eff.layers.is_empty() {
            out.push_str("  (loads nothing from this repository)\n");
        }
        for (i, layer) in eff.layers.iter().enumerate() {
            let marker = if layer.conditional { "?" } else { " " };
            out.push_str(&format!(
                "  {}{marker} {:<44} ~{:>6} tokens est.  {}\n",
                i + 1,
                layer.path,
                layer.est_tokens,
                layer.note
            ));
        }
        out.push_str(&format!(
            "     total (unconditional): ~{} tokens est.\n",
            eff.est_tokens
        ));
        for note in &eff.notes {
            for line in wrap(note, WRAP.saturating_sub(7)) {
                out.push_str(&format!("     {line}\n"));
            }
        }
        out.push('\n');
    }

    out.push_str("Order is load order: earlier entries are read first. A `?` marks a file\n");
    out.push_str("that only loads under a condition agentsdoc cannot see from the repository\n");
    out.push_str(
        "(frontmatter globs, applyTo patterns, on-demand subdirectory loads); those are\n",
    );
    out.push_str("those are excluded from the total.\n");
    for line in wrap(CAVEAT, WRAP) {
        out.push_str(&paint(opts, "2", &line));
        out.push('\n');
    }
    out
}

fn push_field(out: &mut String, name: &str, text: &str) {
    let indent = "         ";
    let head = format!("{indent}{name:<7}");
    let body_indent = format!("{indent}{}", " ".repeat(7));
    for (i, line) in wrap(text, WRAP.saturating_sub(head.len()))
        .into_iter()
        .enumerate()
    {
        if i == 0 {
            out.push_str(&head);
        } else {
            out.push_str(&body_indent);
        }
        out.push_str(&line);
        out.push('\n');
    }
}

fn severity_color(s: Severity) -> &'static str {
    match s {
        Severity::Pass => "32",
        Severity::Info => "36",
        Severity::Warn => "33",
        Severity::Fail => "31",
    }
}

fn grade_color(grade: &str) -> &'static str {
    match grade {
        "A" | "B" => "32",
        "C" => "33",
        "n/a" => "36",
        _ => "31",
    }
}

/// Greedy word wrap. Words longer than `width` are emitted on their own line
/// rather than being split, which keeps URLs intact and clickable.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(20);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Render the machine-readable report.
///
/// JSON is hand-rolled rather than pulled in via serde: the document has a
/// fixed shape with no user-controlled keys, and skipping serde keeps the
/// dependency tree at three direct crates.
pub fn render_json(report: &Report) -> String {
    let mut out = String::with_capacity(8192);
    out.push_str("{\n");
    out.push_str("  \"tool\": \"agentsdoc\",\n");
    out.push_str(&format!(
        "  \"version\": \"{}\",\n",
        env!("CARGO_PKG_VERSION")
    ));
    out.push_str(&format!("  \"sources_read\": {},\n", quote(SOURCES_READ)));
    out.push_str(&format!(
        "  \"repository\": {},\n",
        quote(&report.root_name)
    ));
    out.push_str(&format!("  \"grade\": {},\n", quote(report.grade())));
    out.push_str(&format!("  \"score\": {},\n", num(report.score)));
    out.push_str(&format!(
        "  \"counts\": {{\"pass\": {}, \"warn\": {}, \"fail\": {}, \"info\": {}}},\n",
        report.counts.pass, report.counts.warn, report.counts.fail, report.counts.info
    ));
    out.push_str(&format!("  \"skipped\": {},\n", report.skipped));
    out.push_str(&format!(
        "  \"total_est_tokens\": {},\n",
        report.total_est_tokens()
    ));

    out.push_str("  \"files\": [\n");
    for (i, f) in report.files.iter().enumerate() {
        out.push_str(&format!(
            "    {{\"path\": {}, \"kind\": {}, \"bytes\": {}, \"lines\": {}, \"est_tokens\": {}}}",
            quote(&f.path),
            quote(f.kind),
            f.bytes,
            f.lines,
            f.est_tokens
        ));
        out.push_str(sep(i, report.files.len()));
    }
    out.push_str("  ],\n");

    out.push_str("  \"agents\": [\n");
    for (i, a) in report.agents.iter().enumerate() {
        let files: Vec<String> = a.files.iter().map(|p| quote(p)).collect();
        out.push_str(&format!(
            "    {{\"agent\": {}, \"name\": {}, \"verified\": {}, \"loads\": [{}]}}",
            quote(a.agent),
            quote(a.name),
            a.verified,
            files.join(", ")
        ));
        out.push_str(sep(i, report.agents.len()));
    }
    out.push_str("  ],\n");

    out.push_str("  \"categories\": [\n");
    for (i, cat) in report.categories.iter().enumerate() {
        out.push_str(&format!(
            "    {{\"id\": {}, \"title\": {}, \"score\": {}, \
\"counts\": {{\"pass\": {}, \"warn\": {}, \"fail\": {}, \"info\": {}}}}}",
            quote(cat.category.as_str()),
            quote(cat.category.title()),
            num(cat.score),
            cat.counts.pass,
            cat.counts.warn,
            cat.counts.fail,
            cat.counts.info
        ));
        out.push_str(sep(i, report.categories.len()));
    }
    out.push_str("  ],\n");

    out.push_str("  \"findings\": [\n");
    for (i, f) in report.findings.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"id\": {},\n", quote(f.id)));
        out.push_str(&format!(
            "      \"category\": {},\n",
            quote(f.category.as_str())
        ));
        out.push_str(&format!(
            "      \"severity\": {},\n",
            quote(f.severity.as_str())
        ));
        out.push_str(&format!("      \"title\": {},\n", quote(&f.title)));
        let evidence: Vec<String> = f.evidence.iter().map(|e| quote(e)).collect();
        out.push_str(&format!("      \"evidence\": [{}],\n", evidence.join(", ")));
        out.push_str(&format!("      \"why\": {},\n", quote(f.why)));
        out.push_str(&format!("      \"source\": {},\n", quote(f.source)));
        out.push_str(&format!("      \"fix\": {}\n", quote(f.fix)));
        out.push_str("    }");
        out.push_str(sep(i, report.findings.len()));
    }
    out.push_str("  ],\n");

    out.push_str(&format!("  \"caveat\": {}\n", quote(CAVEAT)));
    out.push_str("}\n");
    out
}

/// Render the `--effective` view as JSON.
pub fn render_effective_json(root_name: &str, dir: &str, sets: &[Effective]) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str("{\n");
    out.push_str("  \"tool\": \"agentsdoc\",\n");
    out.push_str(&format!(
        "  \"version\": \"{}\",\n",
        env!("CARGO_PKG_VERSION")
    ));
    out.push_str(&format!("  \"sources_read\": {},\n", quote(SOURCES_READ)));
    out.push_str(&format!("  \"repository\": {},\n", quote(root_name)));
    out.push_str(&format!("  \"directory\": {},\n", quote(dir)));
    out.push_str("  \"effective\": [\n");
    for (i, eff) in sets.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"agent\": {},\n", quote(eff.agent)));
        out.push_str(&format!("      \"name\": {},\n", quote(eff.name)));
        out.push_str(&format!("      \"verified\": {},\n", eff.verified));
        out.push_str(&format!("      \"est_tokens\": {},\n", eff.est_tokens));
        out.push_str("      \"layers\": [\n");
        for (j, l) in eff.layers.iter().enumerate() {
            out.push_str(&format!(
                "        {{\"order\": {}, \"path\": {}, \"est_tokens\": {}, \
\"conditional\": {}, \"note\": {}}}",
                j + 1,
                quote(&l.path),
                l.est_tokens,
                l.conditional,
                quote(&l.note)
            ));
            out.push_str(if j + 1 == eff.layers.len() {
                "\n"
            } else {
                ",\n"
            });
        }
        out.push_str("      ],\n");
        let notes: Vec<String> = eff.notes.iter().map(|n| quote(n)).collect();
        out.push_str(&format!("      \"notes\": [{}]\n", notes.join(", ")));
        out.push_str("    }");
        out.push_str(sep(i, sets.len()));
    }
    out.push_str("  ],\n");
    out.push_str(&format!("  \"caveat\": {}\n", quote(CAVEAT)));
    out.push_str("}\n");
    out
}

fn sep(i: usize, len: usize) -> &'static str {
    if i + 1 == len {
        "\n"
    } else {
        ",\n"
    }
}

fn num(v: Option<u32>) -> String {
    match v {
        Some(n) => n.to_string(),
        None => "null".to_string(),
    }
}

/// Quote and escape a string as a JSON scalar.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::effective_all;
    use crate::checks::{audit, Options};
    use crate::scan::Workspace;

    fn sample() -> Workspace {
        Workspace::from_entries(
            "sample",
            [(
                "AGENTS.md",
                "# Sample\n\n## Build\nRun `cargo test`.\n\n## Style\nUse rustfmt.\n",
            )],
        )
    }

    #[test]
    fn text_report_is_deterministic_and_carries_the_caveat() {
        let r = audit(&sample(), &Options::default());
        let a = render_text(&r, RenderOptions { color: false });
        let b = render_text(&r, RenderOptions { color: false });
        assert_eq!(a, b);
        assert!(a.contains("Agent loading behavior changes fast"));
        assert!(a.contains("portability-claude-code"));
    }

    #[test]
    fn actionable_findings_always_show_their_source() {
        let r = audit(&sample(), &Options::default());
        let text = render_text(&r, RenderOptions { color: false });
        assert!(text.contains("https://code.claude.com/docs/en/memory"));
        assert!(text.contains("source"));
    }

    #[test]
    fn no_color_output_has_no_escape_codes() {
        let r = audit(&sample(), &Options::default());
        let plain = render_text(&r, RenderOptions { color: false });
        assert!(!plain.contains('\x1b'));
        let colored = render_text(&r, RenderOptions { color: true });
        assert!(colored.contains('\x1b'));
    }

    #[test]
    fn json_is_stable_and_escaped() {
        let r = audit(&sample(), &Options::default());
        let a = render_json(&r);
        assert_eq!(a, render_json(&r));
        assert!(a.starts_with("{\n  \"tool\": \"agentsdoc\","));
        assert!(a.contains("\"caveat\":"));
        assert!(a.contains("\"sources_read\": \"2026-11-12\""));
        assert_eq!(a.matches('"').count() % 2, 0);
    }

    #[test]
    fn quote_escapes_control_characters() {
        assert_eq!(quote("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
        assert_eq!(quote("\u{1}"), "\"\\u0001\"");
    }

    #[test]
    fn effective_rendering_is_stable_in_both_modes() {
        let ws = sample();
        let sets = effective_all(&ws, "");
        let text = render_effective_text("sample", "", &sets, RenderOptions { color: false });
        assert_eq!(
            text,
            render_effective_text("sample", "", &sets, RenderOptions { color: false })
        );
        assert!(text.contains("loads nothing from this repository"));

        let json = render_effective_json("sample", "", &sets);
        assert_eq!(json, render_effective_json("sample", "", &sets));
        assert!(json.contains("\"agent\": \"claude-code\""));
        assert_eq!(json.matches('"').count() % 2, 0);
    }

    #[test]
    fn wrap_keeps_long_urls_intact() {
        let long = "https://example.com/".to_string() + &"a".repeat(200);
        let lines = wrap(&format!("see {long} now"), 40);
        assert!(lines.iter().any(|l| l == &long));
    }
}
