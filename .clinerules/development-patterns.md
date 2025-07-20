# Development Patterns

## RAII Resource Management
- **Windows Resources**: Automatic cleanup of registry settings, handles, and other Windows resources
- **Guard Pattern**: Use guard structs that restore state on drop
- **Example**: `WindowsSettingsDefaultTerminalApplicationGuard` restores registry on cleanup

## Async-First Architecture
- **Tokio Runtime**: All I/O operations are async to prevent blocking
- **Entry Points**: Use `#[tokio::main]` for async main functions
- **Concurrency**: Spawn separate tasks for independent operations

## Error Handling Strategy
- **Result Types**: Use `Result<T, E>` for all fallible operations
- **Graceful Degradation**: Log warnings for non-critical failures, continue execution
- **Critical Failures**: Panic with descriptive messages for unrecoverable errors
- **Registry Operations**: Registry failures are logged but don't stop execution

## Documentation Standards
- Use `//!` for module-level documentation with `#![doc(html_no_source)]`
- Use `///` for functions with `# Arguments`, `# Returns`, and `# Examples` sections
- Document panics and error conditions explicitly

## Testing Strategy
- Tests in `src/tests/` with `test_*.rs` naming convention
- Use `mockall` for Windows API mocking to avoid system modification during tests
- Follow Arrange-Act-Assert pattern with descriptive test names
