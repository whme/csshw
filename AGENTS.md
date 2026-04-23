# csshW — Agent Instructions

## Project Overview

csshW is a Rust-based cluster SSH tool for Windows inspired by csshX. It enables users to SSH into
multiple hosts simultaneously with synchronized keystroke distribution.

## Architecture

- **Daemon-Client Model**: One daemon process coordinates multiple client processes
- **Process Isolation**: Each SSH connection runs in its own console window
- **Focus-Based Input**: Keystrokes go to all clients when daemon focused, single client when client focused
- **Windows-Native**: Deep integration with Windows APIs for terminal and registry management

## Key Design Philosophy

- **Windows-Specific**: Not designed for cross-platform compatibility — embraces Windows APIs directly
- **User Experience**: Automatic configuration generation, sensible defaults, graceful degradation
- **Configuration-Driven**: TOML-based configuration with auto-generation of defaults
- **Safety First**: Extensive use of Result types and proper error handling

## Project Structure

- **Binary**: `csshw.exe` — Main executable with CLI interface (`src/main.rs`, `src/cli.rs`)
- **Library**: `csshw_lib` — Core functionality (`src/lib.rs`)
- **Modules**: `src/client/`, `src/daemon/`, `src/serde/`, `src/utils/`
- **Tests**: `src/tests/` with component-based organization (`test_*.rs` naming)
- **xtask**: `xtask/` — Developer automation tasks (README checks, release, changelog, social preview)
- **Config**: `.config/` — grouped, shared single-line marker files consumed
  by both `xtask` and CI. Currently holds `.config/coverage/` (pinned
  nightly toolchain, pinned Python tools `diff-cover` / `pycobertura`, and
  the coverage ignore-filename regex). Filenames follow
  `<identifier>.<kind>` where `<kind>` is `version` or `regex`.

## Build & Test Commands

```sh
cargo build                 # build
cargo fmt                   # format (run before submitting)
cargo lint                  # clippy (alias defined in .cargo/config.toml)
cargo test                  # unit + integration tests
cargo doc-tests             # documentation tests
```

Always run `cargo fmt`, `cargo lint`, and both test commands before considering any task complete.

## Code Standards

- **Document everything**: modules, functions, structs, constants — no exceptions
- **Minimize inline comments**: comments explain *why*, never *what*
- **Module-level docs**: use `//!` with `#![doc(html_no_source)]`
- **Function docs**: include purpose, `# Arguments`, `# Returns`, and `# Examples` sections
- **Document panics and error scenarios explicitly**

## Development Patterns

### RAII Resource Management
- Use guard structs that restore Windows state on `Drop`
- Example: `WindowsSettingsDefaultTerminalApplicationGuard` restores registry on cleanup

### Async-First Architecture
- All I/O operations are async via Tokio to prevent blocking
- Use `#[tokio::main]` for async entry points
- Spawn separate tasks for independent operations

### Error Handling
- Use `Result<T, E>` for all fallible operations
- Log warnings for non-critical failures and continue execution
- Panic with descriptive messages only for unrecoverable errors
- Registry failures are logged but do not stop execution

## Windows-Specific Implementation

### String Conversion
- **Rust → Windows API**: `OsString::encode_wide()` for UTF-16 encoding
- **Windows API → Rust**: `to_string_lossy()` for safe conversion back
- Always ensure proper null termination for C-style strings

### Windows API Integration
- Check all Windows API return values with descriptive error messages
- Apply RAII patterns to all Windows resources (handles, registry keys, etc.)
- Use `unsafe` blocks sparingly with proper validation
- Use `mockall` for testing Windows API calls without system side-effects

## Testing Standards

- **Naming**: `test_*.rs` files in `src/tests/`, descriptive test function names
- **Pattern**: Arrange-Act-Assert for all tests
- **Mocking**: Use `mockall` for all Windows API interactions — tests must have zero side-effects on the system
- **No external state**: tests must not modify registry, filesystem, or process state

## Commit Messages

Follow the conventions in
[`.claude/skills/commit/SKILL.md`](.claude/skills/commit/SKILL.md).
AI-generated commits MUST include a `Co-authored-by:` trailer
naming the model.

## User Interaction

- Clarify open questions before starting work
- Identify and resolve all ambiguities and assumptions up front
- Evaluate trade-offs before choosing an approach

## Agent GitHub auth

Paseo-spawned agents would otherwise inherit the contributor's full
`gh` CLI login — typically including the classic `repo` scope,
which is enough to delete the repository or force-push to `main`.
To keep an agent constrained to what it actually needs, each
contributor supplies a **fine-grained** Personal Access Token and
`cargo xtask inject-agent-token` (wired into `paseo.json`'s
`worktree.setup`) writes it into `.claude/settings.local.json` at
worktree creation time. Claude Code then injects `GH_TOKEN` into
the agent process, and `gh` honors `GH_TOKEN` over the keyring.

### One-time per-clone setup

1. Generate a fine-grained PAT at
   <https://github.com/settings/personal-access-tokens/new>.
   - **Resource owner**: yourself.
   - **Repository access**: "Only selected repositories" — pick the
     repos you actually work in from paseo (at minimum, your clone
     of `csshw`).
   - **Repository permissions**:
     - `Contents`: Read and write
     - `Pull requests`: Read and write
     - `Issues`: Read and write
     - `Metadata`: Read (auto-required)
     - Leave **everything else** at "No access" — in particular
       `Administration`, `Workflows`, `Secrets`, `Environments`,
       `Actions`, and `Pages`.
   - **Expiration**: 90 days or less. Put a calendar reminder to
     rotate it.
2. Save the token string (including the `github_pat_` prefix) to
   `<clone>/.paseo/gh-token` in the **source checkout**, not inside
   a worktree. On Unix-like shells also `chmod 600` the file. On
   Windows the default NTFS ACLs inherited from your user profile
   already restrict it to you.

Both `.paseo/gh-token` and `.claude/settings.local.json` are
gitignored and must never be committed.

### Expected behaviour

- On `paseo create`, you'll see
  `INFO - paseo agent GitHub auth: wrote …/.claude/settings.local.json
  from …/.paseo/gh-token (scoped PAT)`.
- If `.paseo/gh-token` is absent, the setup step prints a notice
  pointing here and exits 0 — the agent then uses whatever
  `gh auth` you already have.
- If the file contains a classic `ghp_…` or OAuth `gho_…` token
  (which cannot be scoped tightly enough), the setup step fails
  with a clear error. Replace with a `github_pat_` token.
- If `gh` calls start returning 401 after the PAT expires,
  regenerate it, overwrite `.paseo/gh-token`, and re-run
  `cargo xtask inject-agent-token` in the worktree (or simply
  create a new worktree).

### Attribution

Commits, PRs, and API calls made by the agent are attributed to
you (fine-grained PATs act as their owning user). The
`Co-authored-by: Claude …` trailer in every AI-authored commit
remains the marker that distinguishes agent work from hand-written
work.

## GitHub Pull Requests

Both PR creation and addressing review feedback are covered in
[`.claude/skills/github-pr/SKILL.md`](.claude/skills/github-pr/SKILL.md).
When addressing feedback: reply to every unresolved review comment,
mark each thread resolved once addressed, and push to update the PR.

## Completion Checklist

Before considering any task complete:

1. Documentation is complete and accurate
2. All tests pass (`cargo doc-tests && cargo test`)
3. Code is formatted (`cargo fmt`)
4. No clippy warnings (`cargo lint`)
5. All interactions with external systems are mocked in tests
6. Configuration changes maintain backwards compatibility
