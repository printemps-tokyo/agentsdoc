# agentsdoc

Audit a repository's AI coding agent instruction files for consistency and
cross-tool portability, and show what each agent would actually load.

A single Rust binary. No network access at any point, no telemetry, three direct
dependencies.

```console
$ agentsdoc .
agentsdoc: my-project
grade C  score 67/100  |  pass 4  warn 2  fail 1  info 9

Portability (score 64)
  [FAIL] portability-claude-code     Claude Code would load no project instructions, but this repository has 1 file(s)
         - AGENTS.md  (present, not loaded by Claude Code)
         why    loads CLAUDE.md, .claude/CLAUDE.md, .claude/rules/*.md and CLAUDE.local.md,
                walking up from the working directory; AGENTS.md is not read natively
         source https://code.claude.com/docs/en/memory (read 2026-07-29): "Claude Code reads
                CLAUDE.md, not AGENTS.md."
         fix    Add a CLAUDE.md at the repository root whose first line is `@AGENTS.md`, or
                symlink CLAUDE.md to AGENTS.md. The Claude Code memory doc gives both
                recipes.
```

---

## Why this is a real problem now

`AGENTS.md` is a genuine standard. [agents.md](https://agents.md/) (read
2026-07-29) describes it as an open format read by more than 60 tools, including
OpenAI Codex, Google Jules, Aider, VS Code, Devin, JetBrains Junie, Cursor and
GitHub Copilot, and states that it "is now stewarded by the Agentic AI Foundation
under the Linux Foundation." It has no required schema -- "AGENTS.md is just
standard Markdown. Use any headings you like; the agent simply parses the text
you provide" -- and it nests: "Agents automatically read the nearest file in the
directory tree, so the closest one takes precedence."

