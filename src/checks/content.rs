//! Content: size, the conventional sections, and whether the paths an
//! instruction file points at actually exist.
//!
//! These checks are advisory. The AGENTS.md spec is explicit that the format
//! has no required schema, so a missing section is a suggestion, never a defect.
//! The one exception is a relative link to a path the repository does not
//! contain: that is a concrete, verifiable error, and it fails.

use crate::checks::{Meta, Options, LINE_GUIDANCE};
use crate::model::{Category, Finding, Severity};
use crate::scan::{resolve, InstructionFile, Kind, Workspace};
use crate::text;

/// Files whose prose is worth inspecting.
fn instruction_texts(ws: &Workspace) -> Vec<&InstructionFile> {
    ws.files()
        .iter()
        .filter(|f| f.kind.is_instruction_text())
        .collect()
}

const SIZE: Meta = Meta {
    id: "content-size",
    category: Category::Content,
    why: "Instruction files are loaded at the start of every session and are spent from the same \
          context window as the code the agent is meant to read. A long file also gets followed \
          less reliably than a short one.",
    source: "https://code.claude.com/docs/en/memory (read 2026-07-29): \"target under 200 lines \
             per CLAUDE.md file. Longer files consume more context and reduce adherence.\" Token \
             figures are estimates (chars / 4), not a tokenizer count.",
    fix: "Move detail into path-scoped rules or skills that load on demand, or split the file and \
          keep only what has to be true in every session. Note that @path imports organize a file \
          without shrinking it: imported content still loads at launch.",
};

/// Flag instruction files over the token budget or the line guidance.
pub fn size(ws: &Workspace, opts: &Options) -> Finding {
    let files = instruction_texts(ws);
    if files.is_empty() {
        return SIZE.finding(
            Severity::Info,
            "no instruction files to measure",
            Vec::new(),
        );
    }

    let mut over = Vec::new();
    for f in &files {
        if f.text.is_none() {
            over.push(format!(
                "{}  ({} bytes, too large to read; well past every budget)",
                f.path, f.bytes
            ));
            continue;
        }
        let tokens = f.est_tokens();
        let mut reasons = Vec::new();
        if tokens > opts.token_budget {
            reasons.push(format!(
                "~{tokens} tokens est. > {} budget",
                opts.token_budget
            ));
        }
        if f.lines > LINE_GUIDANCE {
            reasons.push(format!("{} lines > {LINE_GUIDANCE} guidance", f.lines));
        }
        if !reasons.is_empty() {
            over.push(format!("{}  ({})", f.path, reasons.join("; ")));
        }
    }

    let total: usize = files.iter().map(|f| f.est_tokens()).sum();
    if over.is_empty() {
        return SIZE.finding(
            Severity::Pass,
            format!(
                "all {} instruction file(s) are within budget (~{total} tokens est. in total)",
                files.len()
            ),
            Vec::new(),
        );
    }

    SIZE.finding(
        Severity::Warn,
        format!(
            "{} instruction file(s) exceed the size budget (~{total} tokens est. in total)",
            over.len()
        ),
        over,
    )
}

const SECTIONS: Meta = Meta {
    id: "content-sections",
    category: Category::Content,
    why: "Build and test commands, code style and commit/PR conventions are the three things an \
          agent otherwise has to guess at, and guessing wrong is expensive in a way that is not \
          obvious until a pull request arrives.",
    source: "https://agents.md/ (read 2026-07-29): \"AGENTS.md is just standard Markdown. Use any \
             headings you like\" -- there is no required schema. The page lists build and test \
             commands, code style guidelines, testing instructions, security considerations and \
             commit / pull request guidelines as popular sections. These are recommended, not \
             required.",
    fix: "Add a short section for whichever topic is missing. One heading and two or three \
          concrete lines each is enough; do not pad the file to satisfy a checklist.",
};

/// Which file the section check reads, in preference order.
fn primary(ws: &Workspace) -> Option<&InstructionFile> {
    for kind in [
        Kind::AgentsMd,
        Kind::ClaudeMd,
        Kind::ClaudeDirMd,
        Kind::CopilotRepo,
        Kind::GeminiMd,
    ] {
        let found = ws
            .root_of_kind(kind)
            .or_else(|| ws.of_kind(kind).next())
            .filter(|f| f.text.is_some());
        if found.is_some() {
            return found;
        }
    }
    instruction_texts(ws).into_iter().find(|f| f.text.is_some())
}

