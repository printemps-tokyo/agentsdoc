//! A read-once, in-memory view of the instruction files in a repository.
//!
//! Every check runs against a [`Workspace`] rather than the filesystem, which
//! keeps the check engine pure (and lets tests build a repository literally, in
//! memory, without a tempdir). The walk itself is `ignore::WalkBuilder`, the
//! same gitignore-aware walker repotome and readyproof use, so a vendored
//! `node_modules/AGENTS.md` never shows up as one of *your* instruction files.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use ignore::WalkBuilder;

use crate::text;

/// Largest instruction file agentsdoc will read into memory. Anything past this
/// is recorded with its size but without text; the size check will flag it
/// long before the content checks would have had anything useful to say.
pub const MAX_READ_BYTES: u64 = 512 * 1024;

/// How many `@path` import hops the scanner follows when reading files that are
/// not themselves instruction files. One past Claude Code's documented limit of
/// four, so that a chain which exceeds the limit can still be reported.
pub const MAX_IMPORT_FOLLOW: usize = 5;

/// Which agent-instruction convention a file belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// `AGENTS.md`, at the root or in any subdirectory.
    AgentsMd,
    /// `CLAUDE.md`, at the root or in any subdirectory.
    ClaudeMd,
    /// `.claude/CLAUDE.md`.
    ClaudeDirMd,
    /// `CLAUDE.local.md`.
    ClaudeLocalMd,
    /// `.claude/rules/**/*.md`.
    ClaudeRule,
    /// `.cursorrules` (legacy; absent from the current Cursor rules doc).
    CursorLegacy,
    /// `.cursor/rules/**/*.mdc` (or `.md`).
    CursorRule,
    /// `.github/copilot-instructions.md`.
    CopilotRepo,
    /// `.github/instructions/**/*.instructions.md`.
    CopilotPath,
    /// `GEMINI.md`.
    GeminiMd,
    /// `.windsurfrules` or `.windsurf/rules/**/*.md`.
    WindsurfRules,
    /// `.clinerules`, or `.clinerules/**/*.md`.
    ClineRules,
    /// `.aider.conf.yml` / `.aider.conf.yaml`.
    AiderConfig,
    /// A root `CONVENTIONS.md`, the file aider's docs use as the example.
    AiderConventions,
}

impl Kind {
    /// Machine token used in JSON output and in the human inventory.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::AgentsMd => "agents-md",
            Kind::ClaudeMd => "claude-md",
            Kind::ClaudeDirMd => "claude-dir-md",
            Kind::ClaudeLocalMd => "claude-local-md",
            Kind::ClaudeRule => "claude-rule",
            Kind::CursorLegacy => "cursorrules-legacy",
            Kind::CursorRule => "cursor-rule",
            Kind::CopilotRepo => "copilot-instructions",
            Kind::CopilotPath => "copilot-path-instructions",
            Kind::GeminiMd => "gemini-md",
            Kind::WindsurfRules => "windsurf-rules",
            Kind::ClineRules => "cline-rules",
            Kind::AiderConfig => "aider-config",
            Kind::AiderConventions => "aider-conventions",
        }
    }

    /// True when the file is a set of instructions in its own right, as opposed
    /// to configuration that merely points at one.
    pub fn is_instruction_text(self) -> bool {
        !matches!(self, Kind::AiderConfig)
    }

    /// Classify a repository-relative path. `rel` uses forward slashes and is
    /// matched case-insensitively, because `Claude.md` and `agents.MD` both
    /// occur in the wild on case-insensitive filesystems.
    pub fn classify(rel: &str) -> Option<Kind> {
        let lower = rel.to_ascii_lowercase();
        let base = lower.rsplit('/').next().unwrap_or(&lower);
        let dir = lower.rfind('/').map(|i| &lower[..i]).unwrap_or("");
        let at_root = dir.is_empty();

        // Directory-scoped conventions first: their basenames are generic.
        if segment_prefix(&lower, ".claude/rules/") && lower.ends_with(".md") {
            return Some(Kind::ClaudeRule);
        }
        if segment_prefix(&lower, ".cursor/rules/")
            && (lower.ends_with(".mdc") || lower.ends_with(".md"))
        {
            return Some(Kind::CursorRule);
        }
        if lower.starts_with(".github/instructions/") && lower.ends_with(".instructions.md") {
            return Some(Kind::CopilotPath);
        }
        if segment_prefix(&lower, ".windsurf/rules/") && lower.ends_with(".md") {
            return Some(Kind::WindsurfRules);
        }
        if segment_prefix(&lower, ".clinerules/") && lower.ends_with(".md") {
            return Some(Kind::ClineRules);
        }
        if lower == ".github/copilot-instructions.md" {
            return Some(Kind::CopilotRepo);
        }

        match base {
            "agents.md" => Some(Kind::AgentsMd),
            "claude.md" if dir.ends_with(".claude") => Some(Kind::ClaudeDirMd),
            "claude.md" => Some(Kind::ClaudeMd),
            "claude.local.md" => Some(Kind::ClaudeLocalMd),
            "gemini.md" => Some(Kind::GeminiMd),
            ".cursorrules" => Some(Kind::CursorLegacy),
            ".windsurfrules" => Some(Kind::WindsurfRules),
            ".clinerules" => Some(Kind::ClineRules),
            ".aider.conf.yml" | ".aider.conf.yaml" => Some(Kind::AiderConfig),
            "conventions.md" if at_root => Some(Kind::AiderConventions),
            _ => None,
        }
    }
}

