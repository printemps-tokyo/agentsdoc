//! Core data model: severities, categories, findings and the aggregated report.
//!
//! Nothing in this module touches the filesystem; it is pure data plus the
//! scoring rollup, so it is trivially testable.

use std::fmt;

/// Outcome of a single check.
///
/// `Info` is deliberately unscored. Several of the things agentsdoc reports are
/// neutral facts ("this repository has no agent instruction files at all",
/// "aider never auto-loads a conventions file") rather than defects, and a
/// repository should not be graded down for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The property holds.
    Pass,
    /// Context only; never affects the score or the exit code.
    Info,
    /// Advisory. Worth looking at, but not verifiable breakage. Only fails the
    /// run under `--strict`.
    Warn,
    /// Concrete, verifiable breakage backed by a cited tool behavior.
    Fail,
}

impl Severity {
    /// Lowercase machine token used in JSON output.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Pass => "pass",
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Fail => "fail",
        }
    }

    /// Fixed-width uppercase label used in the human report.
    pub fn label(self) -> &'static str {
        match self {
            Severity::Pass => "PASS",
            Severity::Info => "INFO",
            Severity::Warn => "WARN",
            Severity::Fail => "FAIL",
        }
    }

    /// Fraction of a check's weight earned by this outcome.
    fn credit(self) -> f64 {
        match self {
            Severity::Pass => 1.0,
            Severity::Warn => 0.5,
            Severity::Fail => 0.0,
            // Info is excluded from scoring entirely (see `Finding::weight`).
            Severity::Info => 1.0,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Rollup bucket a check belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// What instruction files exist, where, and how large they are.
    Discovery,
    /// Which well-known agents would find project instructions here.
    Portability,
    /// Whether the files agree with each other.
    Consistency,
    /// Size, recommended sections, resolvable references.
    Content,
}

impl Category {
    /// Every category, in the order they are rendered. Rendering order is part
    /// of the deterministic-output contract.
    pub const ALL: [Category; 4] = [
        Category::Discovery,
        Category::Portability,
        Category::Consistency,
        Category::Content,
    ];

    /// Machine token, also the accepted `--only` value.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Discovery => "discovery",
            Category::Portability => "portability",
            Category::Consistency => "consistency",
            Category::Content => "content",
        }
    }

    /// Human title used as the section heading.
    pub fn title(self) -> &'static str {
        match self {
            Category::Discovery => "Discovery",
            Category::Portability => "Portability",
            Category::Consistency => "Consistency",
            Category::Content => "Content",
        }
    }

    /// Parse a `--only` value. A few obvious aliases are accepted so the flag
    /// is forgiving without being magic.
    pub fn parse(s: &str) -> Option<Category> {
        match s.trim().to_ascii_lowercase().as_str() {
            "discovery" | "discover" | "inventory" => Some(Category::Discovery),
            "portability" | "portable" | "port" => Some(Category::Portability),
            "consistency" | "consistent" | "consist" => Some(Category::Consistency),
            "content" | "quality" => Some(Category::Content),
            _ => None,
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One check result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable identifier, also the `--skip` value (e.g. `portability-claude-code`).
    pub id: &'static str,
    /// Rollup bucket.
    pub category: Category,
    /// Outcome.
    pub severity: Severity,
    /// One-line human summary of what was (or was not) found.
    pub title: String,
    /// Concrete evidence: repository-relative paths, or `path:line`. Always
    /// sorted by the check that produced it.
    pub evidence: Vec<String>,
    /// Why it matters.
    pub why: &'static str,
    /// The primary source that motivates the check, with the date it was read.
    pub source: &'static str,
    /// A concrete next step.
    pub fix: &'static str,
}

impl Finding {
    /// Scoring weight. `Info` findings are unscored. The two checks that map
    /// onto the headline failure modes (a repository whose instructions are
    /// invisible to Claude Code, or to every AGENTS.md consumer) count double.
    pub fn weight(&self) -> f64 {
        if self.severity == Severity::Info {
            return 0.0;
        }
        match self.id {
            "portability-claude-code" | "portability-codex" => 2.0,
            _ => 1.0,
        }
    }
}

/// Per-severity tallies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
    pub info: usize,
}

