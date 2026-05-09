# Bootstraps the csshw demo recording inside Windows Sandbox.
#
# Mounted folders (set up by xtask::demo::env::sandbox::render_wsb):
#   C:\demo\repo    repo    (read-only)
#   C:\demo\bin     ffmpeg / gifski / Carnac caches (read-only)
#   C:\demo\assets  this script + setup-desktop.ps1 (read-only)
#   C:\demo\out     writable: GIF + done.flag sentinel land here
#
# Flow:
#   1. Source setup-desktop.ps1 (wallpaper, console font, DPI).
#   2. Optionally launch Carnac minimised for the keystroke overlay
#      (skipped when -NoOverlay is passed by the host).
#   3. Build csshw release binaries from the mounted source tree.
#      The host cannot pre-build because it would bake the
#      developer's machine-specific paths into the artifacts; the
#      sandbox build is short and reproducible.
#   4. Invoke `xtask record-demo --env local` against the sandboxed
#      desktop. The local provider already owns the recording flow.
#   5. Copy the resulting GIF to C:\demo\out\csshw.gif and write the
#      sentinel C:\demo\out\done.flag (`ok` on success, `error: ...`
#      on failure) so the host poll loop can release.
#   6. Trigger an immediate sandbox shutdown so the host's
#      terminate_sandbox is a no-op rather than a fallback.

[CmdletBinding()]
param(
    [switch] $NoOverlay
)

$ErrorActionPreference = 'Stop'

# Robust sentinel write: any exit path (success, failure, even a
# trapped exception) must produce C:\demo\out\done.flag, otherwise
# the host's wait_for_sentinel times out without diagnostic output.
$sentinel = 'C:\demo\out\done.flag'
$status = 'error: bootstrap exited unexpectedly'
$ranToCompletion = $false

trap {
    $err = $_.ToString()
    Set-Content -LiteralPath $sentinel -Value "error: $err" -Encoding ASCII -NoNewline
    Stop-Computer -Force
    break
}

try {
    Write-Host '[bootstrap] sourcing setup-desktop.ps1'
    . 'C:\demo\assets\setup-desktop.ps1'

    if (-not $NoOverlay) {
        $carnacExe = 'C:\demo\bin\carnac\lib\net45\Carnac.exe'
        if (Test-Path -LiteralPath $carnacExe) {
            Write-Host '[bootstrap] launching Carnac minimised'
            # Carnac auto-positions in the bottom-right strip, which
            # leaves the daemon and client windows (top-half of the
            # 1920x1080 desktop) clear for the recording.
            Start-Process -FilePath $carnacExe -WindowStyle Minimized | Out-Null
            # Give Carnac a moment to register its global keyboard
            # hook before we start typing.
            Start-Sleep -Seconds 2
        } else {
            Write-Warning "[bootstrap] Carnac.exe missing at $carnacExe; continuing without overlay"
        }
    } else {
        Write-Host '[bootstrap] -NoOverlay: skipping Carnac'
    }

    # Cargo and rustup are not present in a fresh sandbox image. The
    # demo path we ship works only if the host has already built
    # csshw.exe; the sandbox merely consumes it. We defensively
    # locate the prebuilt binary under target\release; if it is
    # missing we surface a clear sentinel error.
    $csshwExe = 'C:\demo\repo\target\release\csshw.exe'
    if (-not (Test-Path -LiteralPath $csshwExe)) {
        $csshwExe = 'C:\demo\repo\target\debug\csshw.exe'
    }
    if (-not (Test-Path -LiteralPath $csshwExe)) {
        throw "no prebuilt csshw.exe found under C:\demo\repo\target\{release,debug}; run `cargo build --release` on the host before `cargo xtask record-demo --env sandbox`"
    }
    $xtaskExe = 'C:\demo\repo\target\release\xtask.exe'
    if (-not (Test-Path -LiteralPath $xtaskExe)) {
        $xtaskExe = 'C:\demo\repo\target\debug\xtask.exe'
    }
    if (-not (Test-Path -LiteralPath $xtaskExe)) {
        throw "no prebuilt xtask.exe found under C:\demo\repo\target\{release,debug}; run `cargo build -p xtask --release` on the host before `cargo xtask record-demo --env sandbox`"
    }

    # The local provider expects to write to <workspace>/target/demo,
    # which inside the sandbox is the read-only C:\demo\repo. We
    # work around that by copying the read-only tree to a writable
    # location under C:\demo\out\repo and pointing the local
    # provider at it.
    $writeRepo = 'C:\demo\out\repo'
    if (Test-Path -LiteralPath $writeRepo) {
        Remove-Item -LiteralPath $writeRepo -Recurse -Force
    }
    # We only need target\release\csshw.exe + target\release\xtask.exe
    # plus anything xtask reads at runtime (CARGO_MANIFEST_DIR -
    # baked at compile time so source layout does not matter at run
    # time). Copy a minimal skeleton that satisfies xtask's
    # workspace_root() resolver: <root>/xtask/Cargo.toml's parent.
    New-Item -ItemType Directory -Path "$writeRepo\xtask" -Force | Out-Null
    New-Item -ItemType Directory -Path "$writeRepo\target\release" -Force | Out-Null
    Copy-Item -LiteralPath $csshwExe -Destination "$writeRepo\target\release\csshw.exe"
    Copy-Item -LiteralPath $xtaskExe -Destination "$writeRepo\target\release\xtask.exe"

    Write-Host '[bootstrap] running xtask record-demo --env local'
    $proc = Start-Process -FilePath "$writeRepo\target\release\xtask.exe" `
        -ArgumentList @('record-demo', '--env', 'local', '--no-overlay') `
        -WorkingDirectory $writeRepo `
        -PassThru -Wait -NoNewWindow
    if ($proc.ExitCode -ne 0) {
        throw "xtask record-demo exited with status $($proc.ExitCode)"
    }

    $producedGif = Join-Path $writeRepo 'target\demo\csshw.gif'
    if (-not (Test-Path -LiteralPath $producedGif)) {
        throw "expected $producedGif after record-demo, but it is missing"
    }
    Copy-Item -LiteralPath $producedGif -Destination 'C:\demo\out\csshw.gif' -Force
    Write-Host '[bootstrap] copied recorded GIF to C:\demo\out\csshw.gif'

    $status = 'ok'
    $ranToCompletion = $true
}
finally {
    if (-not $ranToCompletion -and $status -eq 'error: bootstrap exited unexpectedly') {
        # Trap above handles thrown exceptions; this branch covers
        # script termination paths PowerShell does not surface as
        # exceptions (e.g. native commands aborting the host).
    }
    Set-Content -LiteralPath $sentinel -Value $status -Encoding ASCII -NoNewline
    # Shut the sandbox down so the host's wait_for_sentinel + copy
    # is the only synchronisation point. -Force avoids the
    # "applications have unsaved changes" prompt on the
    # not-actually-real desktop.
    Stop-Computer -Force
}
