//! End-to-end audits over fixture repositories built in temporary directories.
//!
//! These cover the walk (gitignore, hidden files, symlinks) as well as the
//! check engine, which the in-module unit tests exercise from in-memory
//! workspaces. Everything here is offline.

use std::fs;
use std::path::Path;

use agentsdoc::agents::effective_all;
use agentsdoc::checks::{audit, Options, DEFAULT_TOKEN_BUDGET, LINE_GUIDANCE};
use agentsdoc::model::{Category, Report, Severity};
use agentsdoc::scan::Workspace;
use tempfile::TempDir;

/// Build a fixture repository from `(relative path, contents)` pairs.
fn fixture(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (path, contents) in files {
        write(dir.path(), path, contents);
    }
    dir
}

fn write(root: &Path, path: &str, contents: &str) {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(&full, contents).expect("write");
}

fn scan(dir: &TempDir) -> Workspace {
    Workspace::scan(dir.path(), "fixture").expect("scan")
}

fn run(dir: &TempDir) -> Report {
    audit(&scan(dir), &Options::default())
}

fn severity(report: &Report, id: &str) -> Severity {
    report
        .findings
        .iter()
        .find(|f| f.id == id)
        .unwrap_or_else(|| panic!("no finding {id}"))
        .severity
}

fn finding<'a>(report: &'a Report, id: &str) -> &'a agentsdoc::Finding {
    report
        .findings
        .iter()
        .find(|f| f.id == id)
        .unwrap_or_else(|| panic!("no finding {id}"))
}

const RICH_AGENTS_MD: &str = "\
# Example project

## Build and test
Run `cargo test` before opening a pull request.

## Code style
Use rustfmt defaults; four-space indentation.

## Commits
Conventional Commits, please.
";

// ---------------------------------------------------------------------------
// The headline case.
// ---------------------------------------------------------------------------

#[test]
fn agents_md_only_repository_fails_claude_code_portability() {
    let dir = fixture(&[("AGENTS.md", RICH_AGENTS_MD)]);
    let report = run(&dir);

    assert_eq!(severity(&report, "portability-claude-code"), Severity::Fail);
    assert_eq!(severity(&report, "portability-codex"), Severity::Pass);
    assert_eq!(severity(&report, "portability-cursor"), Severity::Pass);
    assert_eq!(severity(&report, "portability-copilot"), Severity::Pass);
    assert_eq!(report.exit_code(false), 1);

    // The FAIL must carry the citation that justifies it.
    let f = finding(&report, "portability-claude-code");
    assert!(f.source.contains("code.claude.com/docs/en/memory"));
    assert!(f.source.contains("2026-07-29"));
    assert!(f
        .source
        .contains("Claude Code reads CLAUDE.md, not AGENTS.md"));
}

#[test]
fn a_thin_importing_claude_md_makes_the_repository_clean() {
    let dir = fixture(&[
        ("AGENTS.md", RICH_AGENTS_MD),
        (
            "CLAUDE.md",
            "@AGENTS.md\n\n## Claude Code\nUse plan mode under src/billing/.\n",
        ),
    ]);
    let report = run(&dir);

    assert_eq!(severity(&report, "portability-claude-code"), Severity::Pass);
    assert_eq!(severity(&report, "portability-codex"), Severity::Pass);
    assert_eq!(
        severity(&report, "consistency-claude-import"),
        Severity::Pass
    );
    assert_eq!(severity(&report, "consistency-duplication"), Severity::Pass);
    assert_eq!(severity(&report, "consistency-imports"), Severity::Pass);
    assert!(!report.has_fail());
    assert_eq!(report.exit_code(false), 0);
}

#[test]
fn a_symlinked_claude_md_is_recognized_and_clean() {
    let dir = fixture(&[("AGENTS.md", RICH_AGENTS_MD)]);
    #[cfg(unix)]
    std::os::unix::fs::symlink("AGENTS.md", dir.path().join("CLAUDE.md")).expect("symlink");
    #[cfg(not(unix))]
    return;

    let report = run(&dir);
    assert_eq!(severity(&report, "portability-claude-code"), Severity::Pass);
    assert_eq!(
        severity(&report, "consistency-claude-import"),
        Severity::Pass
    );
    let f = finding(&report, "consistency-claude-import");
    assert!(f.title.contains("symlink"));
}

