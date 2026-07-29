# agentsdoc

A Rust CLI that audits a repository's AI coding agent instruction files for
consistency and cross-tool portability. Offline, no network access at any point.

This file is the single source of truth for agent instructions in this
repository. `CLAUDE.md` and `GEMINI.md` are one-line imports of it, which is the
arrangement agentsdoc itself recommends.

## Build and test commands

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo build --release
```

All four must pass before a change is proposed. `cargo test --all` is offline;
no test reaches the network.

## Layout

- `src/scan.rs` is the only module that touches the filesystem. It produces a
  `Workspace`, a read-once view of the repository.
- `src/agents.rs` holds the agent registry: which tool loads which files, and
  the official document that says so.
- `src/checks/` is pure: `Workspace` plus `Options` in, one `Finding` per check
  out.
- `src/report.rs` renders text and JSON. Both are byte-for-byte reproducible.
- `src/main.rs` does arguments, the walk and rendering, and nothing else.

## Code style

- rustfmt defaults; four-space indentation.
- No emoji anywhere, including comments and output.
- Prefer `?`, let-else and iterator adaptors. Avoid needless clones.
- Comments explain why a decision was made, not what the line does.

## Adding or changing a check

Every check must cite a document that was actually read, with the date, in its
`source` field. Beyond that:

- FAIL is only for breakage a cited behavior makes verifiable. Everything
  advisory is WARN or INFO.
- If a tool's behavior cannot be confirmed from official documentation, the
  check reports INFO and says "unverified". Never assert a loading behavior
  without a citation.
- Add tests covering the pass outcome, the not-clean outcome, and the
  not-applicable outcome.
- Update the check catalog in `README.md` in the same change.

## Dependencies

`anyhow`, `clap` and `ignore`, and that is meant to stay short. A change that
adds a dependency has to explain why a few dozen lines of local code would be
worse.

## Commit and pull request conventions

- [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`,
  `docs:`, `chore:`, `refactor:`, `test:`.
- English commit messages and pull request text.
- No `Co-Authored-By` trailers.
- One pull request, one purpose. See [CONTRIBUTING.md](CONTRIBUTING.md).