/// True when `path` contains `prefix` as a run of whole path segments starting
/// at the root or immediately after a `/`.
fn segment_prefix(path: &str, prefix: &str) -> bool {
    path.starts_with(prefix) || path.contains(&format!("/{prefix}"))
}

/// One discovered instruction file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionFile {
    /// Repository-relative path with forward slashes.
    pub path: String,
    pub kind: Kind,
    pub bytes: u64,
    /// Number of lines, `0` when the text was not read.
    pub lines: usize,
    /// File contents, or `None` when the file exceeded [`MAX_READ_BYTES`] or
    /// was not valid UTF-8.
    pub text: Option<String>,
    /// True when the path is a symbolic link.
    ///
    /// This matters: the Claude Code memory doc offers `ln -s AGENTS.md
    /// CLAUDE.md` as one of the two supported ways to make a single instruction
    /// file serve both tools, and a symlinked mirror cannot drift the way a
    /// copied one can.
    pub symlink: bool,
}

impl InstructionFile {
    /// Directory the file lives in, `""` for the repository root.
    pub fn dir(&self) -> &str {
        self.path.rfind('/').map(|i| &self.path[..i]).unwrap_or("")
    }

    /// Estimated tokens. `0` when the text was not read.
    pub fn est_tokens(&self) -> usize {
        self.text.as_deref().map(text::estimate_tokens).unwrap_or(0)
    }

    /// Text, or the empty string when it was not read.
    pub fn body(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }
}

/// An immutable view of a repository's agent instruction files.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    root_name: String,
    files: Vec<InstructionFile>,
    /// Every repository-relative path known to exist: files and directories.
    present: BTreeSet<String>,
    /// Text of every file agentsdoc has read: the instruction files, plus the
    /// transitive targets of their `@path` imports. An imported README or
    /// package.json is not an instruction file, but it does enter the context
    /// window, and its own imports have to be followed to measure chain depth.
    texts: BTreeMap<String, String>,
}

impl Workspace {
    /// Build a workspace directly from `(path, contents)` pairs.
    ///
    /// Paths that no convention recognizes are still recorded as existing, so
    /// reference checks can resolve links to ordinary source files.
    pub fn from_entries<I, P, C>(root_name: impl Into<String>, entries: I) -> Workspace
    where
        I: IntoIterator<Item = (P, C)>,
        P: Into<String>,
        C: Into<String>,
    {
        let mut ws = Workspace {
            root_name: root_name.into(),
            ..Workspace::default()
        };
        for (path, contents) in entries {
            let path = normalize(&path.into());
            if path.is_empty() {
                continue;
            }
            let body: String = contents.into();
            ws.record_present(&path);
            ws.texts.insert(path.clone(), body.clone());
            if let Some(kind) = Kind::classify(&path) {
                ws.files.push(InstructionFile {
                    path,
                    kind,
                    bytes: body.len() as u64,
                    lines: body.lines().count(),
                    text: Some(body),
                    symlink: false,
                });
            }
        }
        ws.finish();
        ws
    }

