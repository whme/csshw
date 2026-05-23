Fixed `changelogging build` silently dropping all `*.bugfix.md` news
fragments from the generated changelog. The custom `[types]` mapping in
`changelogging.toml` renamed `fix` to `bugfix`, but the default `order`
list still referenced `fix`, so bugfix entries were never rendered. An
explicit top-level `order = [..., "bugfix", ...]` is now configured.
