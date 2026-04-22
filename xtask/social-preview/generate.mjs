// Social preview PNG generator.
//
// Runs inside the pinned `mcr.microsoft.com/playwright:<tag>` Docker image
// invoked by `xtask/src/social_preview.rs`. The container's working
// directory is the workspace root (bind-mounted from the host); template,
// logo, and font paths are resolved relative to that CWD. `OUT_PATH` is
// passed through from the Rust xtask and is typically an absolute
// container path (for example, under `/workspace/...`).
//
// Inputs (environment):
//   OUT_PATH      — output path for the PNG inside the container
//                   filesystem (required, set by the Rust xtask; may be
//                   absolute).
//   GITHUB_TOKEN  — optional; enables authenticated GitHub API requests.
//   GITHUB_OWNER  — defaults to "whme".
//   GITHUB_REPO   — defaults to "csshw".
//
// Outputs:
//   A 1280×640 PNG written to OUT_PATH.
//
// Design goals:
//   - Fail loudly on any network, file, or Playwright error — no silent
//     fallbacks. Unknown language colors are the one exception (warn +
//     grey fallback).
//   - No HTTP libraries — use Node's built-in `fetch`. Per run we make
//     up to three outbound calls: GitHub repo metadata, GitHub language
//     bytes, and (on cache miss) the linguist colour map from
//     ozh/github-colors.

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { chromium } from "@playwright/test";

const OWNER = process.env.GITHUB_OWNER || "whme";
const REPO = process.env.GITHUB_REPO || "csshw";
const OUT_PATH = process.env.OUT_PATH;
if (!OUT_PATH) {
  console.error("OUT_PATH environment variable is required.");
  process.exit(1);
}

const TEMPLATE_PATH = "templates/social-preview.html";
const LOGO_PATH = "res/csshw.svg";
const FONT_PATH = "res/dejavu-sans-mono.book.ttf";
// ozh/github-colors is a long-running community mirror of the colors
// embedded in github-linguist/linguist's `languages.yml`, published as
// plain JSON. Using it at runtime keeps us from committing and manually
// maintaining a snapshot in this repository.
const LINGUIST_COLORS_URL =
  "https://raw.githubusercontent.com/ozh/github-colors/master/colors.json";
const UNKNOWN_LANGUAGE_COLOR = "#cccccc";
const OTHER_BUCKET_COLOR = "#ededed";
const VIEWPORT = { width: 1280, height: 640 };
// Final PNG matches GitHub's recommended social preview size of
// 1280×640 (see https://docs.github.com/en/repositories/managing-your-
// repositorys-settings-and-features/customizing-your-repository/
// customizing-your-repositorys-social-media-preview).
const DEVICE_SCALE_FACTOR = 1;
// Path (relative to the workspace root) where the linguist colour map is
// cached after the first successful fetch. `target/` is gitignored, so
// the cache is never committed. Delete the file to force a refresh.
const LINGUIST_COLORS_CACHE = "target/social-preview/linguist-colors.json";

async function githubFetch(pathname) {
  const url = `https://api.github.com/${pathname}`;
  const headers = {
    Accept: "application/vnd.github+json",
    "User-Agent": "csshw-social-preview",
    "X-GitHub-Api-Version": "2022-11-28",
  };
  if (process.env.GITHUB_TOKEN) {
    headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  }
  const res = await fetch(url, { headers });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(
      `GitHub API ${url} returned ${res.status} ${res.statusText}: ${body}`,
    );
  }
  return await res.json();
}

/** Format a star count as 742, 1.2k, 12.4k, 1.3M. */
function formatStars(n) {
  if (n < 1000) return String(n);
  if (n < 10_000) return (n / 1000).toFixed(1).replace(/\.0$/, "") + "k";
  if (n < 1_000_000) return (n / 1000).toFixed(1).replace(/\.0$/, "") + "k";
  return (n / 1_000_000).toFixed(1).replace(/\.0$/, "") + "M";
}

/** HTML-escape a string for safe substitution into the template. */
function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/**
 * Convert raw `{language: bytes}` into an array of
 * `{name, pct, color}` sorted by pct desc, normalised to 100, with
 * sub-0.5% entries folded into an `Other` bucket.
 */
