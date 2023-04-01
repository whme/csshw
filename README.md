# dissh - distributed SSH

## Setup githooks
`git config --local core.hooksPath .githooks/`

## Format
`cargo fmt`

## Build
`cargo build`

## Run debug version
`dissh.exe [args]`
(It's a symlink to `/target/debug/dissh.exe`)

## Build and run
`cargo run`

## Format + Build + Run
`cargo fmt; cargo build; if ($?) { .\dissh.exe foo bar }`

# Windows 11

Make sure to set the ``Default terminal application`` in the Terminal Startup Settings to ``Windows Console Host``.