/// One recommended topic, and how it is recognized.
struct Topic {
    name: &'static str,
    /// Substrings that identify the topic when they appear in a heading.
    headings: &'static [&'static str],
    /// Substrings that identify the topic anywhere in the body.
    body: &'static [&'static str],
}

const TOPICS: [Topic; 3] = [
    Topic {
        name: "build / test commands",
        headings: &[
            "build", "test", "command", "script", "setup", "develop", "run", "lint", "check", "ci",
        ],
        body: &[
            "npm test",
            "npm run",
            "yarn ",
            "pnpm ",
            "cargo test",
            "cargo build",
            "make test",
            "make build",
            "pytest",
            "go test",
            "bundle exec",
            "./gradlew",
            "mvn ",
            "composer ",
            "dotnet test",
            "just ",
            "task ",
        ],
    },
    Topic {
        name: "code style",
        headings: &["style", "convention", "format", "naming", "lint", "idiom"],
        body: &[
            "indent",
            "formatter",
            "prettier",
            "rustfmt",
            "eslint",
            "gofmt",
            "black",
            "ruff",
            "naming convention",
            "line length",
            "tabs or spaces",
        ],
    },
    Topic {
        name: "commit / PR conventions",
        headings: &["commit", "pull request", "pr ", "review", "branch", "git"],
        body: &[
            "conventional commit",
            "commit message",
            "pull request",
            "pr title",
            "branch nam",
            "squash",
            "sign-off",
            "signed-off-by",
        ],
    },
];

/// Report which recommended topics the primary instruction file omits.
pub fn sections(ws: &Workspace, _opts: &Options) -> Finding {
    let Some(file) = primary(ws) else {
        return SECTIONS.finding(
            Severity::Info,
            "no instruction file to inspect for recommended sections",
            Vec::new(),
        );
    };

    let heads = text::headings(file.body());
    let body_lower = file.body().to_ascii_lowercase();

    let mut present = Vec::new();
    let mut missing = Vec::new();
    for topic in &TOPICS {
        let hit = topic
            .headings
            .iter()
            .any(|k| heads.iter().any(|h| h.contains(k)))
            || topic.body.iter().any(|k| body_lower.contains(k));
        if hit {
            present.push(topic.name);
        } else {
            missing.push(topic.name);
        }
    }

    if missing.is_empty() {
        return SECTIONS.finding(
            Severity::Pass,
            format!("{} covers all three recommended topics", file.path),
            present
                .iter()
                .map(|p| format!("{}: {p}", file.path))
                .collect(),
        );
    }

    SECTIONS.finding(
        Severity::Warn,
        format!(
            "{} omits {} recommended (not required) topic(s)",
            file.path,
            missing.len()
        ),
        missing
            .iter()
            .map(|m| format!("{}: no {m} section found", file.path))
            .collect(),
    )
}

const LINKS: Meta = Meta {
    id: "content-links",
    category: Category::Content,
    why: "An instruction that points at a file which does not exist sends the agent looking for \
          something it will never find, and the reader has no way to tell whether the path moved \
          or the instruction was always wrong.",
    source: "Not a tool behavior: a relative link whose target is absent from the repository is \
             verifiably broken. Link targets are resolved against the directory of the file that \
             contains them, and gitignored targets are probed on disk before being reported.",
    fix: "Update the path, or drop the link. Links inside fenced code blocks are treated as \
          examples and are never checked.",
};

