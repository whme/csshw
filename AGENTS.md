# csshW - Agent Instructions

## Project Overview

csshW is a Rust-based cluster SSH tool for Windows inspired by csshX. It enables users to SSH into
multiple hosts simultaneously with synchronized keystroke distribution.

## Architecture

- **Daemon-Client Model**: One daemon process coordinates multiple client processes
- **Process Isolation**: Each SSH connection runs in its own console window
- **Focus-Based Input**: Keystrokes go to all clients when daemon focused, single client when client focused
- **Windows-Native**: Deep integration with Windows APIs for terminal and registry management

## Key Design Philosophy

- **Windows-Specific**: Not designed for cross-platform compatibility - embraces Windows APIs directly
- **User Experience**: Automatic configuration generation, sensible defaults, graceful degradation
- **Configuration-Driven**: TOML-based configuration with auto-generation of defaults
- **Safety First**: Extensive use of Result types and proper error handling

## Project Structure

- **Binary**: `csshw.exe` - Main executable with CLI interface (`src/main.rs`, `src/cli.rs`)
- **Library**: `csshw_lib` - Core functionality (`src/lib.rs`)
- **Modules**: `src/client/`, `src/daemon/`, `src/serde/`, `src/utils/`
- **Tests**: `src/tests/` with component-based organization (`test_*.rs` naming)
- **xtask**: `xtask/` - Developer automation tasks (README checks, release, changelog, social preview)
- **Config**: `.config/` - grouped, shared single-line marker files consumed
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
cargo xtask check-typography # ASCII-punctuation lint
```

Always run `cargo fmt`, `cargo lint`, and both test commands before considering any task complete.

## ASCII-Only Punctuation

Do NOT use decorative or "smart" Unicode punctuation anywhere in the
repo - not in code, comments, docstrings, commit messages, PR
descriptions, or markdown docs. Use the ASCII equivalent:

- em-dash and en-dash -> single `-` (NEVER `--`)
- smart quotes        -> `'` or `"`
- ellipsis            -> `...`
- arrows              -> `->`, `<-`, `=>`, etc.
- bullet / middle-dot -> `-` or `*`
- non-breaking space  -> regular space
- math glyphs         -> ASCII operators (`x`, `/`, `>=`, `<=`, `!=`)

Emoji in user-visible output (e.g. CI workflow logs) are fine.

This is enforced by `cargo xtask check-typography`, which runs in the
pre-commit hook and CI. If the check fails, fix the offending
characters - do NOT add to the allowlist.

## Code Standards

- **ASCII-only punctuation**: see the section above; this is enforced in CI

### Documentation and Comments

The goal is csshW's observed style, not "document everything." Match the
density of the surrounding code; do not pad.

**Docstring scope.**

- **Public items** (`pub fn`, `pub struct`, public consts) and trait methods:
  one-sentence imperative summary (`Return the ...`, not `This function
  returns ...`), plus `# Arguments` and `# Returns`. The `# Arguments` block
  is load-bearing - keep it even when trimming other parts of a docstring.
- **`# Examples`**: only for reusable utilities a caller invokes in isolation
  (see `src/utils/windows.rs`). Do NOT add `# Examples` to trait methods, CLI
  entrypoints, protocol handlers, or any function whose behaviour is only
  meaningful inside its module.
- **`# Panics` / `# Errors`**: only when they actually apply. Omit otherwise.
- **Private helpers**: one-line doc if the purpose is non-obvious; skip
  entirely for trivial helpers, simple getters, single-expression wrappers.
- **Test functions, closures, trivial trait impls**: no docs.
- **Module docs** (`//!`): one line for typical modules. Multi-paragraph only
  when the module defines a protocol or wire format (see
  `src/serde/protocol/mod.rs`). All library modules use
  `#![doc(html_no_source)]`.

**Docstring style.**

````rust
// GOOD
/// Return the console window handle for the current process.
///
/// # Arguments
/// * `pid` - Process ID whose console is being queried.
///
/// # Returns
/// `HWND` to the attached console, or `null` if none is attached.
pub fn get_console_window_handle(pid: u32) -> HWND { ... }

// BAD - narrates, restates the signature, invents an `# Examples` block for
// a function nobody calls in isolation.
/// This is a function that gets the console window handle. It takes a
/// process ID (a u32) and returns an HWND, which is a Windows handle to
/// the console window.
///
/// # Arguments
/// * `pid` - The process ID. This is a u32 representing the process.
///
/// # Returns
/// Returns the HWND.
///
/// # Examples
/// ```ignore
/// let hwnd = get_console_window_handle(std::process::id());
/// ```
pub fn get_console_window_handle(pid: u32) -> HWND { ... }
````

**Inline comments.** Default to zero - ~85-90% of function bodies in this
repo have no inline comments. Add a `//` comment only for:

- Windows / platform quirks - cite the MS Learn URL or equivalent.
- Non-obvious async ordering, race conditions, or shared-state invariants.
- Magic numbers, protocol byte layout, named-pipe contracts.
- `// SAFETY:` justifications for `unsafe` blocks.

Never paraphrase the next line, narrate steps (`// Step 1: ...`,
`// First, ... // Then, ...`), add banner dividers (`// ----- Helpers -----`),
or commit commented-out code. Canonical examples to study:
`src/utils/windows.rs:934-937` (Win10/11 divergence),
`src/daemon/mod.rs:706-708` (async ordering invariant),
`src/client/mod.rs:409-411` (protocol contract).

```rust
// GOOD - cites a platform quirk and explains the workaround.
// Win10 conhost leaves the bottom row stale after a bulk attribute fill
// until something forces a redraw; Win11+ Terminal repaints on its own.
nudge_cursor(handle)?;

// BAD - paraphrases the call.
// Set the console title.
set_console_title(handle, &title)?;
```

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
- **Rust -> Windows API**: `OsString::encode_wide()` for UTF-16 encoding
- **Windows API -> Rust**: `to_string_lossy()` for safe conversion back
- Always ensure proper null termination for C-style strings

### Windows API Integration
- Check all Windows API return values with descriptive error messages
- Apply RAII patterns to all Windows resources (handles, registry keys, etc.)
- Use `unsafe` blocks sparingly with proper validation
- Use `mockall` for testing Windows API calls without system side-effects

## Testing Standards

- **Naming**: `test_*.rs` files in `src/tests/`, descriptive test function names
- **Pattern**: Arrange-Act-Assert for all tests
- **Mocking**: Use `mockall` for all Windows API interactions - tests must have zero side-effects on the system
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
5. No forbidden Unicode (`cargo xtask check-typography`)
6. All interactions with external systems are mocked in tests
7. Configuration changes maintain backwards compatibility
