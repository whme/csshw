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

# Dev idea

Spawn one leader process and as many follower process as hosts were given.

https://doc.rust-lang.org/std/process/struct.Command.html

