//! Consistency: do the instruction files in this repository agree with each
//! other, and do the links between them resolve?

use std::collections::BTreeSet;

use crate::agents::CLAUDE_IMPORT_DEPTH;
use crate::checks::{Meta, Options};
use crate::model::{Category, Finding, Severity};
use crate::scan::{resolve, InstructionFile, Kind, Workspace};
use crate::text::{self, DUPLICATION_RATIO, MIN_SHARED_LINES};

/// Upper bound on how many files enter the pairwise comparisons. Instruction
/// files are normally counted in single digits; the cap only exists so a
/// pathological repository cannot turn an audit into an O(n^2) text diff.
const MAX_PAIRWISE_FILES: usize = 50;

/// Files whose text is worth comparing: real instruction prose that was read.
fn comparable(ws: &Workspace) -> Vec<&InstructionFile> {
    ws.files()
        .iter()
        .filter(|f| f.kind.is_instruction_text() && f.text.is_some())
        .take(MAX_PAIRWISE_FILES)
        .collect()
}

/// Whether `from` imports `to`.
fn imports_file(from: &InstructionFile, to: &InstructionFile) -> bool {
    text::imports(from.body())
        .into_iter()
        .any(|i| resolve(from.dir(), &i.path) == to.path)
}

const CLAUDE_IMPORT: Meta = Meta {
    id: "consistency-claude-import",
    category: Category::Consistency,
    why: "When a repository has both files, the CLAUDE.md is what Claude Code actually reads. If \
          it does not import AGENTS.md, the AGENTS.md content never reaches Claude Code, and the \
          two files become two sources of truth that drift apart.",
    source: "https://code.claude.com/docs/en/memory (read 2026-07-29): \"If your repository \
             already uses AGENTS.md for other coding agents, create a CLAUDE.md that imports it so \
             both tools read the same instructions without duplicating them.\" The doc gives \
             `@AGENTS.md` and `ln -s AGENTS.md CLAUDE.md` as the two recipes.",
    fix: "Replace CLAUDE.md's body with `@AGENTS.md`, keeping any Claude-specific instructions \
          below the import, or symlink CLAUDE.md to AGENTS.md.",
};

/// Is the repository's CLAUDE.md a thin import layer over AGENTS.md?
pub fn claude_import(ws: &Workspace, _opts: &Options) -> Finding {
    let Some(agents_md) = ws.root_of_kind(Kind::AgentsMd) else {
        return CLAUDE_IMPORT.finding(
            Severity::Info,
            "no root AGENTS.md, so there is no AGENTS.md / CLAUDE.md pair to reconcile",
            Vec::new(),
        );
    };
    let claude = ws
        .root_of_kind(Kind::ClaudeMd)
        .or_else(|| ws.root_of_kind(Kind::ClaudeDirMd));
    let Some(claude) = claude else {
        return CLAUDE_IMPORT.finding(
            Severity::Info,
            "AGENTS.md exists but no CLAUDE.md does; see portability-claude-code",
            vec![agents_md.path.clone()],
        );
    };

    if claude.symlink {
        return CLAUDE_IMPORT.finding(
            Severity::Pass,
            format!("{} is a symlink, so it cannot drift", claude.path),
            vec![format!("{} -> symlink", claude.path)],
        );
    }

    if let Some(import) = text::imports(claude.body())
        .into_iter()
        .find(|i| resolve(claude.dir(), &i.path) == agents_md.path)
    {
        let extra = text::estimate_tokens(claude.body());
        return CLAUDE_IMPORT.finding(
            Severity::Pass,
            format!("{} imports {}", claude.path, agents_md.path),
            vec![format!(
                "{}:{}  @{}  (import layer is ~{} tokens est.)",
                claude.path, import.line, import.path, extra
            )],
        );
    }

    let a = text::significant_lines(agents_md.body());
    let b = text::significant_lines(claude.body());
    let o = text::overlap(&a, &b);
    let mut evidence = vec![format!(
        "{} does not import {} ({} of {} significant lines shared, {:.0}%)",
        claude.path,
        agents_md.path,
        o.shared,
        o.smaller,
        o.ratio * 100.0
    )];
    evidence.extend(
        o.samples
            .iter()
            .map(|s| format!("shared: {}", text::clip(s, 80))),
    );

    CLAUDE_IMPORT.finding(
        Severity::Warn,
        format!(
            "{} does not import {}; Claude Code never sees the AGENTS.md content",
            claude.path, agents_md.path
        ),
        evidence,
    )
}