#[test]
fn a_duplicated_claude_md_warns_without_failing() {
    let body: String = (0..14)
        .map(|i| format!("- Always run verification step {i} before merging anything\n"))
        .collect();
    let dir = fixture(&[
        ("AGENTS.md", &format!("# Rules\n{body}")),
        ("CLAUDE.md", &format!("# Rules\n{body}")),
    ]);
    let report = run(&dir);

    assert_eq!(severity(&report, "consistency-duplication"), Severity::Warn);
    assert_eq!(
        severity(&report, "consistency-claude-import"),
        Severity::Warn
    );
    // Both agents do find instructions, so portability is clean.
    assert_eq!(severity(&report, "portability-claude-code"), Severity::Pass);
    assert_eq!(severity(&report, "portability-codex"), Severity::Pass);

    let f = finding(&report, "consistency-duplication");
    assert!(f.evidence[0].contains("AGENTS.md <-> CLAUDE.md"));
    assert!(f.evidence[0].contains("100% of the smaller file"));

    // WARN alone never fails the run; --strict does.
    assert_eq!(report.exit_code(false), 0);
    assert_eq!(report.exit_code(true), 1);
}

#[test]
fn contradicting_files_are_reported_as_a_heuristic() {
    let dir = fixture(&[
        (
            "AGENTS.md",
            "# Rules\n- Always use two-space indentation in this repository\n",
        ),
        (
            "CLAUDE.md",
            "@AGENTS.md\n- Never use two-space indentation in this repository\n",
        ),
    ]);
    let report = run(&dir);
    let f = finding(&report, "consistency-contradiction");
    assert_eq!(f.severity, Severity::Warn);
    assert!(f.title.contains("heuristic"));
    assert!(f.evidence[0].contains("AGENTS.md:2"));
    assert!(f.evidence[0].contains("CLAUDE.md:2"));
}

// ---------------------------------------------------------------------------
// Nesting and --effective.
// ---------------------------------------------------------------------------

#[test]
fn nested_agents_md_surfaces_the_precedence_tree() {
    let dir = fixture(&[
        ("AGENTS.md", RICH_AGENTS_MD),
        ("CLAUDE.md", "@AGENTS.md\n"),
        (
            "packages/api/AGENTS.md",
            "# API package\nRun `cargo test -p api`.\n",
        ),
        ("packages/api/src/lib.rs", "pub fn f() {}\n"),
    ]);
    let report = run(&dir);

    let nesting = finding(&report, "discovery-nesting");
    assert_eq!(nesting.severity, Severity::Info);
    // Root AGENTS.md, root CLAUDE.md, then the nested package file.
    assert_eq!(nesting.evidence.len(), 3);
    assert!(nesting.evidence[0].starts_with("AGENTS.md"));
    assert!(nesting.evidence[2].contains("wins for packages/api/**"));

    let ws = scan(&dir);
    let sets = effective_all(&ws, "packages/api/src");

    let codex = sets.iter().find(|e| e.agent == "codex").unwrap();
    let codex_paths: Vec<&str> = codex.layers.iter().map(|l| l.path.as_str()).collect();
    assert_eq!(codex_paths, ["packages/api/AGENTS.md"]);
    assert!(codex
        .notes
        .iter()
        .any(|n| n.contains("AGENTS.md is overridden")));

    // Claude Code concatenates root-down, and expands the import in place.
    let claude = sets.iter().find(|e| e.agent == "claude-code").unwrap();
    let claude_paths: Vec<&str> = claude.layers.iter().map(|l| l.path.as_str()).collect();
    assert_eq!(claude_paths, ["CLAUDE.md", "AGENTS.md"]);
    assert!(claude.est_tokens > 0);
}

#[test]
fn effective_totals_exclude_conditional_layers() {
    let dir = fixture(&[
        ("AGENTS.md", RICH_AGENTS_MD),
        ("CLAUDE.md", "# Claude only\nBe concise in reviews.\n"),
        (
            ".claude/rules/api.md",
            "---\npaths:\n  - \"src/api/**\"\n---\n# API rules\nValidate every input.\n",
        ),
        (
            ".claude/rules/global.md",
            "# Global\nPrefer small commits.\n",
        ),
    ]);
    let ws = scan(&dir);
    let sets = effective_all(&ws, "");
    let claude = sets.iter().find(|e| e.agent == "claude-code").unwrap();

    let conditional: Vec<&str> = claude
        .layers
        .iter()
        .filter(|l| l.conditional)
        .map(|l| l.path.as_str())
        .collect();
    assert_eq!(conditional, [".claude/rules/api.md"]);

    let unconditional: usize = claude
        .layers
        .iter()
        .filter(|l| !l.conditional)
        .map(|l| l.est_tokens)
        .sum();
    assert_eq!(claude.est_tokens, unconditional);
}

