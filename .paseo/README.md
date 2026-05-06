# `.paseo/` - per-contributor paseo state

This directory holds per-contributor state used by
[paseo](https://paseo.dev) when it spawns AI coding agents on this
repository. It is intentionally checked in (with this README only) so
the directory exists in fresh clones - every other file here is
gitignored.

## `gh-token`

A fine-grained GitHub Personal Access Token, used to scope the
`gh` CLI of paseo-spawned AI agents down to least-privilege.

`cargo xtask inject-agent-token` reads this file at worktree
creation time (wired up via `paseo.json`'s `worktree.setup`) and
injects the token as `GH_TOKEN` into the agent's environment via
`.claude/settings.local.json`.

The full setup procedure (how to mint the token, which scopes to
grant, how to rotate it) lives in
[`CONTRIBUTING.md`](../CONTRIBUTING.md) under
"AI agent GitHub auth (optional)".
