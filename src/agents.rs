//! The agent registry: which well-known coding agents load which files, and
//! the official documentation that says so.
//!
//! Every entry carries the URL agentsdoc actually read and the date it read it.
//! Tool loading behavior changes fast, and a check that asserts "tool X ignores
//! file Y" without a citation is worse than no check at all. An agent whose
//! behavior could not be confirmed from an official doc is marked
//! [`Agent::verified`] `= false`, is never allowed to produce a FAIL, and says
//! "unverified" in its finding.

use crate::model::Severity;
use crate::scan::{InstructionFile, Kind, Workspace};
use crate::text;

/// Maximum import hops Claude Code follows.
///
/// Source: <https://code.claude.com/docs/en/memory> (read 2026-07-29) --
/// "Imported files can recursively import other files, with a maximum depth of
/// four hops."
pub const CLAUDE_IMPORT_DEPTH: usize = 4;

/// One agent's documented project-instruction behavior.
#[derive(Debug, Clone, Copy)]
pub struct Agent {
    /// Stable id, used in JSON output and in `--effective`.
    pub id: &'static str,
    /// The portability check id for this agent, also its `--skip` value.
    pub check_id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// False when agentsdoc could not confirm the behavior from an official doc.
    pub verified: bool,
    /// The documentation URL agentsdoc read, with the date.
    pub source: &'static str,
    /// One-line summary of the documented behavior.
    pub behavior: &'static str,
    /// Severity used when the repository has instruction files but this agent
    /// would load none of them.
    pub missing: Severity,
    /// What to do about it.
    pub fix: &'static str,
}

/// Every agent agentsdoc knows about, in report order.
pub const AGENTS: &[Agent] = &[
    Agent {
        id: "claude-code",
        check_id: "portability-claude-code",
        name: "Claude Code",
        verified: true,
        source: "https://code.claude.com/docs/en/memory (read 2026-07-29): \
                 \"Claude Code reads CLAUDE.md, not AGENTS.md.\"",
        behavior: "loads CLAUDE.md, .claude/CLAUDE.md, .claude/rules/*.md and CLAUDE.local.md, \
                   walking up from the working directory; AGENTS.md is not read natively",
        missing: Severity::Fail,
        fix: "Add a CLAUDE.md at the repository root whose first line is `@AGENTS.md`, or symlink \
              CLAUDE.md to AGENTS.md. The Claude Code memory doc gives both recipes.",
    },
    Agent {
        id: "codex",
        check_id: "portability-codex",
        name: "OpenAI Codex (and other AGENTS.md consumers)",
        verified: true,
        source: "https://agents.md/ (read 2026-07-29): AGENTS.md is the open format these tools \
                 read, and \"agents automatically read the nearest file in the directory tree, so \
                 the closest one takes precedence\".",
        behavior: "loads the nearest AGENTS.md walking up from the working directory",
        missing: Severity::Fail,
        fix: "Add an AGENTS.md at the repository root. If CLAUDE.md already holds the content, \
              move it to AGENTS.md and leave a CLAUDE.md containing `@AGENTS.md`.",
    },
    Agent {
        id: "cursor",
        check_id: "portability-cursor",
        name: "Cursor",
        verified: true,
        source: "https://cursor.com/docs/context/rules (read 2026-07-29): \"Project rules live in \
                 .cursor/rules as .mdc files\" and \"Cursor supports AGENTS.md in the project root \
                 and subdirectories.\"",
        behavior: "loads .cursor/rules/*.mdc and AGENTS.md (root and subdirectories)",
        missing: Severity::Fail,
        fix: "Add an AGENTS.md at the repository root, or add project rules under .cursor/rules/.",
    },
    Agent {
        id: "copilot",
        check_id: "portability-copilot",
        name: "GitHub Copilot",
        verified: true,
        source: "https://docs.github.com/en/copilot/how-tos/configure-custom-instructions/\
                 add-repository-instructions (read 2026-07-29): repository instructions live in \
                 \".github/copilot-instructions.md\"; AGENTS.md is supported anywhere in the \
                 repository and \"the nearest AGENTS.md file in the directory tree will take \
                 precedence\"; CLAUDE.md or GEMINI.md in the repository root are accepted \
                 alternatives.",
        behavior: "loads .github/copilot-instructions.md, .github/instructions/*.instructions.md, \
                   AGENTS.md anywhere, or a root CLAUDE.md / GEMINI.md",
        missing: Severity::Fail,
        fix: "Add .github/copilot-instructions.md, or an AGENTS.md at the repository root.",
    },
    Agent {
        id: "gemini-cli",
        check_id: "portability-gemini-cli",
        name: "Gemini CLI",
        verified: true,
        source: "https://google-gemini.github.io/gemini-cli/docs/cli/gemini-md.html \
                 (read 2026-07-29): GEMINI.md is the default context file name, and \
                 `context.fileName` in settings.json can be set to another name or a list \
                 (the doc's own example is [\"AGENTS.md\", \"CONTEXT.md\", \"GEMINI.md\"]).",
        behavior: "loads GEMINI.md hierarchically by default; other names require \
                   context.fileName in settings.json, which lives outside the repository",
        // Not a FAIL: the file name is configurable per-user, so agentsdoc cannot
        // see from the repository alone whether this is actually broken.
        missing: Severity::Warn,
        fix: "Add a GEMINI.md at the repository root (a one-line `@AGENTS.md` import works, since \
              Gemini CLI supports @file.md imports), or tell your team to set context.fileName.",
    },
    Agent {
        id: "aider",
        check_id: "portability-aider",
        name: "aider",
        verified: true,
        source:
            "https://aider.chat/docs/usage/conventions.html (read 2026-07-29): conventions are \
                 loaded with `aider --read CONVENTIONS.md`, `/read`, or a `read:` key in \
                 .aider.conf.yml. The page documents no file that is loaded automatically.",
        behavior: "loads nothing automatically; a conventions file must be named on the command \
                   line or in .aider.conf.yml",
        // Never a FAIL: "loads nothing" is aider's documented default, not a
        // defect in the repository.
        missing: Severity::Info,
        fix: "Optional: commit a .aider.conf.yml with `read: AGENTS.md` so contributors using \
              aider pick up the same instructions without extra flags.",
    },
    Agent {
        id: "windsurf",
        check_id: "portability-windsurf",
        name: "Windsurf",
        verified: false,
        source: "unverified - agentsdoc found no official Windsurf documentation it could cite on \
                 2026-07-29. Check your tool's current docs.",
        behavior: "commonly documented in the wild as reading .windsurfrules or \
                   .windsurf/rules/*.md; agentsdoc does not assert this",
        missing: Severity::Info,
        fix: "Verify against the current Windsurf documentation before adding or removing files \
              on agentsdoc's account.",
    },
];

