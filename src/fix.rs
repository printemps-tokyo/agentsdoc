//! `--fix`: make one root AGENTS.md the single source and point the other
//! tools at it, the arrangement the cited docs recommend.
//!
//! Planning is pure (a `Workspace` in, a list of changes out); `main.rs`
//! applies the plan. The fixer only ever does three things, all at the
//! repository root:
//!
//! - creates a one-line `@AGENTS.md` import as CLAUDE.md or GEMINI.md when
//!   AGENTS.md exists and the file does not (Claude Code and Gemini CLI both
//!   document `@file` imports);
//! - replaces a CLAUDE.md or GEMINI.md that is a copy of AGENTS.md with that
//!   import, so there is one copy to edit;
//! - when there is no AGENTS.md but a CLAUDE.md, moves the CLAUDE.md text into
//!   a new AGENTS.md and leaves the import behind (the Claude Code memory
//!   doc's recipe).
//!
//! It never touches a symlink, a file that differs from AGENTS.md, or anything
//! below the root.

use crate::scan::{Kind, Workspace};

/// The one-line import the created and replaced files hold.
pub const IMPORT: &str = "@AGENTS.md\n";

/// One file change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Repository-relative path.
    pub path: String,
    /// True when the file must not exist yet; false when it replaces a file
    /// whose current text is `expected`.
    pub create: bool,
    /// For a replacement: the text the file had when it was scanned. The
    /// change is skipped if the file no longer matches.
    pub expected: Option<String>,
    pub contents: String,
    pub why: &'static str,
}

/// Text with line endings and trailing whitespace normalized, for deciding
/// whether one file is a copy of another.
fn normalized(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Whether a file already imports AGENTS.md on a line of its own.
fn imports_agents(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim() == "@AGENTS.md" || l.trim() == "@./AGENTS.md")
}

/// Plan the changes for a workspace. An empty plan means nothing to do.
pub fn plan(ws: &Workspace) -> Vec<Change> {
    let mut changes = Vec::new();
    let agents = ws.file("AGENTS.md");
    let claude = ws.root_of_kind(Kind::ClaudeMd);
    let claude_dir = ws.file(".claude/CLAUDE.md");
    let gemini = ws.root_of_kind(Kind::GeminiMd);

    let agents_text = match agents {
        Some(a) => match &a.text {
            Some(t) => t.clone(),
            None => return changes, // unreadable or too large: leave it alone
        },
        None => {
            // No AGENTS.md: move a root CLAUDE.md's text into one.
            let Some(c) = claude.filter(|c| !c.symlink) else {
                return changes;
            };
            let Some(text) = c.text.clone() else {
                return changes;
            };
            if normalized(&text).is_empty() || imports_agents(&text) {
                return changes;
            }
            changes.push(Change {
                path: "AGENTS.md".to_string(),
                create: true,
                expected: None,
                contents: text.clone(),
                why: "moved from CLAUDE.md so every AGENTS.md consumer reads it",
            });
            changes.push(Change {
                path: c.path.clone(),
                create: false,
                expected: Some(text.clone()),
                contents: IMPORT.to_string(),
                why: "now imports AGENTS.md (Claude Code reads CLAUDE.md, not AGENTS.md)",
            });
            if gemini.is_none() {
                changes.push(gemini_import());
            }
            return changes;
        }
    };
    if agents.is_some_and(|a| a.symlink) {
        // AGENTS.md is itself a link (often to CLAUDE.md); importing it from
        // CLAUDE.md could make a loop, so leave this layout to the human.
        return changes;
    }

    // CLAUDE.md: create the import, or replace a copy with it.
    match claude {
        None if claude_dir.is_none() => changes.push(Change {
            path: "CLAUDE.md".to_string(),
            create: true,
            expected: None,
            contents: IMPORT.to_string(),
            why: "Claude Code reads CLAUDE.md, not AGENTS.md",
        }),
        Some(c) => {
            if let Some(text) = copy_of(c, &agents_text) {
                changes.push(Change {
                    path: c.path.clone(),
                    create: false,
                    expected: Some(text),
                    contents: IMPORT.to_string(),
                    why: "was a copy of AGENTS.md; now imports it, so there is one copy to edit",
                });
            }
        }
        None => {}
    }

    // GEMINI.md: the same, for Gemini CLI's default context file name.
    match gemini {
        None => changes.push(gemini_import()),
        Some(g) => {
            if let Some(text) = copy_of(g, &agents_text) {
                changes.push(Change {
                    path: g.path.clone(),
                    create: false,
                    expected: Some(text),
                    contents: IMPORT.to_string(),
                    why: "was a copy of AGENTS.md; now imports it, so there is one copy to edit",
                });
            }
        }
    }
    changes
}

fn gemini_import() -> Change {
    Change {
        path: "GEMINI.md".to_string(),
        create: true,
        expected: None,
        contents: IMPORT.to_string(),
        why: "Gemini CLI reads GEMINI.md by default and supports @file imports",
    }
}

/// The file's text when it is a plain copy of `agents_text` (not a symlink,
/// not already an import).
fn copy_of(file: &crate::scan::InstructionFile, agents_text: &str) -> Option<String> {
    if file.symlink {
        return None;
    }
    let text = file.text.as_ref()?;
    (!imports_agents(text) && normalized(text) == normalized(agents_text)).then(|| text.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(changes: &[Change]) -> Vec<(&str, bool)> {
        changes
            .iter()
            .map(|c| (c.path.as_str(), c.create))
            .collect()
    }

    #[test]
    fn agents_md_alone_gets_both_imports() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "# rules\nRun tests.\n")]);
        let plan = plan(&ws);
        assert_eq!(paths(&plan), vec![("CLAUDE.md", true), ("GEMINI.md", true)]);
        assert!(plan.iter().all(|c| c.contents == IMPORT));
    }

    #[test]
    fn copies_become_imports_and_different_files_are_left_alone() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "# rules\nRun tests.\n"),
                ("CLAUDE.md", "# rules  \r\nRun tests.\n\n"),
                ("GEMINI.md", "# something else\n"),
            ],
        );
        let plan = plan(&ws);
        assert_eq!(paths(&plan), vec![("CLAUDE.md", false)]);
        assert_eq!(
            plan[0].expected.as_deref(),
            Some("# rules  \r\nRun tests.\n\n")
        );
    }

    #[test]
    fn claude_md_alone_moves_into_agents_md() {
        let ws = Workspace::from_entries("x", [("CLAUDE.md", "# rules\nUse pnpm.\n")]);
        let plan = plan(&ws);
        assert_eq!(
            paths(&plan),
            vec![
                ("AGENTS.md", true),
                ("CLAUDE.md", false),
                ("GEMINI.md", true)
            ]
        );
        assert_eq!(plan[0].contents, "# rules\nUse pnpm.\n");
        assert_eq!(plan[1].contents, IMPORT);
    }

    #[test]
    fn an_already_synced_repository_needs_nothing() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "# rules\n"),
                ("CLAUDE.md", "@AGENTS.md\n"),
                ("GEMINI.md", "@AGENTS.md\n"),
            ],
        );
        assert!(plan(&ws).is_empty());
        let empty = Workspace::from_entries("x", [("README.md", "hi")]);
        assert!(plan(&empty).is_empty());
    }

    #[test]
    fn a_dot_claude_memory_counts_as_claude_md() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "# rules\n"),
                (".claude/CLAUDE.md", "@../AGENTS.md\n"),
            ],
        );
        assert_eq!(paths(&plan(&ws)), vec![("GEMINI.md", true)]);
    }
}
