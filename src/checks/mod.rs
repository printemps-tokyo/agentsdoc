//! The check registry.
//!
//! Every check is a pure function from a [`Workspace`] plus [`Options`] to
//! exactly one [`Finding`]. One check produces one finding, always, so the
//! catalog printed in the README is exactly the catalog the tool emits and
//! `--skip <id>` has an unambiguous target.
//!
//! Every check is anchored on a document agentsdoc actually read, named in the
//! finding's `source` field together with the date. FAIL is reserved for
//! concrete breakage that a cited behavior makes verifiable: a repository whose
//! instructions a documented loader would never see, a `@path` import that
//! resolves to nothing, a relative link that points at a path the repository
//! does not contain. Everything else is WARN or INFO.

pub mod consistency;
pub mod content;
pub mod discovery;
pub mod portability;

use crate::agents::{self, AGENTS};
use crate::model::{AgentCoverage, Category, FileSummary, Finding, Report, Severity};
use crate::scan::Workspace;

/// Default per-file estimated-token budget used by `content-size`.
///
/// Anchored on the Claude Code memory doc (read 2026-07-29): "target under 200
/// lines per CLAUDE.md file. Longer files consume more context and reduce
/// adherence." 200 lines of prose is roughly 12,000 characters, which the
/// chars/4 heuristic puts near 3,000 tokens; 4,000 leaves headroom for code
/// blocks and tables so the default does not fire on ordinary files. Override
/// it with `--tokens`.
pub const DEFAULT_TOKEN_BUDGET: usize = 4000;

/// Line-count guidance applied alongside the token budget, straight from the
/// Claude Code memory doc.
pub const LINE_GUIDANCE: usize = 200;

/// Static description of one check, shared by every finding it can emit.
pub(crate) struct Meta {
    pub id: &'static str,
    pub category: Category,
    /// Why it matters.
    pub why: &'static str,
    /// The primary source, with the date agentsdoc read it.
    pub source: &'static str,
    /// The concrete next step when the check is not clean.
    pub fix: &'static str,
}

impl Meta {
    pub(crate) fn finding(
        &self,
        severity: Severity,
        title: impl Into<String>,
        evidence: Vec<String>,
    ) -> Finding {
        Finding {
            id: self.id,
            category: self.category,
            severity,
            title: title.into(),
            evidence,
            why: self.why,
            source: self.source,
            fix: self.fix,
        }
    }
}

/// Which checks to run, and the thresholds they use.
#[derive(Debug, Clone)]
pub struct Options {
    /// When non-empty, run only checks in these categories.
    pub only: Vec<Category>,
    /// Check ids to suppress entirely.
    pub skip: Vec<String>,
    /// Per-file estimated-token budget for `content-size`.
    pub token_budget: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            only: Vec::new(),
            skip: Vec::new(),
            token_budget: DEFAULT_TOKEN_BUDGET,
        }
    }
}