/// Look up an agent by id.
pub fn agent(id: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|a| a.id == id)
}

/// Whether `agent_id` would load `file` as project instructions.
///
/// Only the *repository-visible* sources count. User-scope files
/// (`~/.claude/CLAUDE.md`, `~/.gemini/GEMINI.md`) and machine-local settings are
/// deliberately out of scope: they are not part of the repository and cannot be
/// relied on by a contributor who just cloned it.
pub fn loads(agent_id: &str, file: &InstructionFile) -> bool {
    let at_root = file.dir().is_empty();
    match agent_id {
        "claude-code" => matches!(
            file.kind,
            Kind::ClaudeMd | Kind::ClaudeDirMd | Kind::ClaudeLocalMd | Kind::ClaudeRule
        ),
        "codex" => file.kind == Kind::AgentsMd,
        "cursor" => matches!(file.kind, Kind::AgentsMd | Kind::CursorRule),
        "copilot" => match file.kind {
            Kind::AgentsMd | Kind::CopilotRepo | Kind::CopilotPath => true,
            // The doc offers root CLAUDE.md / GEMINI.md as alternatives; nested
            // ones are not mentioned, so they are not counted.
            Kind::ClaudeMd | Kind::GeminiMd => at_root,
            _ => false,
        },
        "gemini-cli" => file.kind == Kind::GeminiMd,
        // aider is handled by `aider_reads`, because what it loads is decided
        // by .aider.conf.yml rather than by the file's name.
        "aider" => false,
        "windsurf" => file.kind == Kind::WindsurfRules,
        _ => false,
    }
}

