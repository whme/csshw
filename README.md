# csshW
_Cluster SSH tool for Windows inspired by [csshX](https://github.com/brockgr/csshx)_

![csshw demo](https://github.com/whme/csshw/blob/84570f8dd767e17df0027f11a19e5e3276718787/demo/csshw.gif)[^1][^2]

## Pre-requisites
- ``Default terminal application`` is set to ``Windows Console Host`` in the windows Terminal Startup Settings (Windows 11 only)

## Overview
csshW consist of 4 executables:
- ``csshw`` - a launcher that starts the daemon application and serves as main entry point
- ``csshw-daemon`` - spawns and positions the client windows and propagates any key-strokes to them
- ``csshw-client`` - starts the launcher and replays key-strokes received from the daemon
- ``csshw-launcher`` - launches the SSH application in a non blocking fashion

csshW will launch 1 daemon and N client windows (with N being the number of hosts to SSH onto).<br>
Key-strokes performed while having the daemon console focussed will be sent to all clients simoultaneously and be replayed by them.<br>
Focussing a client will cause any key-strokes to be sent to this client only.

## Download/Installation
csshW is a portable application and is not installed.<br>
To download the csshW application refer to the [Releases 📦](https://github.com/whme/csshw/releases) page.

## Contributing
csshW uses pre-commit githooks to enforce good code style.<br>
Install them via ``git config --local core.hooksPath .githooks/``.

Symlinks for the debug version of each executable are placed in the project root for easy debugging.
Format, build and execute debug version:
```
cargo fmt; cargo build; if ($?) { .\csshw.exe foo bar }
```

## Available/Verified configurations

Config path `%AppData%\csshw\config\client-config.toml`

- Default (✔️):
    ```
    ssh_config_path = 'C:\Users\<username>\.ssh\config'
    program = 'csshw-launcher.exe'
    arguments = [
        'ssh',
        '-XY'
        '{{USERNAME_AT_HOST}}'
    ]
    username_host_placeholder = '{{USERNAME_AT_HOST}}'
    ```

[^1]: The searchbar used to launch csshw in the demo clip is [keypirinha](https://keypirinha.com/).
[^2]: The tool to show key presses in the demo clip is [carnac the magnificent](http://carnackeys.com/).