// ---------------------------------------------------------------------------
// Content checks.
// ---------------------------------------------------------------------------

#[test]
fn an_oversized_file_warns_at_the_documented_threshold() {
    let big = "x".repeat(DEFAULT_TOKEN_BUDGET * 4 + 100);
    let dir = fixture(&[("AGENTS.md", &big), ("CLAUDE.md", "@AGENTS.md\n")]);
    let report = run(&dir);

    let f = finding(&report, "content-size");
    assert_eq!(f.severity, Severity::Warn);
    assert!(f.evidence.iter().any(|e| e.starts_with("AGENTS.md")));
    assert!(f.source.contains("target under 200 lines"));
    assert_eq!(report.exit_code(false), 0);
    assert_eq!(report.exit_code(true), 1);
}

#[test]
fn the_line_guidance_is_independent_of_the_token_budget() {
    let many = format!("# Rules\n{}", "- keep it short\n".repeat(LINE_GUIDANCE));
    let dir = fixture(&[("AGENTS.md", &many)]);
    let report = run(&dir);
    let f = finding(&report, "content-size");
    assert_eq!(f.severity, Severity::Warn);
    assert!(f.evidence[0].contains("lines > 200 guidance"));
}

#[test]
fn a_custom_token_budget_is_honored() {
    let dir = fixture(&[("AGENTS.md", RICH_AGENTS_MD)]);
    let ws = scan(&dir);
    assert_eq!(
        severity(&audit(&ws, &Options::default()), "content-size"),
        Severity::Pass
    );
    let tight = Options {
        token_budget: 5,
        ..Options::default()
    };
    assert_eq!(
        severity(&audit(&ws, &tight), "content-size"),
        Severity::Warn
    );
}

#[test]
fn a_broken_relative_link_fails() {
    let dir = fixture(&[
        (
            "AGENTS.md",
            "# Rules\nSee [the guide](docs/guide.md) and [the spec](https://agents.md/).\n",
        ),
        ("CLAUDE.md", "@AGENTS.md\n"),
    ]);
    let report = run(&dir);
    let f = finding(&report, "content-links");
    assert_eq!(f.severity, Severity::Fail);
    assert_eq!(f.evidence.len(), 1);
    assert!(f.evidence[0].starts_with("AGENTS.md:2"));
    assert!(f.evidence[0].ends_with("(missing)"));
    assert_eq!(report.exit_code(false), 1);
}

#[test]
fn a_resolvable_link_passes_even_when_the_target_is_gitignored() {
    let dir = fixture(&[
        (".gitignore", "build/\n"),
        (
            "AGENTS.md",
            "# Rules\nSee [the output](build/notes.md) for details.\n",
        ),
        ("build/notes.md", "# Notes\n"),
        ("CLAUDE.md", "@AGENTS.md\n"),
    ]);
    let report = run(&dir);
    // The gitignored file is not part of the walk, but the link probe finds it.
    assert_eq!(severity(&report, "content-links"), Severity::Pass);
}

#[test]
fn a_broken_import_fails() {
    let dir = fixture(&[
        ("AGENTS.md", RICH_AGENTS_MD),
        ("CLAUDE.md", "@AGENTS.md\n@docs/gone.md\n"),
    ]);
    let report = run(&dir);
    let f = finding(&report, "consistency-imports");
    assert_eq!(f.severity, Severity::Fail);
    assert!(f.evidence.iter().any(|e| e.contains("docs/gone.md")));
}

#[test]
fn missing_recommended_sections_warn_and_say_they_are_optional() {
    let dir = fixture(&[
        ("AGENTS.md", "# Project\n\nWe like clean code.\n"),
        ("CLAUDE.md", "@AGENTS.md\n"),
    ]);
    let report = run(&dir);
    let f = finding(&report, "content-sections");
    assert_eq!(f.severity, Severity::Warn);
    assert!(f.title.contains("recommended (not required)"));
    assert!(f.source.contains("no required schema"));
}