/// Files `agent_id` would load, sorted by path.
pub fn coverage<'a>(agent_id: &str, ws: &'a Workspace) -> Vec<&'a InstructionFile> {
    if agent_id == "aider" {
        return aider_reads(ws);
    }
    ws.files().iter().filter(|f| loads(agent_id, f)).collect()
}

/// Paths named by a `read:` key in `.aider.conf.yml`, resolved against the
/// repository root, keeping only the ones that exist.
pub fn aider_reads(ws: &Workspace) -> Vec<&InstructionFile> {
    let Some(conf) = ws.of_kind(Kind::AiderConfig).next() else {
        return Vec::new();
    };
    read_key(conf.body())
        .into_iter()
        .filter_map(|p| ws.file(&p))
        .collect()
}

/// Values of a top-level `read:` key in a minimal YAML document.
///
/// Handles `read: FILE`, `read: [a, b]` and a block sequence of `- item` lines.
/// A real YAML parser would be more correct, but `.aider.conf.yml` is a flat
/// key/value file and this is the only key agentsdoc looks at.
pub fn read_key(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in yaml.lines() {
        let trimmed = line.trim_end();
        if in_block {
            let t = trimmed.trim_start();
            if let Some(item) = t.strip_prefix("- ") {
                out.push(unquote(item));
                continue;
            }
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            in_block = false;
        }
        // Only top-level keys: indented keys belong to some other mapping.
        if trimmed.starts_with(char::is_whitespace) {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("read:") else {
            continue;
        };
        let rest = rest.trim();
        if rest.is_empty() {
            in_block = true;
        } else if let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            out.extend(inner.split(',').map(unquote).filter(|s| !s.is_empty()));
        } else {
            out.push(unquote(rest));
        }
    }
    out.retain(|s| !s.is_empty());
    out
}

fn unquote(s: &str) -> String {
    s.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

/// One entry in an effective instruction set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// Repository-relative path, or a synthetic label for out-of-repo scopes.
    pub path: String,
    /// Why this file is in the list, and any caveat about its position.
    pub note: String,
    /// Estimated tokens, `0` for entries that are not files in this repository.
    pub est_tokens: usize,
    /// True when the file only loads under a condition agentsdoc cannot see
    /// (path-scoped rules, frontmatter globs, on-demand subdirectory loads).
    pub conditional: bool,
}

/// What one agent would assemble for a given subdirectory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub agent: &'static str,
    pub name: &'static str,
    pub verified: bool,
    /// Ordered: earliest-loaded first.
    pub layers: Vec<Layer>,
    /// Sum over unconditional layers.
    pub est_tokens: usize,
    /// Ordering caveats and out-of-repo scopes.
    pub notes: Vec<String>,
}

/// Assemble the effective instruction set every known agent would load when
/// working in `dir` (repository-relative, `""` for the root).
pub fn effective_all(ws: &Workspace, dir: &str) -> Vec<Effective> {
    AGENTS.iter().map(|a| effective(a, ws, dir)).collect()
}

