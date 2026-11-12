//! Black-box tests of the binary: flags, exit codes, JSON shape, determinism.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// Path to the binary under test, as cargo built it.
fn bin() -> PathBuf {
    let mut p = std::env::current_exe().expect("test exe path");
    p.pop(); // the deps/ directory
    if p.ends_with("deps") {
        p.pop();
    }
    p.join(format!("agentsdoc{}", std::env::consts::EXE_SUFFIX))
}

fn fixture(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (path, contents) in files {
        let full = dir.path().join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(&full, contents).expect("write");
    }
    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg(dir)
        .args(args)
        .env_remove("NO_COLOR")
        .output()
        .expect("run agentsdoc")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("exit code")
}

const CLEAN: [(&str, &str); 2] = [
    (
        "AGENTS.md",
        "# Example\n\n## Build and test\nRun `cargo test`.\n\n## Code style\nrustfmt defaults.\n\n\
         ## Commits\nConventional Commits.\n",
    ),
    ("CLAUDE.md", "@AGENTS.md\n"),
];

#[test]
fn a_clean_repository_exits_zero() {
    let dir = fixture(&CLEAN);
    let out = run(dir.path(), &["--no-color"]);
    assert_eq!(code(&out), 0);
    let text = stdout(&out);
    assert!(text.starts_with("agentsdoc: "));
    assert!(text.contains("[PASS] portability-claude-code"));
    assert!(text.contains("Agent loading behavior changes fast"));
}

#[test]
fn an_agents_md_only_repository_exits_one() {
    let dir = fixture(&[CLEAN[0]]);
    let out = run(dir.path(), &["--no-color"]);
    assert_eq!(code(&out), 1);
    assert!(stdout(&out).contains("[FAIL] portability-claude-code"));
}

#[test]
fn strict_promotes_warnings_to_a_failure() {
    let dir = fixture(&[
        (
            "AGENTS.md",
            "# Example\n\nNo commands, no style, no commits.\n",
        ),
        ("CLAUDE.md", "@AGENTS.md\n"),
    ]);
    let normal = run(dir.path(), &["--no-color"]);
    assert_eq!(code(&normal), 0);
    assert!(stdout(&normal).contains("[WARN] content-sections"));
    assert_eq!(code(&run(dir.path(), &["--no-color", "--strict"])), 1);
}

#[test]
fn a_repository_with_no_instruction_files_exits_zero_even_under_strict() {
    let dir = fixture(&[("README.md", "# Hello\n")]);
    let out = run(dir.path(), &["--no-color", "--strict"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("no agent instruction files found"));
}

#[test]
fn json_output_has_the_documented_shape() {
    let dir = fixture(&CLEAN);
    let out = run(dir.path(), &["--json"]);
    let text = stdout(&out);
    assert_eq!(code(&out), 0);

    for key in [
        "\"tool\": \"agentsdoc\"",
        "\"version\":",
        "\"sources_read\":",
        "\"repository\":",
        "\"grade\":",
        "\"score\":",
        "\"counts\":",
        "\"skipped\":",
        "\"total_est_tokens\":",
        "\"files\":",
        "\"agents\":",
        "\"categories\":",
        "\"findings\":",
        "\"caveat\":",
    ] {
        assert!(text.contains(key), "missing {key} in JSON output");
    }
    // Findings carry their full contract.
    for key in [
        "\"id\":",
        "\"severity\":",
        "\"evidence\":",
        "\"source\":",
        "\"fix\":",
    ] {
        assert!(text.contains(key), "missing {key} in JSON findings");
    }
    // Never colored, and balanced quoting.
    assert!(!text.contains('\u{1b}'));
    assert_eq!(text.matches('"').count() % 2, 0);
}

#[test]
fn json_output_is_byte_identical_across_runs() {
    let dir = fixture(&CLEAN);
    let a = stdout(&run(dir.path(), &["--json"]));
    let b = stdout(&run(dir.path(), &["--json"]));
    assert_eq!(a, b);
}

#[test]
fn text_output_is_byte_identical_across_runs() {
    let dir = fixture(&CLEAN);
    let a = stdout(&run(dir.path(), &["--no-color"]));
    let b = stdout(&run(dir.path(), &["--no-color"]));
    assert_eq!(a, b);
}

#[test]
fn quiet_prints_only_the_summary_line() {
    let dir = fixture(&CLEAN);
    let out = run(dir.path(), &["--no-color", "--quiet"]);
    let text = stdout(&out);
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("grade "));
    // The caveat still shows, on stderr, so it cannot pollute a pipeline.
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("Agent loading behavior changes fast"));
}

#[test]
fn only_restricts_the_run_to_one_category() {
    let dir = fixture(&[CLEAN[0]]);
    let out = run(dir.path(), &["--no-color", "--only", "content"]);
    let text = stdout(&out);
    assert!(text.contains("Content"));
    assert!(!text.contains("portability-claude-code"));
    // The FAIL lived in portability, so filtering it out makes the run clean.
    assert_eq!(code(&out), 0);
}

