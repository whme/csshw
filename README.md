# csshW
_Cluster SSH tool for Windows inspired by [csshX](https://github.com/brockgr/csshx)_

![csshw demo](https://github.com/whme/csshw/blob/84570f8dd767e17df0027f11a19e5e3276718787/demo/csshw.gif)[^1][^2]

## Pre-requisites
- ``Default terminal application`` is set to ``Windows Console Host`` in the windows Terminal Startup Settings (Windows 11 only)

## Overview
csshW will launch 1 daemon and N client windows (with N being the number of hosts to SSH onto).<br>
Key-strokes performed while having the daemon console focussed will be sent to all clients simoultaneously and be replayed by them.<br>
Focussing a client will cause any key-strokes to be sent to this client only.

## Usage

```cmd
csshw.exe --help
Cluster SSH tool for Windows inspired by csshX

USAGE:
    csshw.exe [OPTIONS] [HOSTS]... [SUBCOMMAND]

ARGS:
    <HOSTS>...    Hosts to connect to

OPTIONS:
    -h, --help                   Print help information
    -u, --username <USERNAME>    Username used to connect to the hosts
    -V, --version                Print version information

SUBCOMMANDS:
    client
    daemon
    help      Print this message or the help of the given subcommand(s)
```
Example:
`csshw.exe -u root hosta.dev hostb.dev hostc.dev`

We recommend using the [ssh_config](https://linux.die.net/man/5/ssh_config) for any configurations like default username etc.

### Configuration

`csshw-config.toml` contains all relevant configurations and is located in the same directory as the executable.
It is automatically created with default values if not present. A config containing invalid data will automatically be owerwritten.

#### clusters
An array of clusters that can be used to alias a set of host names to a specific tag:
```toml
clusters = [
    { name = "dev", hosts = ["hosta.dev", "root@hostb.dev", "hostc.dev"] }
]
```
Clusters may be nested, but be aware of recursive clusters which are not checked for.

#### client
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

##### ssh_config_path
The full qualified path where your ssh configuration can be found.

##### program
Which executable will be used to establish ssh connections.

##### arguments
Additional arguments specified to the chosen program.

##### username_host_placeholder
Placeholder string that indicates where the `username@host` string should be inserted in the program arguments.

## Download/Installation
csshW is a portable application and is not installed.<br>
To download the csshW application refer to the [Releases 📦](https://github.com/whme/csshw/releases) page.

## Contributing
csshW uses pre-commit githooks to enforce good code style.<br>
Install them via ``git config --local core.hooksPath .githooks/``.

[^1]: The searchbar used to launch csshw in the demo clip is [keypirinha](https://keypirinha.com/).
[^2]: The tool to show key presses in the demo clip is [carnac the magnificent](http://carnackeys.com/).
