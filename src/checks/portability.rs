//! Portability: which well-known agents would actually find project
//! instructions in this repository.
//!
//! This is the check the tool exists for. The headline case is a repository
//! that has an AGENTS.md and no CLAUDE.md: every AGENTS.md consumer is fully
//! configured, and Claude Code loads no project instructions at all. The
//! Claude Code memory doc states the behavior outright -- "Claude Code reads
//! CLAUDE.md, not AGENTS.md" -- which is what makes it a FAIL rather than an
//! opinion. The mirror-image case (a CLAUDE.md-only repository, invisible to
//! Codex and Cursor) is graded the same way, from the same kind of citation.
//!
//! An agent whose behavior agentsdoc could not confirm from an official
//! document never produces anything above INFO.

use crate::agents::{self, Agent};
use crate::model::{Category, Finding, Severity};
use crate::scan::{Kind, Workspace};

/// Run the portability check for one agent.
pub fn for_agent(a: &'static Agent, ws: &Workspace) -> Finding {
    let covered = agents::coverage(a.id, ws);

    let (severity, title, evidence, fix) = if !covered.is_empty() {
        let severity = if a.verified {
            Severity::Pass
        } else {
            // A file matching an unverified convention is a candidate, not a
            // confirmed load.
            Severity::Info
        };
        let title = if a.verified {
            format!(
                "{} loads {} file(s) from this repository",
                a.name,
                covered.len()
            )
        } else {
            format!(
                "{} has {} candidate file(s) here; loading behavior unverified",
                a.name,
                covered.len()
            )
        };
        let evidence = covered
            .iter()
            .map(|f| format!("{}  (~{} tokens est.)", f.path, f.est_tokens()))
            .collect();
        (severity, title, evidence, a.fix)
    } else if ws.is_empty() {
        // No instruction files at all is a legitimate state. It is reported
        // once, as context, and never graded.
        (
            Severity::Info,
            format!(
                "no project instructions in this repository; nothing for {} to load",
                a.name
            ),
            Vec::new(),
            "Nothing to fix. A repository with no agent instructions is a normal state; add one \
             only when you have something worth saying in every session.",
        )
    } else if a.id == "cursor" && ws.of_kind(Kind::CursorLegacy).next().is_some() {
        // .cursorrules is absent from the current Cursor rules doc. agentsdoc
        // will not assert that it is ignored, so this is a WARN, not a FAIL.
        (
            Severity::Warn,
            "only a legacy .cursorrules is present; the current Cursor rules doc does not list it"
                .to_string(),
            ws.of_kind(Kind::CursorLegacy)
                .map(|f| f.path.clone())
                .collect(),
            "Migrate .cursorrules to .cursor/rules/*.mdc or to AGENTS.md, both of which the \
             current Cursor doc describes. Whether .cursorrules is still honored is unverified.",
        )
    } else {
        let severity = if a.verified {
            a.missing
        } else {
            Severity::Info
        };
        let title = match (a.verified, severity) {
            // An agent that documents "loads nothing unless told to" is not
            // failing anything; saying so plainly avoids implying a defect.
            (true, Severity::Info) => format!(
                "{} loads nothing by default and this repository does not wire it up",
                a.name
            ),
            (true, _) => format!(
                "{} would load no project instructions, but this repository has {} file(s)",
                a.name,
                ws.files().len()
            ),
            (false, _) => format!(
                "{} finds no matching file; behavior unverified, so this is context only",
                a.name
            ),
        };
        let evidence = ws
            .files()
            .iter()
            .map(|f| format!("{}  (present, not loaded by {})", f.path, a.name))
            .collect();
        (severity, title, evidence, a.fix)
    };

    Finding {
        id: a.check_id,
        category: Category::Portability,
        severity,
        title,
        evidence,
        why: a.behavior,
        source: a.source,
        fix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::agent;

    fn run(id: &str, ws: &Workspace) -> Finding {
        for_agent(agent(id).unwrap(), ws)
    }

    #[test]
    fn agents_md_only_fails_for_claude_code_and_passes_for_codex() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "# rules\nRun the tests.\n")]);
        let claude = run("claude-code", &ws);
        assert_eq!(claude.severity, Severity::Fail);
        assert!(claude
            .source
            .contains("Claude Code reads CLAUDE.md, not AGENTS.md"));
        assert!(claude.evidence.iter().any(|e| e.starts_with("AGENTS.md")));

        assert_eq!(run("codex", &ws).severity, Severity::Pass);
        assert_eq!(run("cursor", &ws).severity, Severity::Pass);
        assert_eq!(run("copilot", &ws).severity, Severity::Pass);
    }

    #[test]
    fn claude_md_only_fails_for_the_agents_md_consumers() {
        let ws = Workspace::from_entries("x", [("CLAUDE.md", "# rules\n")]);
        assert_eq!(run("claude-code", &ws).severity, Severity::Pass);
        assert_eq!(run("codex", &ws).severity, Severity::Fail);
        assert_eq!(run("cursor", &ws).severity, Severity::Fail);
        // Copilot documents a root CLAUDE.md as an accepted alternative.
        assert_eq!(run("copilot", &ws).severity, Severity::Pass);
    }

    #[test]
    fn a_repository_with_no_instructions_is_never_graded_down() {
        let ws = Workspace::from_entries("x", [("README.md", "hello")]);
        for a in agents::AGENTS {
            let f = for_agent(a, &ws);
            assert_eq!(f.severity, Severity::Info, "{} should be INFO", a.id);
        }
    }

    #[test]
    fn gemini_is_a_warning_because_the_file_name_is_configurable() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "# rules\n")]);
        let f = run("gemini-cli", &ws);
        assert_eq!(f.severity, Severity::Warn);
        assert!(f.source.contains("context.fileName"));
    }

    #[test]
    fn aider_is_informational_until_a_config_names_a_file() {
        let bare = Workspace::from_entries("x", [("AGENTS.md", "# rules\n")]);
        assert_eq!(run("aider", &bare).severity, Severity::Info);

        let configured = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "# rules\n"),
                (".aider.conf.yml", "read: AGENTS.md\n"),
            ],
        );
        assert_eq!(run("aider", &configured).severity, Severity::Pass);
    }

    #[test]
    fn an_unverified_agent_never_exceeds_info() {
        let present = Workspace::from_entries("x", [(".windsurfrules", "# rules\n")]);
        let f = run("windsurf", &present);
        assert_eq!(f.severity, Severity::Info);
        assert!(f.title.contains("unverified"));

        let absent = Workspace::from_entries("x", [("AGENTS.md", "# rules\n")]);
        assert_eq!(run("windsurf", &absent).severity, Severity::Info);
    }

    #[test]
    fn legacy_cursorrules_warns_instead_of_failing() {
        let ws = Workspace::from_entries("x", [(".cursorrules", "# rules\n")]);
        let f = run("cursor", &ws);
        assert_eq!(f.severity, Severity::Warn);
        assert!(f.title.contains("legacy"));
    }
}
