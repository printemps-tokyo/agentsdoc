//! Command-line entry point for agentsdoc.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;

use agentsdoc::agents::effective_all;
use agentsdoc::checks::{check_ids, Options, DEFAULT_TOKEN_BUDGET};
use agentsdoc::model::Category;
use agentsdoc::report::{
    render_effective_json, render_effective_text, render_json, render_text, summary_line,
    RenderOptions,
};
use agentsdoc::scan::Workspace;
use agentsdoc::CAVEAT;

const AFTER_HELP: &str = "\
CATEGORIES:
  discovery, portability, consistency, content

EXIT CODES:
  0  no FAIL findings (and, with --strict, no WARN findings)
  1  at least one FAIL finding (with --strict, also any WARN)
  2  usage or I/O error

Tool loading behavior changes fast. agentsdoc encodes what the cited official
docs said on 2026-07-29 and prints the source next to every finding; verify
against your tool's current documentation before acting on one. Behaviors that
could not be confirmed from an official doc are reported as INFO and labeled
unverified, never as FAIL.";

/// Audit a repository's AI coding agent instruction files for consistency and
/// cross-tool portability.
#[derive(Parser, Debug)]
#[command(
    name = "agentsdoc",
    version,
    about,
    long_about = None,
    after_help = AFTER_HELP
)]
struct Cli {
    /// Repository directory to audit.
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Emit the machine-readable JSON document instead of the human report.
    #[arg(long)]
    json: bool,

    /// Also exit non-zero when any check warns.
    #[arg(long)]
    strict: bool,

    /// Print the effective instruction set each agent assembles for this
    /// subdirectory, instead of running the audit.
    #[arg(long, value_name = "SUBDIR")]
    effective: Option<PathBuf>,

    /// Per-file estimated-token budget for content-size.
    #[arg(long, value_name = "N", default_value_t = DEFAULT_TOKEN_BUDGET)]
    tokens: usize,

    /// Run only checks in this category (repeatable).
    #[arg(long, value_name = "CATEGORY", value_parser = parse_category)]
    only: Vec<Category>,

    /// Skip a check by id (repeatable).
    #[arg(long, value_name = "CHECK_ID")]
    skip: Vec<String>,

    /// Never emit ANSI color codes.
    #[arg(long)]
    no_color: bool,

    /// Print only the one-line summary.
    #[arg(short, long)]
    quiet: bool,
}

fn parse_category(s: &str) -> Result<Category, String> {
    Category::parse(s).ok_or_else(|| {
        let names: Vec<&str> = Category::ALL.iter().map(|c| c.as_str()).collect();
        format!(
            "unknown category '{s}' (expected one of: {})",
            names.join(", ")
        )
    })
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("agentsdoc: {err:#}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<i32> {
    let cli = Cli::parse();

    for id in &cli.skip {
        if !check_ids().contains(&id.as_str()) {
            anyhow::bail!(
                "unknown check id '{id}'\nknown ids: {}",
                check_ids().join(", ")
            );
        }
    }
    if cli.tokens == 0 {
        anyhow::bail!("--tokens must be greater than zero");
    }
    if !cli.path.is_dir() {
        anyhow::bail!("not a directory: {}", cli.path.display());
    }

    let root_name = cli
        .path
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| cli.path.display().to_string());

    let workspace = Workspace::scan(&cli.path, root_name.clone())
        .with_context(|| format!("failed to walk {}", cli.path.display()))?;

    // Color is opt-out via the flag, the NO_COLOR convention, or a non-tty
    // stdout; JSON is never colored.
    let color = !cli.no_color
        && !cli.json
        && std::env::var_os("NO_COLOR").is_none()
        && std::io::stdout().is_terminal();
    let opts = RenderOptions { color };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if let Some(subdir) = &cli.effective {
        let rel = relative_subdir(&cli.path, subdir)?;
        let sets = effective_all(&workspace, &rel);
        if cli.json {
            out.write_all(render_effective_json(&root_name, &rel, &sets).as_bytes())?;
        } else {
            out.write_all(render_effective_text(&root_name, &rel, &sets, opts).as_bytes())?;
        }
        out.flush()?;
        return Ok(0);
    }

    let report = agentsdoc::audit(
        &workspace,
        &Options {
            only: cli.only,
            skip: cli.skip,
            token_budget: cli.tokens,
        },
    );

    if cli.json {
        out.write_all(render_json(&report).as_bytes())?;
    } else if cli.quiet {
        // stdout stays a single pipeable line; the framing goes to stderr so it
        // is still shown without polluting the summary.
        writeln!(
            out,
            "agentsdoc: {}  {}",
            report.root_name,
            summary_line(&report, opts)
        )?;
        eprintln!("{CAVEAT}");
    } else {
        out.write_all(render_text(&report, opts).as_bytes())?;
    }
    out.flush()?;

    Ok(report.exit_code(cli.strict))
}

/// Turn `--effective <subdir>` into a repository-relative path with forward
/// slashes, rejecting anything outside the audited root.
fn relative_subdir(root: &Path, subdir: &Path) -> Result<String> {
    // Accept both an absolute path and one relative to the audited root.
    let candidate = if subdir.is_absolute() {
        subdir.to_path_buf()
    } else {
        root.join(subdir)
    };
    if !candidate.is_dir() {
        anyhow::bail!("not a directory: {}", candidate.display());
    }
    let root_abs = root
        .canonicalize()
        .with_context(|| format!("cannot resolve {}", root.display()))?;
    let sub_abs = candidate
        .canonicalize()
        .with_context(|| format!("cannot resolve {}", candidate.display()))?;
    let rel = sub_abs.strip_prefix(&root_abs).map_err(|_| {
        anyhow::anyhow!(
            "{} is outside the audited repository {}",
            sub_abs.display(),
            root_abs.display()
        )
    })?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}
