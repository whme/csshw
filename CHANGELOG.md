# Changelog

<!-- changelogging: start -->

## 0.18.1 (2025-10-07)

### Bug Fixes

- Fix wrong example for the documentation of the -p/--port CLI option


## 0.18.0 (2025-10-07)

### Features

- Dedicated ports per host are now supported. E.g.: csshw.exe -p 33 host1:11 host2:22 host3. (#61)


## 0.17.0 (2025-04-15)

### Features

- Dedicated usernames per host are now supported. E.g.: `csshw.exe -u userA user1@host1 hostA1 hostA2`. (#49)
- Hosts/Cluster Tag(s) now support [brace expansion](https://www.gnu.org/software/bash/manual/html_node/Brace-Expansion.html).
  E.g. `csshw.exe "host{1..3}" host5` which will be resolved to `csshw.exe host1 host2 host3 host5`.
  Note: the windows Powershell and maybe other windows shells do not support brace expansion but interpret curly braces (`{}`) and other special characters which might cause issue.
  To avoid this, the hostname using brace expansion should be quoted as shown in the example above. (#46)


## 0.16.0 (2025-03-01)

### Features

- csshW is now per monitor DPI aware. This means the Windows operating system will automatically scale daemon and client console windows according to the system settings
- The control mode `[c]reate window(s)` option now also supports cluster tags. (#37)

### Bug Fixes

- Fixed a bug that would cause client console windows to flicker when using the control mode `[c]reate window(s)` option
- Fixed a bug that would cause key presses to be registered in control mode even if a control key was pressed
- Fixed a bug that would sometimes break the rendering of client console windows after additional windows were added via the control mode `[c]reate window(s)` option
- Fixed a bug that would prevent the daemon console window from receiving focus after using the control mode `[c]reate window(s)` option


## 0.15.2 (2024-01-08)

### Bug Fixes

- Fixed a bug that would prevent the default terminal application setting from being overwritten if the default value had never been changed before. (#31)


## 0.15.1 (2024-01-07)

### Bug Fixes

- Emit a warning instead of panicing if we cannot read the `Default terminal application` configuration from the registry


## 0.15.0 (2024-01-07)

### Features

- Automatically change default terminal application setting, making the `Default terminal application` setting change pre-requisite obsolete (#28)


## 0.14.0 (2024-01-02)

### Features

- Update ssh2-config to 0.2.3
- Update pre-requisites to better reflect incompatibility with Windows Terminal (#26)


## 0.13.0 (2023-05-21)

### Bug Fixes

- Fixed a bug that would prevent the daemon console from closing after all client windows closed
- Fixed paste bug that would cause daemon and clients to crash when pasting large amounts of text


## 0.12.0 (2023-05-18)

### Features

- Client console windows now arrange themselves immediately after being launched, no longer waiting for all client windows to be launched before rearranging themselves
- Should a client fail to connect the window will now stay open until receiving `Shift-Alt-C` from the daemon

### Bug Fixes

- Fixed a bug that would cause client windows to have their title replaced with a generic one
- Fixed a bug that would prevent daemon and client console windows from arranging themselves correctly on windows 10


## 0.11.0 (2023-05-04)

### Features

- Added `copy active [h]ostname(s)` option to control mode. Populates the clipboard with a list of hostnames of the currently active client windows
- Added `[c]reate window(s)` option to control mode. Adds a new window for each specified hostname (space separated list of hostnames). Uses the same user as for the existing client windows
- Reduced binary size


## 0.10.0 (2023-04-26)

### Features

- Improved copy/paste performance, highly reducing the likelyhood for crashes when pasting small to medium sized text snippets
- Added `SSH client` to the list or pre-requisites. (Note: an SSH client was always a pre-requisite to use csshw, only the documentation was missing)


## 0.9.4 (2023-04-25)

### Features

- Added a new CLI option `-d/--debug` that will cause the csshw applications to write any crash/panic information into a dedicated logfile in the `logs` folder (logfile name format: `<utc_datetime>_<application_name>.log`)

### Bug Fixes

- Fixed a bug with the daemon/client window synchronization that would prevent client windows from being minimized


## 0.9.3 (2023-03-16)

### Bug Fixes

- Fixed a bug with the daemon/client window synchronization that would cause client windows to no longer be moved into the foreground together with the daemon window after having received manual focus


## 0.9.2 (2023-03-05)

### Features

- Daemon console window is now included in the retile mode

### Bug Fixes

- Fixed [Mio's tokens for named pipes may be delivered after deregistration](https://github.com/whme/csshw/security/dependabot/2)


## 0.9.0 (2022-09-21)

### Features

- Client consoles will now be moved to the foreground when the daemon console receives focus


## 0.8.1 (2022-09-21)

### Features

- Daemon console now features a rudimentary control mode which can be activated with `Ctrl + A` and exited with `Esc` (#12)
- Control mode has `[r]etile` option which will reposition all remaining client windows (#12)
- Daemon console color is now configurable


## 0.7.0 (2022-08-26)

### Features

- Change default daemon console placement to be static at the bottom on the screen (#13)
- Daemon console height is now configurable (#13)
- Client arrangement is now configurable (called `aspect_ratio_adjustement`)


## 0.6.0 (2022-08-23)

### Features

- Added support for cluster tags (#10)


## 0.5.1 (2022-08-11)

### Features

- Added usage example to README


## 0.5.0 (2022-08-09)

### Features

- Moved the config file to the same directory as the executable, now truly making csshW a portable application
- Improved window layout
- Moved daemon console window to bottom/bottom right
- Improved Copy&Paste behavior (small text snippets can be copied; larger ones still cause a crash)


## 0.4.0 (2022-08-06)

### Features

- csshW now ships in a single executable
- Window arrangement has been overhauled


## 0.3.1 (2022-08-05)

Removed the WSL2 dependency 🥳
csshW is still in early development, please report any bugs or better yet submit pull requests with fixes 🚀


## 0.2.0 (2022-08-04)

Added an experimental ssh launcher ...


## 0.1.0 (2022-07-24)

The very first release 📦
csshW is still in early development, please report any bugs or better yet submit pull requests with fixes 🚀
