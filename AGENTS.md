# AGENTS.md

This file defines project-specific rules for agents working in this repository.

## Project Boundaries

- The bottom-layer implementation must be written in Rust.
- The core product requirement is to collect all comments for one or more specified Bilibili videos.
- Keep `README.md` empty unless the user explicitly asks to write it.
- `doc/` is ignored by Git. Do not force-add anything under `doc/` unless the user explicitly requests it.
- Do not copy implementation details from reference projects. External projects may only be used for explicitly requested style, workflow, protocol, or research references.

## Commit Message Style

Use the DEEIX-Chat `dev` branch commit style as the reference for commit and PR wording:

```text
<type>: <concise English summary>
```

Examples:

```text
init: scaffold Rust workspace
feat: add video comment collection command
fix: retry comment page requests after transient failures
refactor: split Bilibili client and comment parser
research: document Bilibili comment pagination behavior
learn: add Rust ownership primer notes
tooling: add local development task runner
deps: add reqwest and serde dependencies
data: add sample comment fixture for parser tests
```

Allowed types:

- `init`: initial setup, scaffolding, repository bootstrap, early project shape.
- `feat`: user-visible capability or behavior.
- `fix`: bug fix or incorrect behavior correction.
- `refactor`: internal restructuring without intended behavior change.
- `perf`: performance improvement.
- `docs`: tracked documentation changes.
- `learn`: learning notes, primers, or practice artifacts that are intentionally tracked.
- `research`: API investigation, protocol analysis, feasibility notes, or exploratory findings that are intentionally tracked.
- `test`: tests, fixtures, or test infrastructure.
- `data`: sample data, schemas, exports, or parser fixtures that are safe and intentionally tracked.
- `deps`: dependency changes.
- `build`: build system, packaging, Cargo configuration, or release build changes.
- `ci`: GitHub Actions or other CI configuration.
- `tooling`: local developer tools, scripts, linters, formatters, or automation.
- `chore`: maintenance that does not fit another specific type.
- `style`: formatting-only changes with no behavior impact.
- `revert`: revert a previous commit.

Rules:

- Commit subjects should be English, imperative or action-oriented, and specific.
- Prefer no scope unless it adds clarity. Use `feat(cli): ...` only when useful.
- Do not mix unrelated changes into one commit.
- Do not commit caches, build output, local configuration, credentials, or ignored tool folders.
- Before committing, check `git status --short` and review the staged diff.

## PR Style

PR titles should follow the same style as commit subjects:

```text
<type>: <concise English summary>
```

PR descriptions should explain:

- What changed and why.
- What concrete changes were made.
- How the change was verified.
- Any Bilibili API assumptions, data/privacy implications, compatibility notes, or known limitations.
- Any learning or research context when the PR exists partly to build Rust or Bilibili-domain understanding.

## Verification Expectations

When a Rust workspace exists, prefer these checks where practical:

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test
```

If a check cannot be run, state that explicitly in the final response or PR description.