/// Every relative link in every instruction file resolves to a real path.
pub fn links(ws: &Workspace, _opts: &Options) -> Finding {
    let mut total = 0usize;
    let mut broken = Vec::new();

    for f in instruction_texts(ws) {
        for r in text::relative_links(f.body()) {
            total += 1;
            let target = resolve(f.dir(), &r.target);
            if target.is_empty() {
                broken.push(format!(
                    "{}:{}  {}  (resolves outside the repository root)",
                    f.path, r.line, r.target
                ));
                continue;
            }
            if !ws.exists(&target) {
                broken.push(format!(
                    "{}:{}  {}  ->  {target}  (missing)",
                    f.path, r.line, r.target
                ));
            }
        }
    }

    if total == 0 {
        return LINKS.finding(
            Severity::Info,
            "no relative links in any instruction file",
            Vec::new(),
        );
    }
    if broken.is_empty() {
        return LINKS.finding(
            Severity::Pass,
            format!("all {total} relative link(s) resolve"),
            Vec::new(),
        );
    }
    broken.sort();
    LINKS.finding(
        Severity::Fail,
        format!(
            "{} of {total} relative link(s) point at missing paths",
            broken.len()
        ),
        broken,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::DEFAULT_TOKEN_BUDGET;

    fn opts() -> Options {
        Options::default()
    }

    #[test]
    fn a_small_file_is_within_budget() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "# Rules\nBe brief.\n")]);
        let f = size(&ws, &opts());
        assert_eq!(f.severity, Severity::Pass);
    }

    #[test]
    fn an_oversized_file_warns_at_the_documented_threshold() {
        let big = "x".repeat(DEFAULT_TOKEN_BUDGET * 4 + 8);
        let ws = Workspace::from_entries("x", [("AGENTS.md", big)]);
        let f = size(&ws, &opts());
        assert_eq!(f.severity, Severity::Warn);
        assert!(f.evidence[0].contains("tokens est. > 4000 budget"));
    }

    #[test]
    fn the_line_guidance_fires_independently_of_the_token_budget() {
        let many_lines = "a\n".repeat(LINE_GUIDANCE + 1);
        let ws = Workspace::from_entries("x", [("CLAUDE.md", many_lines)]);
        let f = size(&ws, &opts());
        assert_eq!(f.severity, Severity::Warn);
        assert!(f.evidence[0].contains("201 lines > 200 guidance"));
    }

    #[test]
    fn the_token_budget_is_configurable() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "x".repeat(400))]);
        assert_eq!(size(&ws, &opts()).severity, Severity::Pass);
        let tight = Options {
            token_budget: 10,
            ..Options::default()
        };
        assert_eq!(size(&ws, &tight).severity, Severity::Warn);
    }

    #[test]
    fn all_three_topics_present_passes() {
        let ws = Workspace::from_entries(
            "x",
            [(
                "AGENTS.md",
                "# Project\n\n## Build and test\nRun `cargo test`.\n\n\
                 ## Code style\nUse rustfmt defaults.\n\n\
                 ## Commits\nConventional Commits, please.\n",
            )],
        );
        let f = sections(&ws, &opts());
        assert_eq!(f.severity, Severity::Pass);
    }

    #[test]
    fn missing_topics_warn_and_say_they_are_optional() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "# Project\n\nSome prose.\n")]);
        let f = sections(&ws, &opts());
        assert_eq!(f.severity, Severity::Warn);
        assert!(f.title.contains("recommended (not required)"));
        assert_eq!(f.evidence.len(), 3);
        assert!(f.source.contains("no required schema"));
    }

    #[test]
    fn sections_prefer_agents_md_over_other_files() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "# Project\n\nSome prose.\n"),
                ("GEMINI.md", "## Build\nRun `npm test`.\n## Style\nprettier\n## Commits\nconventional commit\n"),
            ],
        );
        assert!(sections(&ws, &opts()).title.starts_with("AGENTS.md"));
    }

    #[test]
    fn a_broken_relative_link_fails() {
        let ws = Workspace::from_entries(
            "x",
            [("AGENTS.md", "See [the guide](docs/guide.md) for details.\n")],
        );
        let f = links(&ws, &opts());
        assert_eq!(f.severity, Severity::Fail);
        assert!(f.evidence[0].contains("AGENTS.md:1"));
        assert!(f.evidence[0].ends_with("(missing)"));
    }

    #[test]
    fn a_resolvable_link_passes() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "See [the guide](docs/guide.md).\n"),
                ("docs/guide.md", "# Guide\n"),
            ],
        );
        assert_eq!(links(&ws, &opts()).severity, Severity::Pass);
    }

    #[test]
    fn links_resolve_against_the_containing_file() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("api/AGENTS.md", "See [notes](../docs/notes.md).\n"),
                ("docs/notes.md", "# Notes\n"),
            ],
        );
        assert_eq!(links(&ws, &opts()).severity, Severity::Pass);
    }

    #[test]
    fn links_in_code_fences_are_examples_not_references() {
        let ws = Workspace::from_entries("x", [("AGENTS.md", "```\n[example](nowhere.md)\n```\n")]);
        assert_eq!(links(&ws, &opts()).severity, Severity::Info);
    }

    #[test]
    fn urls_are_not_checked() {
        let ws = Workspace::from_entries(
            "x",
            [(
                "AGENTS.md",
                "See [spec](https://agents.md/) and [anchor](#top).\n",
            )],
        );
        assert_eq!(links(&ws, &opts()).severity, Severity::Info);
    }
}