function buildLanguages(bytesByLang, colorsByLang) {
  const total = Object.values(bytesByLang).reduce((a, b) => a + b, 0);
  if (total === 0) return [];
  const entries = Object.entries(bytesByLang)
    .map(([name, bytes]) => {
      let color = colorsByLang[name];
      if (!color) {
        console.warn(
          `ozh/github-colors has no entry for "${name}"; using ${UNKNOWN_LANGUAGE_COLOR}.`,
        );
        color = UNKNOWN_LANGUAGE_COLOR;
      }
      return { name, pct: (bytes / total) * 100, color };
    })
    .sort((a, b) => b.pct - a.pct);

  const main = entries.filter((e) => e.pct >= 0.5);
  const otherPct = entries
    .filter((e) => e.pct < 0.5)
    .reduce((a, b) => a + b.pct, 0);
  const result = [...main];
  if (otherPct > 0) {
    result.push({ name: "Other", pct: otherPct, color: OTHER_BUCKET_COLOR });
  }
  // Normalise to sum to exactly 100 after rounding drift.
  const sum = result.reduce((a, b) => a + b.pct, 0);
  if (sum > 0) {
    const scale = 100 / sum;
    result.forEach((e) => {
      e.pct = Math.round(e.pct * scale * 10) / 10;
    });
  }
  // Drop entries that rounded to zero — they contribute no visible
  // segment and just clutter the legend.
  return result.filter((e) => e.pct > 0);
}

async function dataUri(path, mime) {
  const bytes = await readFile(path);
  return `data:${mime};base64,${bytes.toString("base64")}`;
}

/**
 * Load the linguist colour map, preferring a cached copy under `target/`
 * to keep subsequent runs offline. On cache miss, fetches from
 * ozh/github-colors and persists a flat `{ "<Language>": "#rrggbb" }`
 * JSON file for future runs. That repo publishes entries in the form
 * `{ "<Language>": { "color": "#rrggbb", "url": "..." } }` with
 * `color: null` for languages linguist does not assign a hue to — those
 * are dropped so `buildLanguages` falls back to the unknown-language
 * colour instead of crashing. Delete the cache file to force a refresh.
 */
async function fetchLinguistColors() {
  try {
    const cached = await readFile(LINGUIST_COLORS_CACHE, "utf-8");
    console.log(`Using cached linguist colours from ${LINGUIST_COLORS_CACHE}`);
    return JSON.parse(cached);
  } catch (err) {
    if (err.code !== "ENOENT") throw err;
  }
  console.log(`Fetching linguist colours from ${LINGUIST_COLORS_URL}`);
  const res = await fetch(LINGUIST_COLORS_URL);
  if (!res.ok) {
    throw new Error(
      `Linguist colors ${LINGUIST_COLORS_URL} returned ${res.status} ${res.statusText}`,
    );
  }
  const raw = await res.json();
  const colors = {};
  for (const [name, entry] of Object.entries(raw)) {
    if (entry && typeof entry.color === "string") {
      colors[name] = entry.color;
    }
  }
  await mkdir(dirname(LINGUIST_COLORS_CACHE), { recursive: true });
  await writeFile(LINGUIST_COLORS_CACHE, JSON.stringify(colors));
  return colors;
}

async function main() {
  const [repo, languages, colors, templateRaw] = await Promise.all([
    githubFetch(`repos/${OWNER}/${REPO}`),
    githubFetch(`repos/${OWNER}/${REPO}/languages`),
    fetchLinguistColors(),
    readFile(TEMPLATE_PATH, "utf-8"),
  ]);

  const logoDataUri = await dataUri(LOGO_PATH, "image/svg+xml");
  const fontDataUri = await dataUri(FONT_PATH, "font/ttf");
  const langPayload = buildLanguages(languages, colors);

  const replacements = {
    "{{REPO_NAME}}": escapeHtml(repo.name || REPO),
    "{{REPO_FULL_NAME}}": escapeHtml(repo.full_name || `${OWNER}/${REPO}`),
    "{{REPO_DESCRIPTION}}": escapeHtml(repo.description || ""),
    "{{STAR_COUNT}}": escapeHtml(formatStars(repo.stargazers_count || 0)),
    "{{LOGO_DATA_URI}}": logoDataUri,
    "{{FONT_DATA_URI}}": fontDataUri,
    "{{LANGUAGES_JSON}}": JSON.stringify(langPayload),
  };
  let html = templateRaw;
  for (const [key, value] of Object.entries(replacements)) {
    html = html.split(key).join(value);
  }

  const absOut = resolve(OUT_PATH);
  await mkdir(dirname(absOut), { recursive: true });

  const browser = await chromium.launch();
  try {
    const context = await browser.newContext({
      viewport: VIEWPORT,
      deviceScaleFactor: DEVICE_SCALE_FACTOR,
    });
    const page = await context.newPage();
    // Capture browser-side exceptions (template script errors, broken
    // data URIs, etc.) so a silent failure in the inline <script> in
    // templates/social-preview.html fails the build instead of silently
    // writing an incomplete card.
    let pageError = null;
    page.on("pageerror", (err) => {
      pageError = err;
    });
    await page.setContent(html, { waitUntil: "networkidle" });
    if (pageError) throw pageError;
    const png = await page.screenshot({
      type: "png",
      clip: { x: 0, y: 0, width: VIEWPORT.width, height: VIEWPORT.height },
      omitBackground: false,
    });
    await writeFile(absOut, png);
  } finally {
    await browser.close();
  }
  console.log(`Wrote ${absOut}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