impl Counts {
    fn add(&mut self, severity: Severity) {
        match severity {
            Severity::Pass => self.pass += 1,
            Severity::Warn => self.warn += 1,
            Severity::Fail => self.fail += 1,
            Severity::Info => self.info += 1,
        }
    }
}

/// Rollup for one category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryScore {
    pub category: Category,
    /// 0-100, or `None` when every finding in the category was informational
    /// (or the category was filtered out).
    pub score: Option<u32>,
    pub counts: Counts,
}

/// One discovered instruction file, as reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSummary {
    /// Repository-relative path with forward slashes.
    pub path: String,
    /// Machine token for the file kind (e.g. `agents-md`).
    pub kind: &'static str,
    pub bytes: u64,
    pub lines: usize,
    /// Estimated tokens. See [`crate::text::estimate_tokens`] for the heuristic.
    pub est_tokens: usize,
}

/// Whether a given agent would find project instructions in this repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCoverage {
    pub agent: &'static str,
    pub name: &'static str,
    /// False when agentsdoc could not verify the behavior from an official doc.
    pub verified: bool,
    /// Files this agent would load, sorted.
    pub files: Vec<String>,
}

/// The aggregated audit result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Display name of the audited repository (its directory name).
    pub root_name: String,
    /// Every instruction file found, sorted by path.
    pub files: Vec<FileSummary>,
    /// Per-agent coverage matrix, in registry order.
    pub agents: Vec<AgentCoverage>,
    /// Findings, already in deterministic order.
    pub findings: Vec<Finding>,
    /// Per-category rollups, in `Category::ALL` order, skipping empty ones.
    pub categories: Vec<CategoryScore>,
    pub counts: Counts,
    /// Overall 0-100 score, or `None` when nothing scoreable ran.
    pub score: Option<u32>,
    /// Number of checks suppressed by `--skip` / `--only`.
    pub skipped: usize,
}

/// The most recent date on which a cited tool behavior in this build was read
/// from its official documentation. Each source also carries its own date.
pub const SOURCES_READ: &str = "2026-11-12";

/// The framing that must accompany every report, in both output modes.
pub const CAVEAT: &str = "Agent loading behavior changes fast. agentsdoc encodes what the cited \
official docs said on the date shown with each source (most recently 2026-11-12); verify against your tool's current documentation before acting \
on a finding. Behaviors agentsdoc could not confirm from an official doc are reported as INFO and \
labeled unverified, never as FAIL.";

impl Report {
    /// Build a report from findings, computing the rollups. `findings` is
    /// sorted here so callers cannot accidentally emit unstable output.
    pub fn new(
        root_name: String,
        files: Vec<FileSummary>,
        agents: Vec<AgentCoverage>,
        mut findings: Vec<Finding>,
        skipped: usize,
    ) -> Report {
        // Deterministic ordering: category order first, then check id. Severity
        // is deliberately not part of the key so that adding a finding never
        // reshuffles unrelated lines.
        findings.sort_by(|a, b| {
            category_rank(a.category)
                .cmp(&category_rank(b.category))
                .then_with(|| a.id.cmp(b.id))
        });

        let mut counts = Counts::default();
        for f in &findings {
            counts.add(f.severity);
        }

        let mut categories = Vec::new();
        for category in Category::ALL {
            let in_cat: Vec<&Finding> =
                findings.iter().filter(|f| f.category == category).collect();
            if in_cat.is_empty() {
                continue;
            }
            let mut cat_counts = Counts::default();
            for f in &in_cat {
                cat_counts.add(f.severity);
            }
            categories.push(CategoryScore {
                category,
                score: score_of(in_cat.into_iter()),
                counts: cat_counts,
            });
        }

        let score = score_of(findings.iter());

        Report {
            root_name,
            files,
            agents,
            findings,
            categories,
            counts,
            score,
            skipped,
        }
    }

