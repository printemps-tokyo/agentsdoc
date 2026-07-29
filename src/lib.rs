//! agentsdoc -- audit a repository's AI coding agent instruction files for
//! consistency and cross-tool portability.
//!
//! # The problem
//!
//! A repository can carry an `AGENTS.md`, a `CLAUDE.md`, a `.cursorrules`, a
//! `.github/copilot-instructions.md` and a `GEMINI.md`, and no two of those are
//! read by the same set of tools. The most common failure is silent: a
//! repository adopts `AGENTS.md`, every AGENTS.md consumer is configured, and
//! Claude Code loads no project instructions at all, because
//! <https://code.claude.com/docs/en/memory> (read 2026-07-29) says plainly
//! "Claude Code reads `CLAUDE.md`, not `AGENTS.md`." Nothing errors. The agent
//! simply does not know your conventions.
//!
//! agentsdoc walks a repository, reports what exists, works out which agents
//! would find something and which would find nothing, and checks the files
//! against each other.
//!
//! # Accuracy
//!
//! Every claim about a tool's loading behavior is attached to the official
//! document agentsdoc read, together with the date. FAIL is reserved for
//! breakage a cited behavior makes verifiable. Where no official documentation
//! could be found, the check reports INFO and says "unverified" rather than
//! asserting anything. See [`agents::AGENTS`] for the full table.
//!
//! # Shape
//!
//! - [`scan::Workspace`] is a read-once view of the repository. Building one is
//!   the only step that touches the filesystem.
//! - [`checks::audit`] is pure: workspace in, [`model::Report`] out.
//! - [`agents::effective_all`] is pure: workspace plus a subdirectory in,
//!   per-agent load order out.
//! - [`report`] renders either as text or as JSON.
//!
//! No network access happens at any point.
//!
//! ```no_run
//! use agentsdoc::{checks, report, scan::Workspace};
//!
//! let ws = Workspace::scan(std::path::Path::new("."), "my-repo")?;
//! let audit = checks::audit(&ws, &checks::Options::default());
//! print!("{}", report::render_text(&audit, report::RenderOptions { color: false }));
//! # Ok::<(), std::io::Error>(())
//! ```

pub mod agents;
pub mod checks;
pub mod model;
pub mod report;
pub mod scan;
pub mod text;

pub use agents::{effective_all, Effective, Layer};
pub use checks::{audit, Options, DEFAULT_TOKEN_BUDGET};
pub use model::{Category, Counts, Finding, Report, Severity, CAVEAT, SOURCES_READ};
pub use report::{render_json, render_text, RenderOptions};
pub use scan::Workspace;
