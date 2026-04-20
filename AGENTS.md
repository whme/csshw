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

## Build & Test Commands

```sh
cargo build                 # build
cargo fmt                   # format (run before submitting)
cargo lint                  # clippy (alias defined in Makefile.toml)
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