const DUPLICATION: Meta = Meta {
    id: "consistency-duplication",
    category: Category::Consistency,
    why: "Two files that restate the same instructions are two files that have to be edited \
          together. In practice one of them gets updated and the other quietly keeps telling some \
          agent the old thing.",
    source: "https://code.claude.com/docs/en/memory (read 2026-07-29): \"create a CLAUDE.md that \
             imports it so both tools read the same instructions without duplicating them\", and \
             \"if two rules contradict each other, Claude may pick one arbitrarily\". The overlap \
             measure itself is agentsdoc's own; see the README.",
    fix: "Keep the content in one file and make the other import it (`@AGENTS.md`) or symlink to \
          it, leaving only tool-specific additions behind.",
};

/// Report file pairs that restate a large share of each other.
pub fn duplication(ws: &Workspace, _opts: &Options) -> Finding {
    let files = comparable(ws);
    if files.len() < 2 {
        return DUPLICATION.finding(
            Severity::Info,
            "fewer than two instruction files to compare",
            Vec::new(),
        );
    }

    let lines: Vec<Vec<String>> = files
        .iter()
        .map(|f| text::significant_lines(f.body()))
        .collect();

    let mut evidence = Vec::new();
    for i in 0..files.len() {
        for j in (i + 1)..files.len() {
            let (a, b) = (files[i], files[j]);
            // An import or a symlink is the recommended way to share content,
            // not duplication.
            if a.symlink || b.symlink || imports_file(a, b) || imports_file(b, a) {
                continue;
            }
            let o = text::overlap(&lines[i], &lines[j]);
            if o.shared < MIN_SHARED_LINES || o.ratio < DUPLICATION_RATIO {
                continue;
            }
            evidence.push(format!(
                "{} <-> {}: {} of {} significant lines shared ({:.0}% of the smaller file)",
                a.path,
                b.path,
                o.shared,
                o.smaller,
                o.ratio * 100.0
            ));
            evidence.extend(
                o.samples
                    .iter()
                    .map(|s| format!("  shared: {}", text::clip(s, 76))),
            );
        }
    }

    if evidence.is_empty() {
        return DUPLICATION.finding(
            Severity::Pass,
            format!(
                "no substantial duplication among {} instruction files",
                files.len()
            ),
            Vec::new(),
        );
    }

    let pairs = evidence
        .iter()
        .filter(|e| !e.starts_with("  shared:"))
        .count();
    DUPLICATION.finding(
        Severity::Warn,
        format!("{pairs} file pair(s) duplicate a substantial share of each other"),
        evidence,
    )
}

const CONTRADICTION: Meta = Meta {
    id: "consistency-contradiction",
    category: Category::Consistency,
    why: "When two instruction files give opposite directions for the same thing, the outcome \
          depends on which one the agent happens to weigh more, which is not something you can \
          rely on.",
    source: "https://code.claude.com/docs/en/memory (read 2026-07-29): \"if two rules contradict \
             each other, Claude may pick one arbitrarily\" and \"Look for conflicting instructions \
             across CLAUDE.md files.\" The detection itself is a deliberately narrow heuristic; \
             see the README.",
    fix: "Decide which instruction is correct, delete the other, and if both files must exist, \
          make one import the other so there is only one place to edit.",
};

/// Heuristic: the same directive stated with opposite polarity in two files.
pub fn contradiction(ws: &Workspace, _opts: &Options) -> Finding {
    let files = comparable(ws);
    if files.len() < 2 {
        return CONTRADICTION.finding(
            Severity::Info,
            "fewer than two instruction files to compare",
            Vec::new(),
        );
    }

    let directives: Vec<Vec<text::Directive>> =
        files.iter().map(|f| text::directives(f.body())).collect();

    let mut evidence = Vec::new();
    for i in 0..files.len() {
        for j in (i + 1)..files.len() {
            if files[i].symlink || files[j].symlink {
                continue;
            }
            for (x, y) in text::contradictions(&directives[i], &directives[j]) {
                let (px, py) = (
                    if x.negated { "never" } else { "always" },
                    if y.negated { "never" } else { "always" },
                );
                evidence.push(format!(
                    "{}:{} says \"{px} {}\" but {}:{} says \"{py} {}\"",
                    files[i].path,
                    x.line,
                    text::clip(&x.core, 60),
                    files[j].path,
                    y.line,
                    text::clip(&y.core, 60),
                ));
            }
        }
    }

    if evidence.is_empty() {
        return CONTRADICTION.finding(
            Severity::Pass,
            "no directly contradicting directives detected (heuristic)",
            Vec::new(),
        );
    }

    evidence.sort();
    CONTRADICTION.finding(
        Severity::Warn,
        format!(
            "{} directive(s) appear with opposite polarity in two files (heuristic)",
            evidence.len()
        ),
        evidence,
    )
}

