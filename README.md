# csshW - Cluster SSH tool for Windows inspired by csshX

## Setup githooks
`git config --local core.hooksPath .githooks/`

## Format
`cargo fmt`

## Build
`cargo build`

## Run debug version
`csshw.exe [args]`
(It's a symlink to `/target/debug/csshw.exe`)

## Build and run
`cargo run`

## Format + Build + Run
`cargo fmt; cargo build; if ($?) { .\csshw.exe foo bar }`

# Windows 11

Make sure to set the ``Default terminal application`` in the Terminal Startup Settings to ``Windows Console Host``.