    /// Letter grade derived from the overall score.
    pub fn grade(&self) -> &'static str {
        match self.score {
            None => "n/a",
            Some(s) if s >= 90 => "A",
            Some(s) if s >= 75 => "B",
            Some(s) if s >= 60 => "C",
            Some(s) if s >= 40 => "D",
            Some(_) => "F",
        }
    }

    /// Sum of the estimated token cost of every discovered instruction file.
    pub fn total_est_tokens(&self) -> usize {
        self.files.iter().map(|f| f.est_tokens).sum()
    }

    /// True when at least one check failed.
    pub fn has_fail(&self) -> bool {
        self.counts.fail > 0
    }

    /// True when at least one check warned.
    pub fn has_warn(&self) -> bool {
        self.counts.warn > 0
    }

    /// Process exit code: 0 clean, 1 when a FAIL (or, under `--strict`, a WARN)
    /// is present. Usage and I/O errors exit 2 from `main`.
    pub fn exit_code(&self, strict: bool) -> i32 {
        if self.has_fail() || (strict && self.has_warn()) {
            1
        } else {
            0
        }
    }
}

fn category_rank(c: Category) -> usize {
    Category::ALL
        .iter()
        .position(|x| *x == c)
        .unwrap_or(usize::MAX)
}

/// Weighted score over an iterator of findings, rounded to the nearest integer.
fn score_of<'a>(findings: impl Iterator<Item = &'a Finding>) -> Option<u32> {
    let mut earned = 0.0;
    let mut total = 0.0;
    for f in findings {
        let w = f.weight();
        total += w;
        earned += w * f.severity.credit();
    }
    if total <= 0.0 {
        return None;
    }
    Some((earned / total * 100.0).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(id: &'static str, category: Category, severity: Severity) -> Finding {
        Finding {
            id,
            category,
            severity,
            title: String::new(),
            evidence: Vec::new(),
            why: "",
            source: "",
            fix: "",
        }
    }

    fn report(findings: Vec<Finding>) -> Report {
        Report::new("x".into(), Vec::new(), Vec::new(), findings, 0)
    }

    #[test]
    fn info_findings_do_not_move_the_score() {
        let all_pass = report(vec![finding("a-x", Category::Content, Severity::Pass)]);
        let with_info = report(vec![
            finding("a-x", Category::Content, Severity::Pass),
            finding("a-y", Category::Discovery, Severity::Info),
        ]);
        assert_eq!(all_pass.score, Some(100));
        assert_eq!(with_info.score, Some(100));
    }

    #[test]
    fn an_all_info_category_has_no_score() {
        let r = report(vec![finding("a-y", Category::Discovery, Severity::Info)]);
        assert_eq!(r.categories.len(), 1);
        assert_eq!(r.categories[0].score, None);
        assert_eq!(r.score, None);
        assert_eq!(r.grade(), "n/a");
    }

    #[test]
    fn warn_is_half_credit() {
        let r = report(vec![
            finding("a-x", Category::Content, Severity::Pass),
            finding("a-y", Category::Content, Severity::Warn),
        ]);
        assert_eq!(r.score, Some(75));
        assert_eq!(r.grade(), "B");
    }

    #[test]
    fn headline_portability_checks_count_double() {
        let r = report(vec![
            finding(
                "portability-claude-code",
                Category::Portability,
                Severity::Fail,
            ),
            finding("content-size", Category::Content, Severity::Pass),
        ]);
        // 1 of 3 weighted points earned.
        assert_eq!(r.score, Some(33));
    }

    #[test]
    fn exit_codes_follow_the_contract() {
        let warn = report(vec![finding("a-x", Category::Content, Severity::Warn)]);
        assert_eq!(warn.exit_code(false), 0);
        assert_eq!(warn.exit_code(true), 1);

        let fail = report(vec![finding("a-x", Category::Content, Severity::Fail)]);
        assert_eq!(fail.exit_code(false), 1);
        assert_eq!(fail.exit_code(true), 1);
    }

    #[test]
    fn findings_sort_by_category_then_id() {
        let r = report(vec![
            finding("z-last", Category::Content, Severity::Pass),
            finding("b-two", Category::Discovery, Severity::Pass),
            finding("a-one", Category::Discovery, Severity::Pass),
            finding("m-mid", Category::Portability, Severity::Pass),
        ]);
        let ids: Vec<&str> = r.findings.iter().map(|f| f.id).collect();
        assert_eq!(ids, ["a-one", "b-two", "m-mid", "z-last"]);
    }

    #[test]
    fn category_parse_accepts_aliases() {
        assert_eq!(Category::parse("PORT"), Some(Category::Portability));
        assert_eq!(Category::parse(" content "), Some(Category::Content));
        assert_eq!(Category::parse("nope"), None);
    }
}
