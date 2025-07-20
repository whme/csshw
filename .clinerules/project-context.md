# csshW Project Context

## What is csshW?
csshW is a Rust-based cluster SSH tool for Windows inspired by csshX. It enables users to SSH into multiple hosts simultaneously with synchronized keystroke distribution.

## Core Architecture
- **Daemon-Client Model**: One daemon process coordinates multiple client processes
- **Process Isolation**: Each SSH connection runs in its own console window
- **Focus-Based Input**: Keystrokes go to all clients when daemon focused, single client when client focused
- **Windows-Native**: Deep integration with Windows APIs for terminal and registry management

## Key Design Philosophy
- **Windows-Specific**: Not designed for cross-platform compatibility - embraces Windows APIs
- **User Experience**: Automatic configuration generation, sensible defaults, graceful degradation
- **Configuration-Driven**: TOML-based configuration with auto-generation of defaults
- **Safety First**: Extensive use of Result types and proper error handling

## Project Structure
- **Binary**: `csshw.exe` - Main executable with CLI interface
- **Library**: `csshw_lib` - Core functionality library
- **Modules**: `client/`, `daemon/`, `serde/`, `utils/` for feature separation
- **Tests**: `src/tests/` with component-based organization