#[test]
fn skip_is_repeatable_and_validated() {
    let dir = fixture(&[CLEAN[0]]);
    let out = run(
        dir.path(),
        &[
            "--no-color",
            "--skip",
            "portability-claude-code",
            "--skip",
            "portability-gemini-cli",
        ],
    );
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("2 skipped"));

    let bad = run(dir.path(), &["--skip", "no-such-check"]);
    assert_eq!(code(&bad), 2);
    assert!(String::from_utf8(bad.stderr)
        .unwrap()
        .contains("unknown check id"));
}

#[test]
fn an_unknown_category_is_a_usage_error() {
    let dir = fixture(&CLEAN);
    let out = run(dir.path(), &["--only", "nonsense"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn a_missing_path_is_an_io_error() {
    let out = Command::new(bin())
        .arg("/definitely/not/a/real/path/agentsdoc")
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("not a directory"));
}

#[test]
fn the_token_threshold_is_configurable_from_the_cli() {
    let dir = fixture(&CLEAN);
    let loose = run(dir.path(), &["--no-color", "--only", "content", "--strict"]);
    assert_eq!(code(&loose), 0);

    let tight = run(
        dir.path(),
        &[
            "--no-color",
            "--only",
            "content",
            "--strict",
            "--tokens",
            "5",
        ],
    );
    assert_eq!(code(&tight), 1);
    assert!(stdout(&tight).contains("tokens est. > 5 budget"));

    let zero = run(dir.path(), &["--tokens", "0"]);
    assert_eq!(code(&zero), 2);
}

#[test]
fn effective_prints_the_per_agent_load_order() {
    let dir = fixture(&[
        CLEAN[0],
        CLEAN[1],
        (
            "packages/api/AGENTS.md",
            "# API\nRun `cargo test -p api`.\n",
        ),
        ("packages/api/src/lib.rs", "pub fn f() {}\n"),
    ]);
    let out = run(dir.path(), &["--no-color", "--effective", "packages/api"]);
    assert_eq!(code(&out), 0);
    let text = stdout(&out);
    assert!(text.contains("effective instruction set for packages/api"));
    assert!(text.contains("Claude Code [claude-code]"));
    assert!(text.contains("packages/api/AGENTS.md"));
    assert!(text.contains("total (unconditional):"));
}

#[test]
fn effective_supports_json_and_rejects_paths_outside_the_repository() {
    let dir = fixture(&CLEAN);
    let out = run(dir.path(), &["--effective", ".", "--json"]);
    assert_eq!(code(&out), 0);
    let text = stdout(&out);
    assert!(text.contains("\"effective\": ["));
    assert!(text.contains("\"layers\": ["));
    assert!(text.contains("\"conditional\":"));
    assert_eq!(text.matches('"').count() % 2, 0);

    let outside = run(dir.path(), &["--effective", "/tmp"]);
    assert_eq!(code(&outside), 2);
}

#[test]
fn no_color_and_the_no_color_convention_both_suppress_escapes() {
    let dir = fixture(&CLEAN);
    assert!(!stdout(&run(dir.path(), &["--no-color"])).contains('\u{1b}'));

    let out = Command::new(bin())
        .arg(dir.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("run");
    assert!(!stdout(&out).contains('\u{1b}'));
}

#[test]
fn help_and_version_work() {
    let help = Command::new(bin()).arg("--help").output().expect("run");
    assert_eq!(help.status.code(), Some(0));
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("--effective <SUBDIR>"));
    assert!(text.contains("EXIT CODES:"));

    let version = Command::new(bin()).arg("--version").output().expect("run");
    assert_eq!(version.status.code(), Some(0));
    assert!(String::from_utf8(version.stdout)
        .unwrap()
        .contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn fix_syncs_the_other_tools_to_agents_md_and_dry_run_changes_nothing() {
    let dir = fixture(&[
        (
            "AGENTS.md",
            "# rules\n\nRun `cargo test` before committing.\n",
        ),
        (
            "GEMINI.md",
            "# rules\n\nRun `cargo test` before committing.\n",
        ),
    ]);

    let dry = run(dir.path(), &["--fix", "--dry-run", "--quiet"]);
    let log = String::from_utf8(dry.stderr.clone()).unwrap();
    assert!(log.contains("would create CLAUDE.md"), "{log}");
    assert!(log.contains("would rewrite GEMINI.md"), "{log}");
    assert!(!dir.path().join("CLAUDE.md").exists());

    let fixed = run(dir.path(), &["--fix", "--json"]);
    assert_eq!(
        fixed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&fixed.stdout)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "@AGENTS.md\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("GEMINI.md")).unwrap(),
        "@AGENTS.md\n"
    );
    // The report on stdout is the audit after the fix.
    let doc = String::from_utf8(fixed.stdout).unwrap();
    assert!(doc.trim_start().starts_with('{'));

    let again = run(dir.path(), &["--fix", "--quiet"]);
    assert!(String::from_utf8_lossy(&again.stderr).contains("nothing to change"));

    let refused = run(dir.path(), &["--dry-run"]);
    assert_eq!(refused.status.code(), Some(2), "--dry-run needs --fix");
}

#[test]
fn fix_moves_a_lone_claude_md_into_agents_md() {
    let dir = fixture(&[("CLAUDE.md", "# rules\n\nUse pnpm.\n")]);
    let out = run(dir.path(), &["--fix", "--quiet"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        "# rules\n\nUse pnpm.\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "@AGENTS.md\n"
    );
}
