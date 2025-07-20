# Requirements and Standards

## User interaction

- Clarify open questions with the user before starting any work
- Always ask for clarification and follow-ups
- Fully understand all the aspects of the problem and gather details to make it very precise and clear
- Ask about all the hypothesis and assumptions that need to be made. Remove all the ambiguities and uncertainties
- Think about all possible ways to solve the problem
- Set up the evaluation criteria and trade-offs to access the merit of the solutions
- Find the optimal solution and the criteria making it optimal and the trade-offs involved

## Code Quality Standards
- **Everything must be documented** modules, functions, structs, constants, everything.
- **Minimize inline comments** - comments should never describe *what* the code does, but *why* it does it in this way

## Testing Standards

### Test Organization
- **Tests must follow `test_*.rs` naming convention** in `src/tests/`
- **Use descriptive test names** that explain what is being tested
- **Follow Arrange-Act-Assert pattern** for test structure

## Documentation Requirements

### Code Documentation
- **Module-level documentation** must use `//!` with `#![doc(html_no_source)]`
- **Function documentation** must include purpose, arguments, return values, and examples
- **Document panic conditions** and error scenarios explicitly

### Change Documentation
- **Update relevant documentation** when changing behavior
- **Add examples** for new configuration options
- **Document breaking changes** clearly

# Development Workflow

## Before Starting Work
- **Read relevant documentation** in the codebase before making changes
- **Understand the daemon-client coordination** impact of any changes

## Before Task Completion
- **All tests must pass** before considering any task complete (`cargo doc-tests` and `cargo tests`)
- **All clippy warnings must be resolved** - no exceptions for new code (`cargo lint`)
- **Code must be formatted** before submission (`cargo fmt`)

## Mandatory Completion Checklist

Before considering any task complete, verify:

1. ✅ Documentation is complete and accurate
2. ✅ All tests pass (`cargo doc-tests && cargo test`)
3. ✅ Code is formatted (`cargo fmt`)
4. ✅ No clippy warnings (`cargo lint`)
5. ✅ Interactions with external system are all mocked in tests - tests must have no side-effects
6. ✅ Configuration changes maintain backwards compatibility