/// Assemble the effective instruction set for one agent.
pub fn effective(a: &'static Agent, ws: &Workspace, dir: &str) -> Effective {
    let mut layers = Vec::new();
    let mut notes = Vec::new();

    match a.id {
        "claude-code" => {
            notes.push(
                "Order is filesystem root down to the working directory, so the nearest file is \
                 read last (code.claude.com/docs/en/memory)."
                    .to_string(),
            );
            notes.push(
                "User and managed scopes (~/.claude/CLAUDE.md, the managed policy file) also load \
                 but live outside the repository and are not listed."
                    .to_string(),
            );
            let mut chain = crate::scan::ancestors(dir);
            chain.reverse();
            for cursor in chain {
                for (kind, why) in [
                    (Kind::ClaudeMd, "project memory"),
                    (Kind::ClaudeDirMd, "project memory (.claude/)"),
                ] {
                    if let Some(f) = ws.of_kind(kind).find(|f| owner_dir(f) == cursor) {
                        push_with_imports(&mut layers, ws, f, why, false);
                    }
                }
                for f in ws
                    .of_kind(Kind::ClaudeRule)
                    .filter(|f| owner_dir(f) == cursor)
                {
                    let scoped = has_paths_frontmatter(f.body());
                    let why = if scoped {
                        "rule, path-scoped (loads only for matching files)"
                    } else {
                        "rule, unconditional"
                    };
                    push_with_imports(&mut layers, ws, f, why, scoped);
                }
                if let Some(f) = ws
                    .of_kind(Kind::ClaudeLocalMd)
                    .find(|f| owner_dir(f) == cursor)
                {
                    push_with_imports(&mut layers, ws, f, "local memory (appended last)", false);
                }
            }
            if ws.root_of_kind(Kind::ClaudeMd).is_some()
                && ws.root_of_kind(Kind::ClaudeDirMd).is_some()
            {
                notes.push(
                    "The memory doc lists ./CLAUDE.md and ./.claude/CLAUDE.md as equivalent \
                     project locations without stating their relative order; agentsdoc lists \
                     ./CLAUDE.md first."
                        .to_string(),
                );
            }
        }
        "codex" => {
            notes.push(
                "agents.md: \"the closest one takes precedence\", so only the nearest AGENTS.md \
                 is listed as loaded."
                    .to_string(),
            );
            if let Some(f) = ws.nearest(Kind::AgentsMd, dir) {
                push_with_imports(&mut layers, ws, f, "nearest AGENTS.md", false);
                for other in ws.chain(Kind::AgentsMd, dir) {
                    if other.path != f.path {
                        notes.push(format!("{} is overridden by the nearer file.", other.path));
                    }
                }
            }
        }
        "cursor" => {
            for f in ws.of_kind(Kind::CursorRule) {
                let (why, conditional) = cursor_rule_note(f.body());
                push_with_imports(&mut layers, ws, f, why, conditional);
            }
            if let Some(f) = ws.nearest(Kind::AgentsMd, dir) {
                push_with_imports(&mut layers, ws, f, "nearest AGENTS.md", false);
            }
            if ws.of_kind(Kind::CursorLegacy).next().is_some() {
                notes.push(
                    ".cursorrules is present but the current Cursor rules doc does not list it; \
                     agentsdoc treats it as legacy and does not assume it is still read."
                        .to_string(),
                );
            }
        }
        "copilot" => {
            if let Some(f) = ws.of_kind(Kind::CopilotRepo).next() {
                push_with_imports(&mut layers, ws, f, "repository-wide instructions", false);
            }
            for f in ws.of_kind(Kind::CopilotPath) {
                push_with_imports(
                    &mut layers,
                    ws,
                    f,
                    "path-scoped (applyTo frontmatter decides)",
                    true,
                );
            }
            if let Some(f) = ws.nearest(Kind::AgentsMd, dir) {
                push_with_imports(&mut layers, ws, f, "nearest AGENTS.md", false);
            } else if let Some(f) = ws.root_of_kind(Kind::ClaudeMd) {
                push_with_imports(
                    &mut layers,
                    ws,
                    f,
                    "root CLAUDE.md (documented fallback)",
                    false,
                );
            } else if let Some(f) = ws.root_of_kind(Kind::GeminiMd) {
                push_with_imports(
                    &mut layers,
                    ws,
                    f,
                    "root GEMINI.md (documented fallback)",
                    false,
                );
            }
        }
        "gemini-cli" => {
            notes.push(
                "~/.gemini/GEMINI.md also loads but lives outside the repository and is not listed."
                    .to_string(),
            );
            let mut chain = crate::scan::ancestors(dir);
            chain.reverse();
            for cursor in chain {
                if let Some(f) = ws.of_kind(Kind::GeminiMd).find(|f| f.dir() == cursor) {
                    push_with_imports(&mut layers, ws, f, "hierarchical context file", false);
                }
            }
            if layers.is_empty() && ws.nearest(Kind::AgentsMd, dir).is_some() {
                notes.push(
                    "AGENTS.md is present; it loads only if context.fileName in settings.json \
                     names it."
                        .to_string(),
                );
            }
        }
        "aider" => {
            let reads = aider_reads(ws);
            if reads.is_empty() {
                notes.push(
                    "aider loads nothing automatically. Pass --read <file>, use /read, or set a \
                     `read:` key in .aider.conf.yml."
                        .to_string(),
                );
            }
            for f in reads {
                push_with_imports(
                    &mut layers,
                    ws,
                    f,
                    "named by read: in .aider.conf.yml",
                    false,
                );
            }
        }
        _ => {
            notes.push(format!(
                "{} behavior is unverified; anything listed here is a candidate, not a confirmed load.",
                a.name
            ));
            for f in coverage(a.id, ws) {
                push_with_imports(&mut layers, ws, f, "candidate (unverified)", true);
            }
        }
    }

    let est_tokens = layers
        .iter()
        .filter(|l| !l.conditional)
        .map(|l| l.est_tokens)
        .sum();

    Effective {
        agent: a.id,
        name: a.name,
        verified: a.verified,
        layers,
        est_tokens,
        notes,
    }
}

