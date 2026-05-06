# Social preview tooling

This directory contains the non-Rust tooling invoked by
`cargo xtask generate-social-preview`. The Rust side
(`xtask/src/social_preview.rs`) is a thin `docker run` orchestrator; all
HTTP, template substitution, and screenshotting happens here, inside the
pinned `mcr.microsoft.com/playwright:<tag>` Docker image.

## What's in here

- `generate.mjs` - Node ESM entry point. Reads
  `templates/social-preview.html`, fetches GitHub repo + language data,
  renders the card via Playwright + Chromium, and writes PNG to
  `$OUT_PATH` (supplied by the Rust xtask).
- `package.json` / `package-lock.json` - pin `@playwright/test` to the
  numeric part of the Playwright Docker tag. **Both are committed.**
- `node_modules/` - gitignored, populated on first invocation.

The generated PNG is written to `target/social-preview/social-preview.png`
by default - under Cargo's build-artifact directory, which is already
`.gitignore`d. Override with `--out <PATH>`.

Language -> colour mappings are fetched from
[ozh/github-colors][ozh-colors] at runtime (a JSON mirror of
github-linguist's `languages.yml`), so no colour snapshot lives in this
repository. Unknown languages fall back to `#cccccc` with a warning.

[ozh-colors]: https://github.com/ozh/github-colors

## Running

From the workspace root, on any host with Rust, Cargo, and Docker:

```sh
cargo xtask generate-social-preview
# or
cargo xtask generate-social-preview --out path/under/workspace.png
cargo xtask generate-social-preview --token ghp_xxx
```

The host needs **no** local Node.js, npm, or Playwright installation. The
first run performs `npm ci` inside the container (populating
`node_modules/` strictly from the committed lockfile); subsequent runs
skip that step. Each run makes up to three outbound HTTP requests: two
to `api.github.com` (repo + languages) and, on cache miss, one to
`raw.githubusercontent.com` for the linguist colour map (cached under
`target/social-preview/linguist-colors.json` on first success).

Without a `--token` / `GITHUB_TOKEN` the command still works for the
public repo (rate-limited to 60 requests/hour).

## Uploading the generated PNG

GitHub has no API for social preview uploads. To publish a new card:

1. Run `cargo xtask generate-social-preview`.
2. Open the repository's
   **Settings -> General -> Social preview**.
3. Choose **Edit -> Upload an image** and pick
   `target/social-preview/social-preview.png`.

## Version coupling: Playwright <-> Docker tag

The `@playwright/test` version in `package.json` **must** match the
numeric portion of the Docker image tag pinned in
`xtask/src/social_preview.rs::PLAYWRIGHT_IMAGE`. Playwright refuses to run
if the library and the browser binaries in the image disagree. Bump both
together in the same commit. The current pinning is:

| Docker tag                                        | `@playwright/test` |
| ------------------------------------------------- | ------------------ |
| `mcr.microsoft.com/playwright:v1.59.1-noble`      | `1.59.1`           |

## Template authoring

`templates/social-preview.html` is self-contained - no network fetches,
no `@media` queries, a single viewport of 1280x640. Designers can open
the file directly in a browser to iterate on CSS.

The only placeholders the generator substitutes are the ones listed in
`generate.mjs::replacements`. Adding a new placeholder requires editing
both the template and `generate.mjs`.