const IMPORTS: Meta = Meta {
    id: "consistency-imports",
    category: Category::Consistency,
    why: "An @import that resolves to nothing is silently dropped: the instructions you thought \
          you had factored out are simply absent from every session.",
    source: "https://code.claude.com/docs/en/memory (read 2026-07-29): \"CLAUDE.md files can \
             import additional files using @path/to/import syntax\", \"Relative paths resolve \
             relative to the file containing the import\", and \"Imported files can recursively \
             import other files, with a maximum depth of four hops.\"",
    fix: "Fix the path, or delete the import. Remember that relative imports resolve against the \
          importing file, not the working directory; wrap a path in backticks to mention it \
          without importing it.",
};

/// Every `@path` import resolves, and no chain exceeds the documented depth.
///
/// The whole import graph is walked, not just the first hop: an instruction
/// file that imports a shared fragment which itself imports a missing file is
/// just as broken, and just as invisible.
pub fn broken_imports(ws: &Workspace, _opts: &Options) -> Finding {
    let mut total = 0usize;
    let mut broken = Vec::new();
    let mut external = Vec::new();
    let mut deep = Vec::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();

    // Breadth-first over the import graph, so `depth` is the hop count Claude
    // Code would be on when it reached this file.
    let mut frontier: Vec<String> = comparable(ws).iter().map(|f| f.path.clone()).collect();
    visited.extend(frontier.iter().cloned());
    let mut depth = 1usize;
    while !frontier.is_empty() && depth <= CLAUDE_IMPORT_DEPTH + 1 {
        let mut next = Vec::new();
        for from in &frontier {
            let dir = from.rfind('/').map(|i| &from[..i]).unwrap_or("");
            let Some(body) = ws.text(from) else { continue };
            for import in text::imports(body) {
                if depth == 1 {
                    total += 1;
                }
                if import.path.starts_with('~') {
                    external.push(format!(
                        "{from}:{}  @{}  (home-directory import; outside the repository)",
                        import.line, import.path
                    ));
                    continue;
                }
                let target = resolve(dir, &import.path);
                if target.is_empty() {
                    external.push(format!(
                        "{from}:{}  @{}  (resolves outside the repository root)",
                        import.line, import.path
                    ));
                    continue;
                }
                if !ws.exists(&target) {
                    broken.push(format!(
                        "{from}:{}  @{}  ->  {target}  (missing)",
                        import.line, import.path
                    ));
                    continue;
                }
                if depth > CLAUDE_IMPORT_DEPTH {
                    deep.push(format!(
                        "{from}:{}  @{}  (hop {depth}; Claude Code follows at most \
                         {CLAUDE_IMPORT_DEPTH})",
                        import.line, import.path
                    ));
                    continue;
                }
                if visited.insert(target.clone()) {
                    next.push(target);
                }
            }
        }
        frontier = next;
        depth += 1;
    }

    if total == 0 {
        return IMPORTS.finding(
            Severity::Info,
            "no @path imports in any instruction file",
            Vec::new(),
        );
    }
    broken.sort();
    broken.dedup();
    deep.sort();
    deep.dedup();
    external.sort();
    external.dedup();

    if !broken.is_empty() {
        let count = broken.len();
        broken.extend(deep);
        return IMPORTS.finding(
            Severity::Fail,
            format!("{count} @path import(s) resolve to nothing"),
            broken,
        );
    }
    if !deep.is_empty() {
        return IMPORTS.finding(
            Severity::Warn,
            format!("import chain deeper than the documented {CLAUDE_IMPORT_DEPTH} hops"),
            deep,
        );
    }
    IMPORTS.finding(
        Severity::Pass,
        format!("all {total} top-level @path import(s) resolve"),
        external,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thin_importing_claude_md_passes() {
        let ws = Workspace::from_entries(
            "x",
            [
                (
                    "AGENTS.md",
                    "# Rules\nRun the test suite before committing.\n",
                ),
                (
                    "CLAUDE.md",
                    "@AGENTS.md\n\n## Claude Code\nUse plan mode.\n",
                ),
            ],
        );
        let f = claude_import(&ws, &Options::default());
        assert_eq!(f.severity, Severity::Pass);
        assert!(f.evidence[0].starts_with("CLAUDE.md:1"));
    }

    #[test]
    fn a_symlinked_claude_md_passes() {
        let ws = Workspace::from_entries(
            "x",
            [("AGENTS.md", "# Rules\n"), ("CLAUDE.md", "# Rules\n")],
        )
        .with_symlink("CLAUDE.md");
        assert_eq!(
            claude_import(&ws, &Options::default()).severity,
            Severity::Pass
        );
    }

    #[test]
    fn a_duplicating_claude_md_warns_on_both_checks() {
        let body: String = (0..12)
            .map(|i| format!("- Always run the {i} verification step before merging\n"))
            .collect();
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", format!("# Rules\n{body}")),
                ("CLAUDE.md", format!("# Rules\n{body}")),
            ],
        );
        let imp = claude_import(&ws, &Options::default());
        assert_eq!(imp.severity, Severity::Warn);
        assert!(imp.title.contains("never sees"));

        let dup = duplication(&ws, &Options::default());
        assert_eq!(dup.severity, Severity::Warn);
        assert!(dup.evidence[0].contains("AGENTS.md <-> CLAUDE.md"));
        assert!(dup.evidence[0].contains("100%"));
    }

    #[test]
    fn an_import_relationship_is_not_counted_as_duplication() {
        let body: String = (0..12)
            .map(|i| format!("- Always run the {i} verification step before merging\n"))
            .collect();
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", format!("# Rules\n{body}")),
                ("CLAUDE.md", format!("@AGENTS.md\n{body}")),
            ],
        );
        assert_eq!(
            duplication(&ws, &Options::default()).severity,
            Severity::Pass
        );
    }

    #[test]
    fn unrelated_files_do_not_trip_duplication() {
        let ws = Workspace::from_entries(
            "x",
            [
                (
                    "AGENTS.md",
                    "- Always run cargo test before committing anything\n",
                ),
                (
                    "GEMINI.md",
                    "- Prefer descriptive variable names in this project\n",
                ),
            ],
        );
        assert_eq!(
            duplication(&ws, &Options::default()).severity,
            Severity::Pass
        );
    }

    #[test]
    fn contradictions_are_detected_between_files() {
        let ws = Workspace::from_entries(
            "x",
            [
                (
                    "AGENTS.md",
                    "- Always use two-space indentation in this repo\n",
                ),
                (
                    "CLAUDE.md",
                    "- Never use two-space indentation in this repo\n",
                ),
            ],
        );
        let f = contradiction(&ws, &Options::default());
        assert_eq!(f.severity, Severity::Warn);
        assert!(f.evidence[0].contains("AGENTS.md:1"));
        assert!(f.evidence[0].contains("CLAUDE.md:1"));
    }

    #[test]
    fn agreeing_files_pass_the_contradiction_check() {
        let ws = Workspace::from_entries(
            "x",
            [
                (
                    "AGENTS.md",
                    "- Always use two-space indentation in this repo\n",
                ),
                (
                    "CLAUDE.md",
                    "- Always use two-space indentation in this repo\n",
                ),
            ],
        );
        assert_eq!(
            contradiction(&ws, &Options::default()).severity,
            Severity::Pass
        );
    }

    #[test]
    fn a_broken_import_fails() {
        let ws = Workspace::from_entries("x", [("CLAUDE.md", "@docs/missing.md\n")]);
        let f = broken_imports(&ws, &Options::default());
        assert_eq!(f.severity, Severity::Fail);
        assert!(f.evidence[0].contains("docs/missing.md"));
        assert!(f.evidence[0].ends_with("(missing)"));
    }

    #[test]
    fn imports_resolve_relative_to_the_importing_file() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("api/CLAUDE.md", "@rules.md\n"),
                ("api/rules.md", "# api rules\n"),
            ],
        );
        assert_eq!(
            broken_imports(&ws, &Options::default()).severity,
            Severity::Pass
        );
    }

    #[test]
    fn a_home_directory_import_is_reported_but_not_broken() {
        let ws = Workspace::from_entries("x", [("CLAUDE.md", "@~/.claude/mine.md\n")]);
        let f = broken_imports(&ws, &Options::default());
        assert_eq!(f.severity, Severity::Pass);
        assert!(f.evidence[0].contains("home-directory import"));
    }

    #[test]
    fn no_imports_is_informational() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "# Rules\n")]);
        assert_eq!(
            broken_imports(&ws, &Options::default()).severity,
            Severity::Info
        );
    }

    #[test]
    fn an_over_deep_import_chain_warns() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("CLAUDE.md", "@a.md\n"),
                ("a.md", "@b.md\n"),
                ("b.md", "@c.md\n"),
                ("c.md", "@d.md\n"),
                ("d.md", "@e.md\n"),
                ("e.md", "end\n"),
            ],
        );
        let f = broken_imports(&ws, &Options::default());
        assert_eq!(f.severity, Severity::Warn);
        assert!(f.evidence[0].contains("hop 5"));
    }
}
