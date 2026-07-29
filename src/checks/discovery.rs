//! Discovery: what instruction files exist, where, and how they layer.
//!
//! Both checks here are informational by design. Having no agent instruction
//! files is a perfectly ordinary state for a repository, and having nested ones
//! is a feature of the AGENTS.md spec, not a defect.

use crate::checks::{Meta, Options};
use crate::model::{Finding, Severity};
use crate::scan::{Kind, Workspace};

const FILES: Meta = Meta {
    id: "discovery-files",
    category: crate::model::Category::Discovery,
    why: "Every instruction file an agent loads is spent from the same context budget as your \
          actual code, so knowing what exists and what it costs is the first thing to establish.",
    source: "https://agents.md/ (read 2026-07-29) for the AGENTS.md convention; \
             https://code.claude.com/docs/en/memory (read 2026-07-29) for the CLAUDE.md family. \
             Token counts are estimates: chars / 4.",
    fix: "Nothing to fix. This is an inventory, not a verdict.",
};

/// Inventory every discovered instruction file.
pub fn files(ws: &Workspace, _opts: &Options) -> Finding {
    if ws.is_empty() {
        return FILES.finding(
            Severity::Info,
            "no agent instruction files found",
            Vec::new(),
        );
    }

    let total: usize = ws.files().iter().map(|f| f.est_tokens()).sum();
    let evidence = ws
        .files()
        .iter()
        .map(|f| {
            let unread = if f.text.is_none() {
                "  [not read: over the size limit]"
            } else {
                ""
            };
            let link = if f.symlink { "  [symlink]" } else { "" };
            format!(
                "{}  ({}, {} bytes, {} lines, ~{} tokens est.){link}{unread}",
                f.path,
                f.kind.as_str(),
                f.bytes,
                f.lines,
                f.est_tokens()
            )
        })
        .collect();

    FILES.finding(
        Severity::Info,
        format!(
            "{} instruction file(s), ~{} estimated tokens in total",
            ws.files().len(),
            total
        ),
        evidence,
    )
}

const NESTING: Meta = Meta {
    id: "discovery-nesting",
    category: crate::model::Category::Discovery,
    why: "In a monorepo the file that actually applies to a subdirectory is not always the one at \
          the root, and the two conventions layer differently: AGENTS.md consumers take the \
          nearest file only, while Claude Code concatenates the whole chain from the root down.",
    source:
        "https://agents.md/ (read 2026-07-29): \"Agents automatically read the nearest file in \
             the directory tree, so the closest one takes precedence.\" \
             https://code.claude.com/docs/en/memory (read 2026-07-29): \"All discovered files are \
             concatenated into context rather than overriding each other.\"",
    fix: "Nothing to fix. Use --effective <subdir> to see the assembled set for one directory.",
};

/// Surface the nesting tree so the effective layering is visible.
pub fn nesting(ws: &Workspace, _opts: &Options) -> Finding {
    let mut nested: Vec<&crate::scan::InstructionFile> = ws
        .files()
        .iter()
        .filter(|f| matches!(f.kind, Kind::AgentsMd | Kind::ClaudeMd))
        .collect();
    nested.sort_by(|a, b| a.path.cmp(&b.path));

    if nested.iter().all(|f| f.dir().is_empty()) {
        return NESTING.finding(
            Severity::Info,
            "no nested AGENTS.md / CLAUDE.md; precedence is flat",
            Vec::new(),
        );
    }

    let evidence = nested
        .iter()
        .map(|f| {
            let scope = if f.dir().is_empty() {
                "applies repository-wide".to_string()
            } else {
                format!(
                    "wins for {}/** ; concatenated after the root file for Claude Code",
                    f.dir()
                )
            };
            format!(
                "{}  ({}, ~{} tokens est.) - {scope}",
                f.path,
                f.kind.as_str(),
                f.est_tokens()
            )
        })
        .collect();

    let depth = nested
        .iter()
        .map(|f| f.path.matches('/').count())
        .max()
        .unwrap_or(0);
    NESTING.finding(
        Severity::Info,
        format!(
            "{} nested instruction file(s), deepest at {} level(s) below the root",
            nested.iter().filter(|f| !f.dir().is_empty()).count(),
            depth
        ),
        evidence,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_repository_is_informational() {
        let ws = Workspace::from_entries("x", [("README.md", "hello")]);
        let f = files(&ws, &Options::default());
        assert_eq!(f.severity, Severity::Info);
        assert!(f.title.contains("no agent instruction files"));
    }

    #[test]
    fn the_inventory_lists_every_file_with_its_estimate() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "abcd"),
                (".github/copilot-instructions.md", "abcdefgh"),
            ],
        );
        let f = files(&ws, &Options::default());
        assert_eq!(f.severity, Severity::Info);
        assert_eq!(f.evidence.len(), 2);
        assert!(f.title.contains("~3 estimated tokens"));
        assert!(f.evidence[0].starts_with(".github/copilot-instructions.md"));
    }

    #[test]
    fn nesting_is_flat_when_everything_is_at_the_root() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "a"), ("CLAUDE.md", "b")]);
        let f = nesting(&ws, &Options::default());
        assert!(f.title.contains("flat"));
        assert!(f.evidence.is_empty());
    }

    #[test]
    fn nesting_reports_the_deeper_file() {
        let ws =
            Workspace::from_entries("x", [("AGENTS.md", "a"), ("packages/api/AGENTS.md", "b")]);
        let f = nesting(&ws, &Options::default());
        assert_eq!(f.severity, Severity::Info);
        assert_eq!(f.evidence.len(), 2);
        assert!(f.evidence[1].contains("wins for packages/api/**"));
        assert!(f.title.contains("2 level(s)"));
    }
}
