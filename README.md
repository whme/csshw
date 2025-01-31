# csshW
_Cluster SSH tool for Windows inspired by [csshX](https://github.com/brockgr/csshx)_

![csshw demo](https://github.com/whme/csshw/blob/21d218db0d2c0366d413dad8379bdc9544f75bf8/demo/csshw.gif)[^1][^2][^3]

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0) [![CI](https://github.com/whme/csshw/actions/workflows/ci.yml/badge.svg)](https://github.com/whme/csshw/actions/workflows/ci.yml) [![Deploy Docs](https://github.com/whme/csshw/actions/workflows/deploy_docs.yml/badge.svg)](https://github.com/whme/csshw/actions/workflows/deploy_docs.yml)

## Pre-requisite
- Any SSH client (Windows 10 and Windows 11 already include a built-in SSH server and client - [docs](https://learn.microsoft.com/en-us/windows/terminal/tutorials/ssh))

## Overview
csshW will launch 1 daemon and N client windows (with N being the number of hosts to SSH onto).<br>
Key-strokes performed while having the daemon console focussed will be sent to all clients simoultaneously and be replayed by them.<br>
Focussing a client will cause any key-strokes to be sent to this client only.

## Download/Installation
csshW is a portable application and is not installed.<br>
To download the csshW application refer to the [Releases 📦](https://github.com/whme/csshw/releases) page.

## Usage

```cmd
csshw.exe --help
Cluster SSH tool for Windows inspired by csshX

Usage: csshw.exe [OPTIONS] [HOSTS]... [COMMAND]

Commands:
  client  Subcommand that will launch a single client window
  daemon  Subcommand that will launch the daemon window
  help    Print this message or the help of the given subcommand(s)

Arguments:
  [HOSTS]...  Hosts to connect to

Options:
  -u, --username <USERNAME>  Optional username used to connect to the hosts
  -d, --debug                Enable extensive logging
  -h, --help                 Print help
  -V, --version              Print version
```
Example:
`csshw.exe -u root hosta.dev hostb.dev hostc.dev`

We recommend using the [ssh_config](https://linux.die.net/man/5/ssh_config) for any configurations like default username etc.

### Configuration

`csshw-config.toml` contains all relevant configurations and is located in the same directory as the executable.
It is automatically created with default values if not present.

#### `clusters`
An array of clusters that can be used to alias a set of host names to a specific tag:
```toml
clusters = [
    { name = "dev", hosts = ["hosta.dev", "root@hostb.dev", "hostc.dev"] }
]
```
Clusters may be nested, but be aware of recursive clusters which are not checked for.

#### `client`
A collection containing client relevant configuration
``` toml
[client]
ssh_config_path = 'C:\Users\demo_user\.ssh\config'
program = 'ssh'
arguments = [
    '-XY',
    '{{USERNAME_AT_HOST}}',
]
username_host_placeholder = '{{USERNAME_AT_HOST}}'
```

##### `ssh_config_path`
The full qualified path where your ssh configuration can be found.

##### `program`
Which executable will be used to establish ssh connections.

##### `arguments`
Additional arguments specified to the chosen program.

##### `username_host_placeholder`
Placeholder string that indicates where the `username@host` string should be inserted in the program arguments.

#### `daemon`
A collection containing daemon relevant configuration
``` toml
[daemon]
height = 200
aspect_ratio_adjustement = -1.0
console_color = 207
```

##### `height`
The height of the daemon console.

##### `aspect_ratio_adjustment`
Configures whether the available screen space should rather be used horizontally or vertically.
* `> 0.0` - Aims for vertical rectangle shape.
  The larger the value, the more exaggerated the "verticality".
  Eventually the windows will all be columns.
* `= 0.0` - Aims for square shape.
* `< 0.0` - Aims for horizontal rectangle shape.
  The smaller the value, the more exaggerated the "horizontality".
  Eventually the windows will all be rows.
  `-1.0` is the sweetspot for mostly preserving a 16:9 ratio.

##### `console_color`
Configures background and foreground colors used by the daemon console.
Available are all standard windows color combinations ([windows docs](https://learn.microsoft.com/en-us/windows/console/console-screen-buffers#character-attributes)):
```
FOREGROUND_BLUE:        1
FOREGROUND_GREEN:       2
FOREGROUND_RED:         4
FOREGROUND_INTENSITY:   8
BACKGROUND_BLUE:        16
BACKGROUND_GREEN:       32
BACKGROUND_RED:         64
BACKGROUND_INTENSITY:   128
```
e.g. white font on red background: 8+4+2+1+64+128 = `207`

## Contributing
csshW uses pre-commit githooks to enforce good code style.<br>
Install them via ``git config --local core.hooksPath .githooks/``.

[^1]: The searchbar used to launch csshw in the demo clip is [keypirinha](https://keypirinha.com/).
[^2]: The tool to show key presses in the demo clip is [carnac the magnificent](https://github.com/Code52/carnac).
[^3]: The tool used to record the screen as GIF is [ScreenToGif](https://github.com/NickeManarin/ScreenToGif).