    /// Walk `root`, honoring .gitignore/.ignore and skipping `.git`.
    ///
    /// Hidden entries are included on purpose: `.claude/`, `.cursor/`,
    /// `.github/` and `.cursorrules` are the whole point of the walk.
    pub fn scan(root: &Path, root_name: impl Into<String>) -> io::Result<Workspace> {
        let mut ws = Workspace {
            root_name: root_name.into(),
            ..Workspace::default()
        };

        let mut walker = WalkBuilder::new(root);
        walker
            .hidden(false)
            // Honor the audited repository's own ignore files...
            .git_ignore(true)
            .ignore(true)
            // ...but nothing outside it. Parent .gitignore files, the user's
            // global gitignore and .git/info/exclude are all state that lives
            // outside the repository, and honoring them would make the same
            // repository report differently depending on whose machine ran it.
            .parents(false)
            .git_global(false)
            .git_exclude(false)
            // Honor .gitignore even when the target is not inside a git repo.
            .require_git(false)
            .filter_entry(|e| e.file_name() != ".git");

        for entry in walker.build() {
            let Ok(dent) = entry else { continue };
            let Ok(rel) = dent.path().strip_prefix(root) else {
                continue;
            };
            let rel = normalize(&rel.to_string_lossy());
            if rel.is_empty() {
                continue;
            }
            ws.present.insert(rel.clone());
            let symlink = dent.file_type().is_some_and(|t| t.is_symlink());
            // The walker does not follow symlinks by default, so a symlinked
            // entry arrives with a non-file type. Resolving it here is what
            // makes the documented `ln -s AGENTS.md CLAUDE.md` recipe visible
            // to the audit instead of looking like a missing CLAUDE.md.
            let is_file = if symlink {
                std::fs::metadata(dent.path()).is_ok_and(|m| m.is_file())
            } else {
                dent.file_type().is_some_and(|t| t.is_file())
            };
            if !is_file {
                continue;
            }
            let Some(kind) = Kind::classify(&rel) else {
                continue;
            };
            let bytes = std::fs::metadata(dent.path()).map(|m| m.len()).unwrap_or(0);
            let text = if bytes <= MAX_READ_BYTES {
                std::fs::read_to_string(dent.path()).ok()
            } else {
                None
            };
            let lines = text.as_deref().map(|t| t.lines().count()).unwrap_or(0);
            if let Some(t) = &text {
                ws.texts.insert(rel.clone(), t.clone());
            }
            ws.files.push(InstructionFile {
                path: rel,
                kind,
                bytes,
                lines,
                text,
                symlink,
            });
        }

        ws.finish();

        // A reference may legitimately point at a gitignored path (a build
        // artifact, a local env file). Probing those targets on disk keeps the
        // reference check from reporting them as broken, and keeps the check
        // itself pure: it only ever asks the workspace whether a path exists.
        let link_probes: Vec<String> = ws
            .files
            .iter()
            .filter(|f| f.kind.is_instruction_text())
            .flat_map(|f| {
                let dir = f.dir().to_string();
                text::relative_links(f.body())
                    .into_iter()
                    .map(move |r| resolve(&dir, &r.target))
            })
            .collect();
        for probe in link_probes {
            if probe.is_empty() || ws.present.contains(&probe) {
                continue;
            }
            if root.join(&probe).exists() {
                ws.present.insert(probe);
            }
        }

        // Follow `@path` imports transitively and read what they point at. The
        // bound is one hop past Claude Code's documented four so that an
        // over-deep chain is still observable.
        let mut frontier: Vec<String> = ws.files.iter().map(|f| f.path.clone()).collect();
        for _ in 0..=MAX_IMPORT_FOLLOW {
            let mut next = Vec::new();
            for from in &frontier {
                let dir = from.rfind('/').map(|i| &from[..i]).unwrap_or("");
                let Some(body) = ws.texts.get(from) else {
                    continue;
                };
                for import in text::imports(body) {
                    let target = resolve(dir, &import.path);
                    if target.is_empty() || ws.texts.contains_key(&target) {
                        continue;
                    }
                    let full = root.join(&target);
                    if !full.is_file() {
                        continue;
                    }
                    ws.present.insert(target.clone());
                    let small = std::fs::metadata(&full).is_ok_and(|m| m.len() <= MAX_READ_BYTES);
                    if !small {
                        continue;
                    }
                    let Ok(body) = std::fs::read_to_string(&full) else {
                        continue;
                    };
                    ws.texts.insert(target.clone(), body);
                    next.push(target);
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }

        Ok(ws)
    }

    fn record_present(&mut self, path: &str) {
        self.present.insert(path.to_string());
        let mut cursor = path;
        while let Some(i) = cursor.rfind('/') {
            cursor = &cursor[..i];
            self.present.insert(cursor.to_string());
        }
    }

    fn finish(&mut self) {
        self.files.sort_by(|a, b| a.path.cmp(&b.path));
        self.files.dedup_by(|a, b| a.path == b.path);
    }

    /// Mark an already-recorded path as a symbolic link. Builder form, used by
    /// tests that construct a workspace in memory.
    pub fn with_symlink(mut self, path: &str) -> Workspace {
        let key = normalize(path);
        if let Some(f) = self.files.iter_mut().find(|f| f.path == key) {
            f.symlink = true;
        }
        self
    }

    /// Directory name of the audited repository, used as the report title.
    pub fn root_name(&self) -> &str {
        &self.root_name
    }

    /// Every discovered instruction file, sorted by path.
    pub fn files(&self) -> &[InstructionFile] {
        &self.files
    }

    /// True when no instruction file of any convention was found.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Files of a given kind, sorted by path.
    pub fn of_kind(&self, kind: Kind) -> impl Iterator<Item = &InstructionFile> {
        self.files.iter().filter(move |f| f.kind == kind)
    }

    /// The file at exactly `path`, if it is one agentsdoc recognizes.
    pub fn file(&self, path: &str) -> Option<&InstructionFile> {
        let key = normalize(path).to_ascii_lowercase();
        self.files
            .iter()
            .find(|f| f.path.to_ascii_lowercase() == key)
    }

    /// The root-level file of a given kind, if any.
    pub fn root_of_kind(&self, kind: Kind) -> Option<&InstructionFile> {
        self.of_kind(kind).find(|f| f.dir().is_empty())
    }

    /// Text of any file agentsdoc read: an instruction file, or a file pulled
    /// in by an `@path` import.
    pub fn text(&self, path: &str) -> Option<&str> {
        self.texts.get(&normalize(path)).map(String::as_str)
    }

    /// Whether `path` exists in the repository (file or directory).
    pub fn exists(&self, path: &str) -> bool {
        let path = normalize(path);
        !path.is_empty() && self.present.contains(&path)
    }

    /// The nearest file of `kind` when working in `dir`, walking up to the
    /// repository root. `dir` is repository-relative, `""` for the root.
    pub fn nearest(&self, kind: Kind, dir: &str) -> Option<&InstructionFile> {
        for cursor in ancestors(dir) {
            if let Some(f) = self.of_kind(kind).find(|f| f.dir() == cursor) {
                return Some(f);
            }
        }
        None
    }

    /// Every file of `kind` in `dir` or any of its ancestors, ordered from the
    /// repository root down to `dir`.
    pub fn chain(&self, kind: Kind, dir: &str) -> Vec<&InstructionFile> {
        let mut chain: Vec<&InstructionFile> = ancestors(dir)
            .into_iter()
            .filter_map(|cursor| self.of_kind(kind).find(|f| f.dir() == cursor))
            .collect();
        chain.reverse();
        chain
    }
}

/// `dir` and every ancestor, nearest first, ending with the repository root.
pub fn ancestors(dir: &str) -> Vec<&str> {
    let dir = dir.trim_matches('/');
    let mut out = Vec::new();
    let mut cursor = dir;
    loop {
        out.push(cursor);
        match cursor.rfind('/') {
            Some(i) => cursor = &cursor[..i],
            None => break,
        }
    }
    if !dir.is_empty() {
        out.push("");
    }
    out
}

/// Resolve `target` relative to `dir`, collapsing `.` and `..`.
/// Returns `""` when the target escapes the repository root.
pub fn resolve(dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return String::new();
                }
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Normalize a path: forward slashes, no `./` prefix, no surrounding slashes.
fn normalize(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = p.strip_prefix("./").unwrap_or(&p);
    p.trim_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_covers_the_known_conventions() {
        let cases = [
            ("AGENTS.md", Some(Kind::AgentsMd)),
            ("packages/api/AGENTS.md", Some(Kind::AgentsMd)),
            ("CLAUDE.md", Some(Kind::ClaudeMd)),
            (".claude/CLAUDE.md", Some(Kind::ClaudeDirMd)),
            ("CLAUDE.local.md", Some(Kind::ClaudeLocalMd)),
            (".claude/rules/testing.md", Some(Kind::ClaudeRule)),
            (".claude/rules/frontend/style.md", Some(Kind::ClaudeRule)),
            (".cursorrules", Some(Kind::CursorLegacy)),
            (".cursor/rules/style.mdc", Some(Kind::CursorRule)),
            ("web/.cursor/rules/style.mdc", Some(Kind::CursorRule)),
            (".github/copilot-instructions.md", Some(Kind::CopilotRepo)),
            (
                ".github/instructions/api.instructions.md",
                Some(Kind::CopilotPath),
            ),
            ("GEMINI.md", Some(Kind::GeminiMd)),
            (".windsurfrules", Some(Kind::WindsurfRules)),
            (".windsurf/rules/a.md", Some(Kind::WindsurfRules)),
            (".clinerules", Some(Kind::ClineRules)),
            (".aider.conf.yml", Some(Kind::AiderConfig)),
            ("CONVENTIONS.md", Some(Kind::AiderConventions)),
            ("docs/CONVENTIONS.md", None),
            ("README.md", None),
            ("src/main.rs", None),
        ];
        for (path, expected) in cases {
            assert_eq!(Kind::classify(path), expected, "classifying {path}");
        }
    }

    #[test]
    fn classification_is_case_insensitive() {
        assert_eq!(Kind::classify("agents.MD"), Some(Kind::AgentsMd));
        assert_eq!(Kind::classify("Claude.md"), Some(Kind::ClaudeMd));
    }

    #[test]
    fn workspace_records_files_and_directories() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "hello"),
                ("docs/guide.md", "text"),
                ("src/main.rs", "fn main() {}"),
            ],
        );
        assert_eq!(ws.files().len(), 1);
        assert!(ws.exists("docs/guide.md"));
        assert!(ws.exists("docs"));
        assert!(ws.exists("src"));
        assert!(!ws.exists("nope.md"));
    }

    #[test]
    fn ancestors_walk_up_to_the_root() {
        assert_eq!(ancestors(""), [""]);
        assert_eq!(ancestors("a"), ["a", ""]);
        assert_eq!(ancestors("a/b/c"), ["a/b/c", "a/b", "a", ""]);
    }

    #[test]
    fn nearest_and_chain_respect_depth() {
        let ws = Workspace::from_entries(
            "x",
            [
                ("AGENTS.md", "root"),
                ("packages/api/AGENTS.md", "api"),
                ("packages/api/src/lib.rs", ""),
            ],
        );
        assert_eq!(
            ws.nearest(Kind::AgentsMd, "packages/api/src").unwrap().path,
            "packages/api/AGENTS.md"
        );
        assert_eq!(ws.nearest(Kind::AgentsMd, "web").unwrap().path, "AGENTS.md");
        let chain: Vec<&str> = ws
            .chain(Kind::AgentsMd, "packages/api")
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        assert_eq!(chain, ["AGENTS.md", "packages/api/AGENTS.md"]);
    }

    #[test]
    fn resolve_collapses_dot_segments() {
        assert_eq!(resolve("", "docs/a.md"), "docs/a.md");
        assert_eq!(resolve("docs", "a.md"), "docs/a.md");
        assert_eq!(resolve("docs/sub", "../a.md"), "docs/a.md");
        assert_eq!(resolve("docs", "./a.md"), "docs/a.md");
        assert_eq!(resolve("", "../escape.md"), "");
    }
}
