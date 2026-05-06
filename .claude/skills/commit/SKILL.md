---
name: commit
description: Write a git commit message that follows csshW's subject/body/trailer conventions, including the mandatory Co-authored-by trailer for AI-generated commits.
---

# Commit Messages

Commit messages follow the standard three-block layout: a single-line
**subject**, an optional wrapped prose **body**, and a **footer** block
of Git trailers - each block separated from the next by a blank line.

## Subject line

- Imperative mood, first word capitalized (`Add`, `Fix`, `Bump`,
  `Update`, `Support`, `Remove`, `Refactor`, `Replace`, `Improve`,
  `Migrate`).
- Optional lowercase scope prefix followed by `: ` when the change is
  confined to a single area (e.g. `client:`, `control mode:`,
  `post-pr-comment:`, `news-fragment-check:`). Mirror the scope style
  already used in `git log` - do not invent new scopes.
- No trailing period. Keep under ~72 characters.
- Do not pre-append a PR number in parentheses (`(#165)`) - GitHub's
  squash-merge adds that automatically when the PR lands.

## Body

- Separate the subject from the body with a blank line.
- Wrap lines at ~72-76 characters.
- Explain **why** the change is being made. Describe observable
  behavior before/after when relevant. "With this change ..." is a
  common and acceptable opening.
- Use `-` for bullet lists.
- Reference advisories/URLs inline in the body when fixing CVEs or
  Dependabot alerts.

## Footer (trailers)

Trailers go in a final block separated from the body by a blank line.
Order: issue/PR references first, then co-author trailers.

- **GitHub references**: use `GitHub: #<number>` - one per line, in
  the footer, never in the subject or body prose. Do not use `Fixes:`
  (legacy style).
- **AI co-authorship (MANDATORY for AI-generated commits)**: include
  a `Co-authored-by:` trailer naming the model. For example:

  ```
  Co-authored-by: Claude Opus 4.6 <noreply@anthropic.com>
  ```

  - Use the exact model name in use (e.g. `Claude Opus 4.6`,
    `Claude Sonnet 4.6`, `Claude Haiku 4.5`).
  - Email must be `<noreply@anthropic.com>`.
  - Use the Git-canonical casing `Co-authored-by:` (lowercase
    `authored`/`by`). GitHub recognizes other casings too, but
    lowercase matches Git's own trailer convention and avoids
    duplicate trailers when tooling re-adds one.
  - Emit the trailer **exactly once** - never both `Co-Authored-By:`
    and `Co-authored-by:` for the same author.

## Example

```
client: handle zero-byte pipe reads gracefully

Previously a partial read from the daemon's named pipe was treated as
an EOF and caused every client to exit. Distinguish between
`0 bytes read` (daemon gone) and `n>0 bytes read` (buffer not yet
complete) so large pastes no longer tear down the cluster.

GitHub: #142
Co-authored-by: Claude Opus 4.6 <noreply@anthropic.com>
```

## How to actually commit

Use a single-quoted heredoc (`<<'EOF'`) so backticks and `$` in the
body are not expanded by the shell:

```sh
git add <paths>        # prefer explicit paths over `git add -A`
git commit -m "$(cat <<'EOF'
<subject line>

<wrapped body>

GitHub: #<N>
Co-authored-by: <Model Name> <noreply@anthropic.com>
EOF
)"
```

Never pass `--no-verify` or `--no-gpg-sign` unless the user has
explicitly asked for it. If a pre-commit hook fails, fix the issue
and create a new commit - do not `--amend` to "retry" a commit
that never happened.
