# Contributing

Thanks for your interest in contributing to agentsdoc.

## Development

1. Fork / clone the repository
2. Install a stable Rust toolchain (https://rustup.rs)
3. Create a branch: `git switch -c feat/your-change`
4. Make your change and verify locally:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets -- -D warnings
   cargo test --all
   cargo build --release
   ```
5. Commit and open a pull request

## Adding or changing a check

agentsdoc makes claims about what other people's tools do. That is only
defensible with a citation, so every check must name the document it is based
on, with the date it was read, in the check's `source` field. Pull requests that
add or change a check should:

- put the source URL and the date in the `source` field, not just in the pull
  request description, and quote the sentence that establishes the behavior
- reserve FAIL for breakage the cited behavior makes verifiable; anything
  advisory is WARN or INFO
- report INFO and say "unverified" when a tool's behavior cannot be confirmed
  from official documentation, rather than guessing at a FAIL
- never moralize: a repository with no instruction files, or one that omits a
  recommended section, is not doing anything wrong
- add tests covering the pass, the not-clean and the not-applicable outcomes
- update the check catalog in `README.md` in the same change

## Adding an agent

New entries in `src/agents.rs` need the same treatment: an official URL, the
date it was read, and a `verified` flag that is only `true` when the loading
behavior is stated in that document. An unverified agent must have
`missing: Severity::Info`; the test suite enforces this.

## Updating a source

Tool documentation changes. When a cited behavior changes, update the quote, the
URL and the date together, and update `SOURCES_READ` in `src/model.rs` if the
whole set was re-verified.

## Dependencies

agentsdoc depends on `anyhow`, `clap` and `ignore`, and that is meant to stay
short. A pull request adding a dependency needs to explain why the alternative
of a few dozen lines of local code is worse. Dependency updates are handled by
Dependabot (see `.github/dependabot.yml`).

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/) are preferred
(`feat:`, `fix:`, `docs:`, `chore:`, `refactor:`, `test:`). English, no emoji.

## Pull requests

- Keep one PR focused on a single purpose
- CI (fmt / clippy / test / build / self-audit) must be green
- Update the README and tests when behavior changes

## Bug reports and requests

Please open an [issue](../../issues) using the templates.