It is not universal. The Claude Code memory documentation
([code.claude.com/docs/en/memory](https://code.claude.com/docs/en/memory), read
2026-07-29) says plainly:

> Claude Code reads `CLAUDE.md`, not `AGENTS.md`. If your repository already uses
> `AGENTS.md` for other coding agents, create a `CLAUDE.md` that imports it so
> both tools read the same instructions without duplicating them.

So a repository that adopts `AGENTS.md` and stops there has fully configured
Codex, Cursor and Copilot, and has given Claude Code nothing. Nothing errors.
No warning is printed. The agent simply does not know your conventions, and you
find out from the pull request.

The mirror image is just as common: a repository with only a `CLAUDE.md` is
invisible to every AGENTS.md consumer. And the usual "fix" -- keeping both files
with the same content pasted into each -- creates two sources of truth that
diverge on the first edit.

agentsdoc finds all three situations, tells you which tool is affected, and cites
the document that says so.

---

## Install

### From source

```bash
git clone https://github.com/printemps-tokyo/agentsdoc
cd agentsdoc
cargo build --release
./target/release/agentsdoc --help
```

Requires a stable Rust toolchain (https://rustup.rs).

### Prebuilt binaries

Tagged releases publish binaries for `x86_64-unknown-linux-gnu`,
`x86_64-apple-darwin` and `aarch64-apple-darwin`, each with a `.sha256` checksum
next to it. See the [releases page](../../releases).

```bash
curl -LO https://github.com/printemps-tokyo/agentsdoc/releases/latest/download/agentsdoc-aarch64-apple-darwin.tar.gz
curl -LO https://github.com/printemps-tokyo/agentsdoc/releases/latest/download/agentsdoc-aarch64-apple-darwin.tar.gz.sha256
shasum -a 256 -c agentsdoc-aarch64-apple-darwin.tar.gz.sha256
tar -xzf agentsdoc-aarch64-apple-darwin.tar.gz
```

---

## Quickstart

```bash
# Audit the current repository
agentsdoc

# Audit somewhere else, machine-readable
agentsdoc ../other-repo --json

# Fail the build on advisory findings too
agentsdoc . --strict

# One line, for a shell prompt or a status check
agentsdoc . --quiet

# What would each agent actually load when working in packages/api?
agentsdoc . --effective packages/api

# Just the portability question
agentsdoc . --only portability

# Ignore a check you have decided against
agentsdoc . --skip portability-gemini-cli
```

### CLI

| Flag | Meaning |
| --- | --- |
| `[PATH]` | Repository directory to audit. Default `.` |
| `--json` | Emit the machine-readable document instead of the human report |
| `--strict` | Also exit non-zero when any check warns |
| `--effective <SUBDIR>` | Print each agent's assembled instruction set for `<SUBDIR>` instead of running the audit |
| `--tokens <N>` | Per-file estimated-token budget for `content-size`. Default `4000` |
| `--only <CATEGORY>` | Run only `discovery`, `portability`, `consistency` or `content`. Repeatable |
| `--skip <CHECK_ID>` | Skip one check by id. Repeatable |
| `--no-color` | Never emit ANSI color codes |
| `--quiet` / `-q` | Print only the one-line summary (the caveat goes to stderr) |
| `--help`, `--version` | |

Color is also suppressed by the `NO_COLOR` convention and whenever stdout is not
a terminal. JSON output is never colored.

---

## Tool behaviors change. Verify against your tool's docs.

This is the most important caveat in this README, so it is not in a footnote.

agentsdoc encodes what a specific set of official documents said on
**2026-07-29**. Coding agents ship weekly. A behavior that is true today may not
be true when you read this.

Three rules the tool follows so that it stays honest about this:

1. **Every claim has a citation.** Each finding prints the URL and the date the
   document was read, and quotes the sentence that establishes the behavior.
   The date is also in the JSON as `sources_read`.
2. **No citation, no FAIL.** Where agentsdoc could not confirm a tool's behavior
   from official documentation, the check reports INFO and says "unverified --
   check your tool's docs". It never asserts that a tool reads or ignores a file
   without a source.
3. **Configurable behavior is a WARN, not a FAIL.** Gemini CLI's context file
   name is settable in `settings.json`, which lives outside the repository, so
   agentsdoc cannot know from a clone whether a missing `GEMINI.md` is actually a
   problem. It says so and warns rather than failing.

If a finding disagrees with your tool's current documentation, the documentation
is right. Please [open an issue](../../issues) so the citation can be updated.

---

## Sources

Every behavioral claim in agentsdoc comes from one of these, all read on
2026-07-29:

| Source | What it establishes |
| --- | --- |
| [agents.md](https://agents.md/) | The `AGENTS.md` file name; plain Markdown with no required schema; nearest-file precedence in a monorepo; the recommended (not required) section conventions; stewardship by the Agentic AI Foundation under the Linux Foundation |
| [code.claude.com/docs/en/memory](https://code.claude.com/docs/en/memory) | Claude Code reads `CLAUDE.md`, not `AGENTS.md`; the `./CLAUDE.md`, `./.claude/CLAUDE.md`, `./CLAUDE.local.md` and `.claude/rules/` locations; root-down concatenation; `@path` import syntax, relative resolution, the four-hop limit and code-span/fence exclusion; the 200-line size guidance; the `@AGENTS.md` and `ln -s AGENTS.md CLAUDE.md` recipes |
| [cursor.com/docs/context/rules](https://cursor.com/docs/context/rules) | Cursor project rules live in `.cursor/rules` as `.mdc` files; Cursor supports `AGENTS.md` in the project root and subdirectories; plain `.md` rules without frontmatter are ignored; the `alwaysApply` / `globs` / `description` frontmatter fields |
| [docs.github.com -- add repository instructions](https://docs.github.com/en/copilot/how-tos/configure-custom-instructions/add-repository-instructions) | Copilot repository instructions live at `.github/copilot-instructions.md`; `.github/instructions/*.instructions.md` with `applyTo` frontmatter; `AGENTS.md` supported anywhere with nearest-file precedence; root `CLAUDE.md` or `GEMINI.md` as alternatives |
| [gemini-cli docs -- GEMINI.md](https://google-gemini.github.io/gemini-cli/docs/cli/gemini-md.html) | `GEMINI.md` is the default context file name; `context.fileName` in `settings.json` can change it (the doc's own example includes `AGENTS.md`); hierarchical loading from `~/.gemini/`, the project tree and subdirectories; `@file.md` imports |
| [aider.chat -- conventions](https://aider.chat/docs/usage/conventions.html) | aider loads no conventions file automatically; a file must be named with `--read`, `/read`, or a `read:` key in `.aider.conf.yml` |

Windsurf is listed in the agent table but marked **unverified**: no official
document could be cited for it on 2026-07-29, so its check never exceeds INFO.

---

## What it discovers

The walk is gitignore-aware (`ignore::WalkBuilder`, the same crate as `rg`), so a
vendored `node_modules/AGENTS.md` is not counted as yours. Hidden directories are
walked on purpose, because `.claude/`, `.cursor/` and `.github/` are the point.
Symlinks are resolved, so the documented `ln -s AGENTS.md CLAUDE.md` recipe is
visible to the audit.

| Kind (JSON token) | Paths |
| --- | --- |
| `agents-md` | `AGENTS.md`, root and nested |
| `claude-md` | `CLAUDE.md`, root and nested |
| `claude-dir-md` | `.claude/CLAUDE.md` |
| `claude-local-md` | `CLAUDE.local.md` |
| `claude-rule` | `.claude/rules/**/*.md` |
| `cursor-rule` | `.cursor/rules/**/*.mdc` and `.md` |
| `cursorrules-legacy` | `.cursorrules` |
| `copilot-instructions` | `.github/copilot-instructions.md` |
| `copilot-path-instructions` | `.github/instructions/**/*.instructions.md` |
| `gemini-md` | `GEMINI.md` |
| `windsurf-rules` | `.windsurfrules`, `.windsurf/rules/**/*.md` |
| `cline-rules` | `.clinerules`, `.clinerules/**/*.md` |
| `aider-config` | `.aider.conf.yml` / `.yaml` |
| `aider-conventions` | root `CONVENTIONS.md` |

Each is reported with its byte size, line count, and an estimated token count.

### The token estimate

`est_tokens = ceil(chars / 4)`, counting Unicode `char`s rather than bytes.

Four characters per token is the common rule of thumb for English prose under
OpenAI-family BPE tokenizers. It is an estimate and is labeled `~` and "est."
everywhere it appears. The real number depends on which tokenizer your agent
uses, on how much of the file is code rather than prose, and on the script: CJK
text in particular runs much denser than 4 characters per token. Use the figure
to compare files against each other and to notice growth, not as a budget you
can bill against.

---

## Check catalog

16 checks in four categories. Each produces exactly one finding, so the catalog
below is exactly what the tool emits, and every id is a valid `--skip` value.

### discovery

| Id | Severity | What it reports | Source |
| --- | --- | --- | --- |
| `discovery-files` | always INFO | Inventory: every instruction file with kind, bytes, lines and estimated tokens, plus the total | agents.md; Claude Code memory doc |
| `discovery-nesting` | always INFO | The nesting tree, with the scope each nested file wins for, and a note that the two conventions layer differently | agents.md ("the closest one takes precedence"); Claude Code memory doc ("All discovered files are concatenated into context rather than overriding each other") |

Discovery is unscored. Having no instruction files, and having nested ones, are
both perfectly ordinary states.

### portability

One check per agent: `portability-claude-code`, `portability-codex`,
`portability-cursor`, `portability-copilot`, `portability-gemini-cli`,
`portability-aider`, `portability-windsurf`.

Each answers one question: *would this agent find any project instructions in
this repository?*

| Situation | Result |
| --- | --- |
| The agent loads at least one file | PASS (INFO for an unverified agent) |
| The repository has no instruction files at all | INFO for every agent -- there is nothing to be portable about |
| The repository has instructions but this agent loads none, and its behavior is cited | FAIL for Claude Code, Codex, Cursor and Copilot |
| ... and the behavior depends on configuration outside the repository | WARN for Gemini CLI |
| ... and the agent documents loading nothing by default | INFO for aider |
| ... and the behavior could not be verified | INFO for Windsurf |
| Only a legacy `.cursorrules` is present | WARN for Cursor, not FAIL: the current Cursor rules doc does not list `.cursorrules`, and agentsdoc will not assert that it is ignored either |

`portability-claude-code` and `portability-codex` carry double weight in the
score, because they are the two headline failure modes.

### consistency

| Id | Severity | What it checks | Source |
| --- | --- | --- | --- |
| `consistency-claude-import` | PASS / WARN / INFO | When both a root `AGENTS.md` and a root `CLAUDE.md` (or `.claude/CLAUDE.md`) exist: does the CLAUDE.md import the AGENTS.md, or is it a symlink to it? If not, Claude Code never sees the AGENTS.md content -- WARN | Claude Code memory doc: "create a CLAUDE.md that imports it so both tools read the same instructions without duplicating them" |
| `consistency-duplication` | PASS / WARN / INFO | Pairs of instruction files that restate a large share of each other, with the overlap measure below. Pairs where one imports the other, or one is a symlink, are exempt | Claude Code memory doc: "without duplicating them" |
| `consistency-contradiction` | PASS / WARN / INFO | The same directive stated with opposite polarity in two files. Deliberately narrow; labeled a heuristic in the output | Claude Code memory doc: "if two rules contradict each other, Claude may pick one arbitrarily" |
| `consistency-imports` | PASS / WARN / FAIL / INFO | Every `@path` import resolves, following the whole import graph. A missing target is FAIL. A chain deeper than four hops is WARN | Claude Code memory doc: `@path/to/import` syntax, relative resolution, "a maximum depth of four hops" |

#### The duplication measure

agentsdoc defines it, and documents it here so a WARN is arguable rather than
mysterious.

1. Reduce each file to its **significant lines**: drop fenced code blocks, trim
   whitespace, strip leading `#`, `>`, `-`, `*`, `+` and ordered-list markers,
   collapse internal whitespace, lowercase, and drop anything shorter than 16
   characters. Short lines and headings collide by accident and would make every
   pair of Markdown files look similar.
2. `shared` is the multiset intersection of the two files' significant lines.
3. `ratio = shared / min(len(a), len(b))`.

The denominator is the **smaller** file on purpose. The risk being measured is
"one file restates the other and they will drift", and that risk is fully present
when a short `CLAUDE.md` is a verbatim subset of a long `AGENTS.md`. Dividing by
the union would hide exactly that case.

A pair is reported when `shared >= 5` **and** `ratio >= 0.30`. Both thresholds
are constants in `src/text.rs` (`MIN_SHARED_LINES`, `DUPLICATION_RATIO`).

#### The contradiction heuristic

Only lines that open with an explicit polarity marker are considered:

- negative: `do not`, `don't`, `never`, `avoid`, `must not`, `should not`,
  `no longer`
- positive: `always`, `must`, `should`, `please`, `make sure to`

The marker is stripped, a leading `to ` and trailing punctuation are removed, and
what is left is the *core*. Two directives contradict when their cores are
identical and their polarities differ. Cores shorter than 12 characters are
ignored.

This will not catch a contradiction phrased two different ways, and it is not
meant to. It catches the copy-paste-then-edit-one-copy case, which is the one
that actually happens, and the report labels it a heuristic so it is never
mistaken for a proof.

### content

| Id | Severity | What it checks | Source |
| --- | --- | --- | --- |
| `content-size` | PASS / WARN / INFO | Files over `--tokens` estimated tokens (default 4000) or over 200 lines | Claude Code memory doc: "target under 200 lines per CLAUDE.md file. Longer files consume more context and reduce adherence." |
| `content-sections` | PASS / WARN / INFO | Whether the primary instruction file covers build/test commands, code style, and commit/PR conventions. Titled "recommended (not required)" because the format has no schema | agents.md: "AGENTS.md is just standard Markdown. Use any headings you like", plus its list of popular sections |
| `content-links` | PASS / FAIL / INFO | Relative Markdown links whose target does not exist in the repository. Links inside fenced code blocks are treated as examples and skipped; gitignored targets are probed on disk before being reported | Not a tool behavior: a link to a path that is not there is verifiably broken |

The default token budget of 4000 is derived from the 200-line guidance: 200 lines
of prose is roughly 12,000 characters, which the chars/4 heuristic puts near 3,000
tokens, and 4,000 leaves headroom for code blocks and tables so the default does
not fire on ordinary files. The 200-line check runs independently, so a file that
is long but sparse is still flagged.

`content-sections` recognizes a topic from headings or from concrete markers in
the body (`cargo test`, `npm run`, `prettier`, `conventional commit`, and so on).
It is a heuristic, it is advisory, and a missing section is a suggestion. Do not
pad a file to satisfy it.

---

## Severity contract

| Severity | Meaning | Effect on score | Effect on exit code |
| --- | --- | --- | --- |
| `pass` | The property holds | full credit | none |
| `info` | Context, or a neutral fact about the repository | unscored | none, ever |
| `warn` | Advisory. Worth looking at; reasonable projects disagree | half credit | only under `--strict` |
| `fail` | Concrete breakage a cited behavior makes verifiable | no credit | exit 1 |

FAIL is used in exactly three situations:

- a documented loader would find no project instructions in a repository that
  has them (`portability-*`)
- an `@path` import resolves to a file that does not exist
  (`consistency-imports`)
- a relative link points at a path the repository does not contain
  (`content-links`)

Everything else is advisory. In particular, a repository with **no** instruction
files at all is INFO across the board, scores `n/a`, and exits 0 even under
`--strict`. Plenty of repositories legitimately have none.

The score is a weighted percentage over the non-INFO findings (PASS 1.0, WARN
0.5, FAIL 0.0), with `portability-claude-code` and `portability-codex` weighted
double. Grades: A >= 90, B >= 75, C >= 60, D >= 40, otherwise F.

---

## `--effective`

`--effective <subdir>` answers a different question from the audit: *if I start
an agent in this directory, what exactly does it read, in what order, and what
does that cost?*

```console
$ agentsdoc . --effective packages/api
agentsdoc: my-monorepo  effective instruction set for packages/api

Claude Code [claude-code]
  1  CLAUDE.md                                    ~     3 tokens est.  project memory
  2  AGENTS.md                                    ~   611 tokens est.  imported by CLAUDE.md (hop 1)
  3  packages/api/CLAUDE.md                       ~   180 tokens est.  project memory
  4? .claude/rules/api.md                         ~    90 tokens est.  rule, path-scoped (loads only for matching files)
     total (unconditional): ~794 tokens est.
     Order is filesystem root down to the working directory, so the nearest file is read
     last (code.claude.com/docs/en/memory).
     User and managed scopes (~/.claude/CLAUDE.md, the managed policy file) also load but
     live outside the repository and are not listed.

OpenAI Codex (and other AGENTS.md consumers) [codex]
  1  packages/api/AGENTS.md                       ~   210 tokens est.  nearest AGENTS.md
     total (unconditional): ~210 tokens est.
     agents.md: "the closest one takes precedence", so only the nearest AGENTS.md is
     listed as loaded.
     AGENTS.md is overridden by the nearer file.
```

Entries are in load order, `@path` imports are expanded in place with their hop
number, and a `?` marks a file whose loading depends on something agentsdoc
cannot see from the repository alone (frontmatter `paths`/`globs`, Copilot
`applyTo` patterns, Cursor rules without frontmatter). Conditional entries are
excluded from the total.

Out-of-repository scopes -- `~/.claude/CLAUDE.md`, `~/.gemini/GEMINI.md`, the
managed policy file -- are named in the notes but never listed as layers, because
they are not part of what a contributor gets from a clone.

`--effective` prints instead of auditing and always exits 0. `--json` works with
it.

---

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | No FAIL findings. With `--strict`, also no WARN findings |
| `1` | At least one FAIL. With `--strict`, at least one FAIL or WARN |
| `2` | Usage error or I/O error: bad flag, unknown check id, `--tokens 0`, path is not a directory, `--effective` target outside the repository |

`--effective` always exits 0 on success.

In CI:

```yaml
- run: agentsdoc . --strict
```

---

## JSON output

`--json` emits a single object. Keys are fixed; no key is derived from repository
content, so the document is safe to parse positionally or with `jq`.

```jsonc
{
  "tool": "agentsdoc",
  "version": "0.1.0",
  "sources_read": "2026-07-29",     // date every cited behavior was read
  "repository": "adx",              // directory name of the audited repository
  "grade": "C",                     // A | B | C | D | F | n/a
  "score": 72,                      // 0-100, or null when nothing scoreable ran
  "counts": {"pass": 5, "warn": 1, "fail": 1, "info": 9},
  "skipped": 0,                     // checks suppressed by --skip / --only
  "total_est_tokens": 29,
  "files": [                        // sorted by path
    {"path": "AGENTS.md", "kind": "agents-md", "bytes": 114, "lines": 10, "est_tokens": 29}
  ],
  "agents": [                       // the coverage matrix, in registry order
    {"agent": "claude-code", "name": "Claude Code", "verified": true, "loads": []},
    {"agent": "codex", "name": "OpenAI Codex (and other AGENTS.md consumers)",
     "verified": true, "loads": ["AGENTS.md"]}
  ],
  "categories": [
    {"id": "portability", "title": "Portability", "score": 64,
     "counts": {"pass": 3, "warn": 1, "fail": 1, "info": 2}}
  ],
  "findings": [                     // sorted by category order, then id
    {
      "id": "portability-claude-code",
      "category": "portability",
      "severity": "fail",           // pass | info | warn | fail
      "title": "Claude Code would load no project instructions, ...",
      "evidence": ["AGENTS.md  (present, not loaded by Claude Code)"],
      "why": "loads CLAUDE.md, .claude/CLAUDE.md, ...",
      "source": "https://code.claude.com/docs/en/memory (read 2026-07-29): ...",
      "fix": "Add a CLAUDE.md at the repository root whose first line is `@AGENTS.md` ..."
    }
  ],
  "caveat": "Agent loading behavior changes fast. ..."
}
```

`--effective --json` emits a different document:

```jsonc
{
  "tool": "agentsdoc",
  "version": "0.1.0",
  "sources_read": "2026-07-29",
  "repository": "my-monorepo",
  "directory": "packages/api",      // "" for the repository root
  "effective": [
    {
      "agent": "claude-code",
      "name": "Claude Code",
      "verified": true,
      "est_tokens": 794,            // sum over unconditional layers only
      "layers": [
        {"order": 1, "path": "CLAUDE.md", "est_tokens": 3,
         "conditional": false, "note": "project memory"}
      ],
      "notes": ["Order is filesystem root down to the working directory, ..."]
    }
  ],
  "caveat": "..."
}
```

Output is deterministic: findings sort by category order then id, files and
evidence sort by path, and text wraps at a fixed width rather than the terminal
width. Two runs over the same tree produce byte-identical output.

Comments in the blocks above are for the reader; real output is plain JSON.

---

## Fixing the common cases

**AGENTS.md but no CLAUDE.md.** Add a root `CLAUDE.md`:

```markdown
@AGENTS.md

## Claude Code

Use plan mode for changes under `src/billing/`.
```

or, if you have nothing Claude-specific to add, symlink it, which the memory doc
also documents:

```bash
ln -s AGENTS.md CLAUDE.md
```

(On Windows a symlink needs Administrator privileges or Developer Mode, so the
`@AGENTS.md` import is the portable choice.)

**CLAUDE.md but no AGENTS.md.** Rename it to `AGENTS.md` and leave a `CLAUDE.md`
containing `@AGENTS.md`. Everything that read the `CLAUDE.md` still does; Codex,
Cursor and Copilot now do too.

**Both, with the same content pasted into each.** Delete the body of one and
replace it with the import. `consistency-duplication` will stop warning, and so
will the drift.

**Gemini CLI too.** A `GEMINI.md` containing `@AGENTS.md` works, because Gemini
CLI supports `@file.md` imports. This repository does exactly that.

---

## Design notes

**Library / binary split.** All the logic lives in the library and is pure:
`scan::Workspace` is the only thing that touches the filesystem, `checks::audit`
and `agents::effective_all` take a workspace and return data, and `report`
renders. The binary does arguments, the walk and output. Tests build workspaces
in memory from `(path, contents)` pairs, so most of the suite needs no tempdir.

**Three dependencies.**

- `anyhow` -- error context in the thin binary layer.
- `clap` v4 (derive) -- argument parsing.
- `ignore` -- the gitignore-aware walk. Getting `.gitignore`, `.ignore` and
  nested ignore semantics right by hand is not worth it, and this is the crate
  `rg` uses.

No serde: the JSON document has a fixed shape with no user-controlled keys, so it
is hand-rolled with a proper string escaper (about 20 lines). No Markdown parser:
agentsdoc never renders Markdown, it only needs headings, fenced-code boundaries,
inline links and `@path` imports, all of which are unambiguous at the line level.
No regex: the patterns are few and fixed.

**Ignore files outside the repository are not honored.** Parent-directory
`.gitignore`s, the user's global gitignore and `.git/info/exclude` are all state
that lives outside the repository being audited. Honoring them would make the
same repository report differently depending on whose machine ran it.

---

## Dogfood

agentsdoc audits its own repository in CI, under `--strict`. This repository
keeps its instructions in `AGENTS.md`, with a one-line `CLAUDE.md` and
`GEMINI.md` that import it -- which is the arrangement the tool recommends.

```console
$ agentsdoc . --quiet
agentsdoc: agentsdoc  grade A  score 100/100  |  pass 12  warn 0  fail 0  info 4
```

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The short version: every check must cite a
document that was actually read, with the date; FAIL is only for verifiable
breakage; an unverifiable behavior is INFO and says so.

## License

MIT. See [LICENSE](LICENSE).
