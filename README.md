# dissh - distributed SSH

## Setup githooks
`git config --local core.hooksPath .githooks/`

## Format
`cargo fmt`

## Build
`cargo build`

## Run debug version
`ddissh.exe [args]`

## Build and run
`cargo run`

# Dev idea

Spawn one leader process and as many follower process as hosts were given.

https://doc.rust-lang.org/std/process/struct.Command.html

