# Contributing to csshW

Thank you for considering contributing to csshW! It's people like you that make csshW a robust and reliable cluster SSH tool for Windows users.

Following these guidelines helps to communicate that you respect the time of the developers managing and developing this open source project. In return, they should reciprocate that respect in addressing your issue, assessing changes, and helping you finalize your pull requests.

### What kinds of contributions we're looking for

csshW is an open source project and we love to receive contributions from our community — you! There are many ways to contribute, from writing blog posts, improving the documentation, submitting bug reports and feature requests or writing code which can be incorporated into csshW itself.

## Ground Rules

### Technical Responsibilities

Before contributing code to csshW, please ensure you understand and can meet these requirements:

- **Complete Documentation**: All code must be documented - modules, functions, structs, constants, everything
- **Testing Requirements**: All tests must pass (`cargo doc-tests && cargo test`)
- **Code Quality**: Code must be formatted (`cargo fmt`) and pass linting (`cargo lint`)
- **Backwards Compatibility**: Configuration changes must maintain backwards compatibility
- **Comments**: Comments should explain *why*, not *what* - the code should be self-documenting

### Behavioral Expectations

- **Be respectful and considerate** in all interactions
- **Create issues for major changes** before implementing them to discuss the approach
- **Keep contributions focused** - one feature or fix per pull request
- **Test thoroughly** by ensuring high code coverage and manually testing on Windows systems before submitting
- **Follow the existing code patterns** and architectural decisions
- **Be welcoming to newcomers** and encourage diverse new contributors from all backgrounds.

## Your First Contribution

Unsure where to begin contributing to csshW? Here are some suggestions:

- **Documentation improvements** - Look for areas where explanations could be clearer
- **Test coverage** - Add tests for edge cases or improve existing test patterns
- **Bug fixes** - Check the Issues tab for reported bugs
- **Configuration enhancements** - Improve what can be configured and how

## Getting Started

### Development Environment Setup

1. **Prerequisites**:
   - Rust (we use [`rust-toolchain.toml`](https://github.com/whme/csshw/blob/main/rust-toolchain.toml#) to configure the desired rust version/toolchain)
   - Git
   - A Windows development environment

2. **Clone and Setup**:
   ```cmd
   git clone https://github.com/whme/csshw.git
   cd csshw
   git config --local core.hooksPath .githooks/
   ```

3. **Install Development Tools**:
   ```cmd
   cargo install cargo-make
   ```

### Development Workflow

csshW uses cargo aliases and [cargo make](https://github.com/sagiegurari/cargo-make) for development automation. Key commands:

- `cargo fmt` - Format code
- `cargo lint` - Run clippy linting
- `cargo test` - Run all tests
- `cargo build` - Build the project

### Pre-commit Hooks

csshW uses pre-commit git hooks to enforce code quality. These are automatically installed when you set the hooks path as shown above. The hooks will:

- Format your code with `cargo fmt`
- Run linting with `cargo lint`
- Build the project
- Generate documentation
- Update README help output if needed
- Run documentation tests
- Run all tests

### For Small Changes

Small contributions can be submitted directly as pull requests without creating an issue first.

Examples of small changes:
- Spelling/grammar fixes
- Typo corrections and formatting improvements
- Comment cleanup
- Documentation clarifications
- Adding logging messages or debugging output
- Changes to metadata files like `.gitignore`, build scripts, etc.

### For Larger Changes

For anything more substantial:

1. **Create an issue first** to discuss the change
2. **Fork the repository** and create a feature branch
3. **Make your changes** following the coding standards
4. **Ensure all tests pass** and pre-commit hooks succeed (you can enable the github actions after forking to have the CI run on your fork)
5. **Submit a pull request** with a clear description

## How to Suggest a Feature or Enhancement

If you have an idea for a new feature:

1. **Check existing issues** to see if it's already been suggested (open and closed)
2. **Create a new issue** with the "enhancement" label
3. **Describe the feature** following the issue template
4. **Be prepared to discuss** the implementation approach
5. **Consider offering to implement** the feature yourself

## Code Review Process

### Automated Checks

All pull requests go through automated checks using GitHub Actions and must pass all checks.

### Review Criteria

Pull requests are reviewed based on:

- **Code quality** - follows project standards and patterns
- **Testing** - adequate test coverage with proper mocking
- **Documentation** - complete and accurate documentation
- **Windows compatibility** - works correctly on supported Windows versions
- **Backwards compatibility** - doesn't break existing functionality

### Review Timeline

- **Initial response** - within 1 week for most pull requests
- **Detailed review** - depends on complexity and current workload
- **Follow-up** - we expect responses to feedback within 2 weeks

If a pull request shows no activity for 2 weeks after feedback, it may be closed.

## Community

### Communication Channels

- **GitHub Issues** - for bug reports and feature requests
- **Pull Request Comments** - for code-specific discussions

### Maintainers

csshW is maintained by [@whme](https://github.com/whme). Response times may vary based on availability and workload.

## Code, Commit Message and other Conventions

### Code Style

csshW follows standard Rust conventions with some specific requirements:

- **Follow clippy suggestions** - all warnings must be resolved
- **Document everything** - modules, functions, structs, constants
- **Use meaningful names** - prefer clarity over brevity
- **Handle errors properly** - use `Result<T, E>` for fallible operations

### Documentation Style

- **Module documentation**: Use `//!` with `#![doc(html_no_source)]`
- **Function documentation**: Include `# Arguments` and  `# Returns` ( `# Examples` are optional)
- **Document panics** and error conditions explicitly
- **Provide examples** for complex functionality

### Testing Patterns

csshW has no integration tests (yet). The following applies to unit tests.

- **Tests in `src/tests/`** with `test_*.rs` naming convention
- **Use `mockall`** for Windows API mocking
- **Follow Arrange-Act-Assert** pattern
- **Use descriptive test names** that explain what is being tested
- **No side effects** - all external interactions must be mocked

For easy manual testing on Windows we recommend the following setup:
- Enable OpenSSH Server - [docs](https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_install_firstuse?tabs=gui&pivots=windows-10)
- Run csshw against `localhost`:

    ```powershell
    cargo run -- -u $env:USERNAME localhost localhost
    ```

### Commit Messages

- Use clear, descriptive commit messages
- Start with a verb in present tense (e.g., "Add", "Fix", "Update")
- Keep the first line under 50 characters
- Provide additional details in the body

## Development Tools and Automation

### Cargo Make Tasks

csshW uses several `cargo make` tasks for automation either as part of the pre-commit githook, in the GitHub Actions CI or locally.

### Release Process

csshW follows a structured release process:

1. **Prepare release** with `cargo make prepare-release`
2. **Create pull request** from maintenance branch to main
3. **Create release tag** with `cargo make release`
4. **Publish release** through GitHub Actions

Contributors don't need to worry about releases - maintainers handle this process.

---

Thank you for contributing to csshW! Your efforts help make cluster management on Windows better for everyone.