/// Directory a Claude memory file belongs to: `.claude/CLAUDE.md` and
/// `.claude/rules/x.md` both belong to the directory that contains `.claude`.
fn owner_dir(f: &InstructionFile) -> &str {
    let dir = f.dir();
    match dir.find(".claude") {
        Some(0) => "",
        Some(i) => dir[..i].trim_end_matches('/'),
        None => dir,
    }
}

/// Push a file plus everything it imports, depth-first, honoring the documented
/// four-hop limit.
fn push_with_imports(
    layers: &mut Vec<Layer>,
    ws: &Workspace,
    file: &InstructionFile,
    note: &str,
    conditional: bool,
) {
    layers.push(Layer {
        path: file.path.clone(),
        note: note.to_string(),
        est_tokens: file.est_tokens(),
        conditional,
    });
    push_imports(layers, ws, &file.path, file.body(), conditional, 1);
}

fn push_imports(
    layers: &mut Vec<Layer>,
    ws: &Workspace,
    path: &str,
    body: &str,
    conditional: bool,
    depth: usize,
) {
    if depth > CLAUDE_IMPORT_DEPTH {
        return;
    }
    let dir = path.rfind('/').map(|i| &path[..i]).unwrap_or("");
    for import in text::imports(body) {
        let target = crate::scan::resolve(dir, &import.path);
        if target.is_empty() || !ws.exists(&target) || layers.iter().any(|l| l.path == target) {
            continue;
        }
        // An imported file that is not itself an instruction file (a README, a
        // package.json) still expands into the context window at launch.
        let imported = ws.text(&target);
        layers.push(Layer {
            path: target.clone(),
            note: format!("imported by {path} (hop {depth})"),
            est_tokens: imported.map(text::estimate_tokens).unwrap_or(0),
            conditional,
        });
        if let Some(next) = imported {
            push_imports(layers, ws, &target, next, conditional, depth + 1);
        }
    }
}

/// Whether a `.claude/rules/*.md` file carries a `paths:` frontmatter key,
/// which makes it load only for matching files.
pub fn has_paths_frontmatter(text: &str) -> bool {
    frontmatter(text).is_some_and(|fm| {
        fm.lines()
            .any(|l| l.trim_start().starts_with("paths:") && !l.trim_start().starts_with('#'))
    })
}

/// How a Cursor rule loads, based on its frontmatter.
fn cursor_rule_note(text: &str) -> (&'static str, bool) {
    let Some(fm) = frontmatter(text) else {
        // "Plain .md files without frontmatter are ignored" per the Cursor doc,
        // so a bare .md in .cursor/rules is reported as conditional, not loaded.
        return ("no frontmatter (Cursor ignores plain .md rules)", true);
    };
    let has = |key: &str| {
        fm.lines()
            .any(|l| l.trim_start().to_ascii_lowercase().starts_with(key))
    };
    if fm
        .lines()
        .any(|l| l.trim().to_ascii_lowercase().replace(' ', "") == "alwaysapply:true")
    {
        ("alwaysApply: true", false)
    } else if has("globs:") {
        ("globs frontmatter (applies to matching files)", true)
    } else if has("description:") {
        ("description frontmatter (applied intelligently)", true)
    } else {
        ("frontmatter present, load condition unclear", true)
    }
}

