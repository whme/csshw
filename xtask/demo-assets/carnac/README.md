# Carnac attribution

The csshw demo recorder uses [Carnac](https://github.com/Code52/carnac)
to render a keystroke overlay on the bottom strip of the captured
desktop. Carnac is a third-party tool by Code52 contributors,
distributed under the Microsoft Public License (MS-PL); see
[`LICENSE`](LICENSE) in this directory for the verbatim text.

## How Carnac is consumed

We do **not** vendor the Carnac binary in this repository. Instead,
[`xtask/src/demo/bin.rs`](../../src/demo/bin.rs) holds a SHA-pinned
download URL for the upstream `carnac.2.3.13.zip` release artifact.
On the first `cargo xtask record-demo` invocation the recorder
downloads the archive into `target/demo/bin/carnac/`, verifies the
SHA-256 against the constant in `bin.rs`, and extracts the inner
NuGet package to expose `lib/net45/Carnac.exe`. Subsequent runs hit
the warm cache and skip the network entirely.

## Licensing notes

MS-PL section 3(C) requires that every distribution preserve
attribution notices that ship with the software. The recorded GIF
embeds the Carnac overlay (visible Carnac branding in the corner
strip), so the rendered GIF qualifies as distributing a portion of
Carnac. Keeping this LICENSE + README pair in the source tree is
how csshw satisfies that obligation; if you redistribute the
recorded GIF on its own, please carry the same attribution forward.

We deliberately download from upstream rather than mirror the binary
so refreshing the pin is a one-line constant change instead of a
binary commit. The SHA pin guarantees that a tampered CDN cannot
silently swap the overlay for a different binary - a mismatch fails
the recorder loudly with a `bin: SHA-256 mismatch` error.
