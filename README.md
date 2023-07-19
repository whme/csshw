# csshW
_Cluster SSH tool for Windows inspired by [csshX](https://github.com/brockgr/csshx)_

## Pre-requisites
- A working [WSL-2](https://learn.microsoft.com/en-us/windows/wsl/install) installation [^1]
- ``Default terminal application`` is set to ``Windows Console Host`` in the windows Terminal Startup Settings (Windows 11 only)

## Overview
csshW consist of 3 executables:
- ``csshw`` - a launcher that starts the daemon application and serves as main entry point
- ``csshw-daemon`` - spawns and positions the client windows and propagates any key-strokes to them
- ``csshw-client`` - establishes an SSH connection and replays key-strokes received from the daemon

csshW will launch 1 daemon and N client windows (with N being the number of hosts to SSH onto).<br>
Key-strokes performed while having the daemon console focussed will be sent to all clients simoultaneously and be replayed by them.<br>
Focussing a client will cause any key-strokes to be sent to this client only.

## Download/Installation
csshW is a portable application and is not installed.<br>
To download the csshW application refer to the [Releases 📦](https://github.com/whme/csshw/releases) page.

## Contributing
csshW uses pre-commit githooks to enforce good code style.<br>

### Setup development environment
#TODO

[^1]: WSL-2 is the only console application that supports writing to its input buffer.
    Other application I tried:
    - git for windows
    - windows cmd
    - windows powershell