/// The YAML frontmatter block of a Markdown document, if it has one.
pub fn frontmatter(text: &str) -> Option<&str> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_agent_id_is_unique_and_documented() {
        let mut seen = std::collections::BTreeSet::new();
        for a in AGENTS {
            assert!(seen.insert(a.id), "duplicate agent id {}", a.id);
            assert!(!a.source.is_empty(), "{} has no source", a.id);
            assert!(!a.behavior.is_empty(), "{} has no behavior", a.id);
            assert!(!a.fix.is_empty(), "{} has no fix", a.id);
            if !a.verified {
                assert_ne!(
                    a.missing,
                    Severity::Fail,
                    "{} is unverified and must never FAIL",
                    a.id
                );
                assert!(a.source.contains("unverified"));
            }
        }
    }

    #[test]
    fn claude_code_does_not_load_agents_md() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "# rules")]);
        assert!(coverage("claude-code", &ws).is_empty());
        assert_eq!(coverage("codex", &ws).len(), 1);
    }

    #[test]
    fn copilot_accepts_a_root_claude_md_but_not_a_nested_one() {
        let root = Workspace::from_entries("x", [("CLAUDE.md", "# rules")]);
        assert_eq!(coverage("copilot", &root).len(), 1);
        let nested = Workspace::from_entries("x", [("api/CLAUDE.md", "# rules")]);
        assert!(coverage("copilot", &nested).is_empty());
    }

    #[test]
    fn aider_reads_come_from_the_config() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "# rules"),
                (".aider.conf.yml", "read: AGENTS.md\n"),
            ],
        );
        let reads = coverage("aider", &ws);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].path, "AGENTS.md");

        let bare = Workspace::from_entries("x", [("AGENTS.md", "# rules")]);
        assert!(coverage("aider", &bare).is_empty());
    }

    #[test]
    fn read_key_parses_the_three_yaml_shapes() {
        assert_eq!(read_key("read: A.md\n"), ["A.md"]);
        assert_eq!(read_key("read: [A.md, \"B.md\"]\n"), ["A.md", "B.md"]);
        assert_eq!(
            read_key("read:\n  - A.md\n  - B.md\nmodel: x\n"),
            ["A.md", "B.md"]
        );
        assert!(read_key("model: gpt\n").is_empty());
        // An indented `read:` belongs to some other mapping.
        assert!(read_key("nested:\n  read: A.md\n").is_empty());
    }

    #[test]
    fn effective_claude_code_orders_root_first_and_expands_imports() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "# shared rules for everyone"),
                ("CLAUDE.md", "@AGENTS.md\n"),
                ("api/CLAUDE.md", "# api specific"),
                ("api/src/lib.rs", ""),
            ],
        );
        let eff = effective(agent("claude-code").unwrap(), &ws, "api/src");
        let paths: Vec<&str> = eff.layers.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, ["CLAUDE.md", "AGENTS.md", "api/CLAUDE.md"]);
    }

    #[test]
    fn effective_codex_takes_only_the_nearest() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "root"), ("api/AGENTS.md", "api")]);
        let eff = effective(agent("codex").unwrap(), &ws, "api");
        let paths: Vec<&str> = eff.layers.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, ["api/AGENTS.md"]);
        assert!(eff
            .notes
            .iter()
            .any(|n| n.contains("AGENTS.md is overridden")));
    }

    #[test]
    fn frontmatter_is_recognized() {
        assert_eq!(
            frontmatter("---\npaths:\n  - a\n---\nbody\n"),
            Some("paths:\n  - a")
        );
        assert_eq!(frontmatter("no frontmatter\n"), None);
        assert!(has_paths_frontmatter("---\npaths:\n  - \"src/**\"\n---\n"));
        assert!(!has_paths_frontmatter("---\ndescription: x\n---\n"));
    }

    #[test]
    fn cursor_rule_conditions() {
        assert!(!cursor_rule_note("---\nalwaysApply: true\n---\n").1);
        assert!(cursor_rule_note("---\nglobs: src/**\n---\n").1);
        assert!(cursor_rule_note("plain text\n").1);
    }

    #[test]
    fn owner_dir_lifts_dot_claude_paths() {
        let f = |p: &str| InstructionFile {
            path: p.to_string(),
            kind: Kind::ClaudeRule,
            bytes: 0,
            lines: 0,
            text: Some(String::new()),
            symlink: false,
        };
        assert_eq!(owner_dir(&f(".claude/rules/a.md")), "");
        assert_eq!(owner_dir(&f("api/.claude/rules/a.md")), "api");
        assert_eq!(owner_dir(&f("api/CLAUDE.md")), "api");
    }
}