// ---------------------------------------------------------------------------
// Neutral and edge cases.
// ---------------------------------------------------------------------------

#[test]
fn a_repository_with_no_instruction_files_is_not_a_failure() {
    let dir = fixture(&[
        ("README.md", "# Hello\n"),
        ("src/main.rs", "fn main() {}\n"),
    ]);
    let report = run(&dir);

    assert!(!report.has_fail());
    assert!(!report.has_warn());
    assert_eq!(report.counts.fail, 0);
    assert_eq!(report.exit_code(true), 0);
    assert_eq!(report.score, None);
    assert_eq!(report.grade(), "n/a");
    assert!(finding(&report, "discovery-files")
        .title
        .contains("no agent instruction files"));
}

#[test]
fn gitignored_and_vendored_instruction_files_are_not_counted() {
    let dir = fixture(&[
        (".gitignore", "vendor/\n"),
        ("AGENTS.md", RICH_AGENTS_MD),
        ("CLAUDE.md", "@AGENTS.md\n"),
        ("vendor/lib/AGENTS.md", "# someone else's rules\n"),
    ]);
    let report = run(&dir);
    let paths: Vec<&str> = report.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["AGENTS.md", "CLAUDE.md"]);
}

#[test]
fn hidden_convention_directories_are_still_walked() {
    let dir = fixture(&[
        (".github/copilot-instructions.md", "# Copilot\nBe terse.\n"),
        (
            ".cursor/rules/style.mdc",
            "---\nalwaysApply: true\n---\nUse tabs.\n",
        ),
        (".claude/CLAUDE.md", "# Claude\nBe terse.\n"),
    ]);
    let report = run(&dir);
    let kinds: Vec<&str> = report.files.iter().map(|f| f.kind).collect();
    assert!(kinds.contains(&"copilot-instructions"));
    assert!(kinds.contains(&"cursor-rule"));
    assert!(kinds.contains(&"claude-dir-md"));
    assert_eq!(severity(&report, "portability-claude-code"), Severity::Pass);
    assert_eq!(severity(&report, "portability-copilot"), Severity::Pass);
}

#[test]
fn a_legacy_cursorrules_repository_warns_rather_than_fails_for_cursor() {
    let dir = fixture(&[(".cursorrules", "# Legacy rules\nBe terse.\n")]);
    let report = run(&dir);
    assert_eq!(severity(&report, "portability-cursor"), Severity::Warn);
    assert_eq!(severity(&report, "portability-claude-code"), Severity::Fail);
}

// ---------------------------------------------------------------------------
// Flags and determinism.
// ---------------------------------------------------------------------------

#[test]
fn only_and_skip_filter_the_run() {
    let dir = fixture(&[("AGENTS.md", RICH_AGENTS_MD)]);
    let ws = scan(&dir);

    let only = audit(
        &ws,
        &Options {
            only: vec![Category::Content],
            ..Options::default()
        },
    );
    assert!(only
        .findings
        .iter()
        .all(|f| f.category == Category::Content));
    assert!(only.skipped > 0);

    let skipped = audit(
        &ws,
        &Options {
            skip: vec!["portability-claude-code".into()],
            ..Options::default()
        },
    );
    assert!(!skipped
        .findings
        .iter()
        .any(|f| f.id == "portability-claude-code"));
    // Removing the only FAIL makes the run clean.
    assert_eq!(skipped.exit_code(false), 0);
}

#[test]
fn findings_and_files_are_deterministically_ordered() {
    let dir = fixture(&[
        ("packages/z/AGENTS.md", "# z\n"),
        ("packages/a/AGENTS.md", "# a\n"),
        ("AGENTS.md", RICH_AGENTS_MD),
        ("CLAUDE.md", "@AGENTS.md\n"),
    ]);
    let a = run(&dir);
    let b = run(&dir);
    assert_eq!(a, b);

    let files: Vec<&str> = a.files.iter().map(|f| f.path.as_str()).collect();
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted);

    let ids: Vec<&str> = a.findings.iter().map(|f| f.id).collect();
    let mut by_category = ids.clone();
    by_category.sort_by_key(|id| {
        let cat = a.findings.iter().find(|f| f.id == *id).unwrap().category;
        (
            Category::ALL.iter().position(|c| *c == cat).unwrap(),
            id.to_string(),
        )
    });
    assert_eq!(ids, by_category);
}
