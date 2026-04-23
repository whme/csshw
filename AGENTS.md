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

Commit messages follow the standard three-block layout: a single-line
**subject**, an optional wrapped prose **body**, and a **footer** block
of Git trailers — each block separated from the next by a blank line.

### Subject line

- Imperative mood, first word capitalized (`Add`, `Fix`, `Bump`,
  `Update`, `Support`, `Remove`, `Refactor`, `Replace`, `Improve`,
  `Migrate`).
- Optional lowercase scope prefix followed by `: ` when the change is
  confined to a single area (e.g. `client:`, `control mode:`,
  `post-pr-comment:`, `news-fragment-check:`). Mirror the scope style
  already used in `git log` — do not invent new scopes.
- No trailing period. Keep under ~72 characters.
- Do not pre-append a PR number in parentheses (`(#165)`) — GitHub's
  squash-merge adds that automatically when the PR lands.

### Body

- Separate the subject from the body with a blank line.
- Wrap lines at ~72–76 characters.
- Explain **why** the change is being made. Describe observable
  behavior before/after when relevant. "With this change …" is a
  common and acceptable opening.
- Use `-` for bullet lists.
- Reference advisories/URLs inline in the body when fixing CVEs or
  Dependabot alerts.

### Footer (trailers)

Trailers go in a final block separated from the body by a blank line.
Order: issue/PR references first, then co-author trailers.

- **GitHub references**: use `GitHub: #<number>` — one per line, in
  the footer, never in the subject or body prose. Do not use `Fixes:`
  (legacy style).
- **AI co-authorship (MANDATORY for AI-generated commits)**: include
  a `Co-authored-by:` trailer naming the model. For example:

  ```
  Co-authored-by: Claude Opus 4.6 <noreply@anthropic.com>
  ```

  - Use the exact model name in use (e.g. `Claude Opus 4.6`,
    `Claude Sonnet 4.6`, `Claude Haiku 4.5`).
  - Email must be `<noreply@anthropic.com>`.
  - Use the Git-canonical casing `Co-authored-by:` (lowercase
    `authored`/`by`). GitHub recognizes other casings too, but
    lowercase matches Git's own trailer convention and avoids
    duplicate trailers when tooling re-adds one.
  - Emit the trailer **exactly once** — never both `Co-Authored-By:`
    and `Co-authored-by:` for the same author.

### Example

```
client: handle zero-byte pipe reads gracefully

Previously a partial read from the daemon's named pipe was treated as
an EOF and caused every client to exit. Distinguish between
`0 bytes read` (daemon gone) and `n>0 bytes read` (buffer not yet
complete) so large pastes no longer tear down the cluster.

GitHub: #142
Co-authored-by: Claude Opus 4.6 <noreply@anthropic.com>
```

## User Interaction

- Clarify open questions before starting work
- Identify and resolve all ambiguities and assumptions up front
- Evaluate trade-offs before choosing an approach

## Completion Checklist

Before considering any task complete:

1. Documentation is complete and accurate
2. All tests pass (`cargo doc-tests && cargo test`)
3. Code is formatted (`cargo fmt`)
4. No clippy warnings (`cargo lint`)
5. All interactions with external systems are mocked in tests
6. Configuration changes maintain backwards compatibility