/// How one registry entry is executed.
enum Runner {
    /// A plain check over the workspace.
    Plain(fn(&Workspace, &Options) -> Finding),
    /// The per-agent portability check.
    ForAgent(&'static agents::Agent),
}

/// One entry in the registry.
struct CheckSpec {
    id: &'static str,
    category: Category,
    run: Runner,
}

fn registry() -> Vec<CheckSpec> {
    let mut specs = vec![
        CheckSpec {
            id: "discovery-files",
            category: Category::Discovery,
            run: Runner::Plain(discovery::files),
        },
        CheckSpec {
            id: "discovery-nesting",
            category: Category::Discovery,
            run: Runner::Plain(discovery::nesting),
        },
    ];
    specs.extend(AGENTS.iter().map(|a| CheckSpec {
        id: a.check_id,
        category: Category::Portability,
        run: Runner::ForAgent(a),
    }));
    specs.extend([
        CheckSpec {
            id: "consistency-claude-import",
            category: Category::Consistency,
            run: Runner::Plain(consistency::claude_import),
        },
        CheckSpec {
            id: "consistency-duplication",
            category: Category::Consistency,
            run: Runner::Plain(consistency::duplication),
        },
        CheckSpec {
            id: "consistency-contradiction",
            category: Category::Consistency,
            run: Runner::Plain(consistency::contradiction),
        },
        CheckSpec {
            id: "consistency-imports",
            category: Category::Consistency,
            run: Runner::Plain(consistency::broken_imports),
        },
        CheckSpec {
            id: "content-size",
            category: Category::Content,
            run: Runner::Plain(content::size),
        },
        CheckSpec {
            id: "content-sections",
            category: Category::Content,
            run: Runner::Plain(content::sections),
        },
        CheckSpec {
            id: "content-links",
            category: Category::Content,
            run: Runner::Plain(content::links),
        },
    ]);
    specs
}

impl Options {
    fn selected(&self, spec: &CheckSpec) -> bool {
        if !self.only.is_empty() && !self.only.contains(&spec.category) {
            return false;
        }
        !self.skip.iter().any(|s| s == spec.id)
    }
}

/// Run the selected checks against `ws` and aggregate the report.
pub fn audit(ws: &Workspace, opts: &Options) -> Report {
    let specs = registry();
    let mut findings = Vec::with_capacity(specs.len());
    let mut skipped = 0;
    for spec in &specs {
        if !opts.selected(spec) {
            skipped += 1;
            continue;
        }
        findings.push(match spec.run {
            Runner::Plain(f) => f(ws, opts),
            Runner::ForAgent(a) => portability::for_agent(a, ws),
        });
    }

    let files = ws
        .files()
        .iter()
        .map(|f| FileSummary {
            path: f.path.clone(),
            kind: f.kind.as_str(),
            bytes: f.bytes,
            lines: f.lines,
            est_tokens: f.est_tokens(),
        })
        .collect();

    let coverage = AGENTS
        .iter()
        .map(|a| AgentCoverage {
            agent: a.id,
            name: a.name,
            verified: a.verified,
            files: agents::coverage(a.id, ws)
                .into_iter()
                .map(|f| f.path.clone())
                .collect(),
        })
        .collect();

    Report::new(
        ws.root_name().to_string(),
        files,
        coverage,
        findings,
        skipped,
    )
}

/// Every known check id, for `--skip` validation and `--help` text.
pub fn check_ids() -> Vec<&'static str> {
    registry().into_iter().map(|c| c.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_check_id_is_unique_and_matches_its_finding() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "# Build\n\nRun `cargo test`.\n")]);
        let opts = Options::default();
        let mut seen = std::collections::BTreeSet::new();
        for spec in registry() {
            assert!(seen.insert(spec.id), "duplicate check id {}", spec.id);
            let f = match spec.run {
                Runner::Plain(run) => run(&ws, &opts),
                Runner::ForAgent(a) => portability::for_agent(a, &ws),
            };
            assert_eq!(f.id, spec.id);
            assert_eq!(
                f.category, spec.category,
                "category mismatch for {}",
                spec.id
            );
            assert!(!f.title.is_empty(), "{} has an empty title", spec.id);
            assert!(!f.why.is_empty(), "{} has no rationale", spec.id);
            assert!(!f.source.is_empty(), "{} has no source", spec.id);
            assert!(!f.fix.is_empty(), "{} has no fix", spec.id);
        }
    }

    #[test]
    fn only_and_skip_filter_the_registry() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "# hi")]);

        let only_port = audit(
            &ws,
            &Options {
                only: vec![Category::Portability],
                ..Options::default()
            },
        );
        assert!(only_port
            .findings
            .iter()
            .all(|f| f.category == Category::Portability));
        assert_eq!(only_port.categories.len(), 1);
        assert_eq!(only_port.findings.len(), AGENTS.len());

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
        assert_eq!(skipped.skipped, 1);
    }

    #[test]
    fn the_report_carries_the_inventory_and_coverage_matrix() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "# hi")]);
        let r = audit(&ws, &Options::default());
        assert_eq!(r.files.len(), 1);
        assert_eq!(r.files[0].kind, "agents-md");
        assert_eq!(r.agents.len(), AGENTS.len());
        let claude = r.agents.iter().find(|a| a.agent == "claude-code").unwrap();
        assert!(claude.files.is_empty());
    }
}
