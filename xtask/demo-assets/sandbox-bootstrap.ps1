# Bootstraps the csshw demo recording inside Windows Sandbox.
#
# Mounted folders (set up by xtask::demo::env::sandbox::render_wsb):
#   C:\demo\bin     ffmpeg / gifski / Carnac / vcredist caches (RO)
#   C:\demo\assets  this script + setup-desktop.ps1 (read-only)
#   C:\demo\out     writable: prebuilt binaries, GIF, sentinel,
#                   xtask logs all live here
#
# The host builds csshw + xtask with a statically linked MSVC
# runtime (RUSTFLAGS=-C target-feature=+crt-static) directly into
# C:\demo\out\work\target\debug\ on the writable mount. The binaries
# are visible inside the VM with no copy step and xtask's local
# provider locates csshw.exe at <workspace>\target\debug\csshw.exe
# the same way it does on a developer workstation.
#
# Flow:
#   1. Source setup-desktop.ps1 (console font, DPI, hide icons).
#   2. Run vc_redist.x64.exe /install /quiet /norestart so the
#      sandbox's System32 carries the full MSVC runtime that
#      vendored gifski.exe (and any future MSVC-built tool) needs.
#      Without this step gifski exits with STATUS_DLL_NOT_FOUND.
#   3. Optionally launch Carnac minimised for the keystroke overlay
#      (skipped when -NoOverlay is passed by the host).
#   4. Invoke the prebuilt
#      C:\demo\out\work\target\debug\xtask.exe with --env local and
#      --out pointing straight at C:\demo\out\csshw.gif so the GIF
#      lands on the writable mount and is visible on the host
#      without any in-VM copy.
#   5. Write the sentinel C:\demo\out\done.flag (`ok` on success,
#      `error: ...` on failure) so the host poll loop can release.
#   6. Trigger an immediate sandbox shutdown so the host's
#      terminate_sandbox is a no-op rather than a fallback.

[CmdletBinding()]
param(
    [switch] $NoOverlay
)

$ErrorActionPreference = 'Stop'

# Robust sentinel write: any exit path (success or failure) must
# produce C:\demo\out\done.flag, otherwise the host's
# wait_for_sentinel times out without diagnostic output. The
# sentinel is written exactly once, from the `finally` block
# below. We use `try/catch/finally` (not the older `trap` keyword)
# because a script-level `trap` does not fire for errors raised
# inside a `try` block: PowerShell treats the try as the enclosing
# handler even when there is no `catch`, so the trap never sees
# the error and `$status` would silently keep its placeholder.
$sentinel = 'C:\demo\out\done.flag'
$status = 'error: bootstrap exited unexpectedly (no completion path)'

try {
    Write-Host '[bootstrap] sourcing setup-desktop.ps1'
    . 'C:\demo\assets\setup-desktop.ps1'

    # The Windows Sandbox base image ships UCRT but not the MSVC
    # runtime DLLs. Upstream gifski.exe is dynamically linked
    # against vcruntime140.dll, which without the redist installed
    # makes the in-VM gifski invocation fail with
    # STATUS_DLL_NOT_FOUND (0xC0000135). Microsoft's standalone
    # redistributable installer is the canonical fix: it drops the
    # full VC++ runtime into the sandbox's real System32, so any
    # MSVC-built tool we vendor (gifski today, anything else
    # tomorrow) just resolves its imports through the standard DLL
    # search path. The host's xtask::demo::bin module downloads and
    # SHA-pins vc_redist.x64.exe into the read-only bin mount.
    $vcRedist = 'C:\demo\bin\vcredist\vc_redist.x64.exe'
    if (-not (Test-Path -LiteralPath $vcRedist)) {
        throw "missing $vcRedist; the host bin cache did not populate the redist"
    }
    Write-Host '[bootstrap] installing VC++ redistributable (silent)'
    # /install /quiet /norestart is the documented unattended-install
    # surface. Exit code 0 = installed, 1638 = newer version already
    # present (also a success, but the sandbox is fresh so this
    # branch is only relevant if the redist ever lands in a future
    # base image). 3010 = success but reboot pending (we don't
    # reboot the sandbox; the runtime is loadable immediately).
    $vcProc = Start-Process -FilePath $vcRedist `
        -ArgumentList @('/install', '/quiet', '/norestart') `
        -Wait -PassThru -NoNewWindow
    if ($vcProc.ExitCode -ne 0 -and $vcProc.ExitCode -ne 1638 -and $vcProc.ExitCode -ne 3010) {
        throw "vc_redist.x64.exe exited with status $($vcProc.ExitCode)"
    }
    Write-Host "[bootstrap] vc_redist exit code $($vcProc.ExitCode)"

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

    # The host's cargo_build_demo_artifacts wrote csshw.exe and
    # xtask.exe directly into the writable out mount. xtask's local
    # provider looks for csshw.exe at <workspace>\target\debug, so
    # workRoot lines up with the cargo --target-dir the host used.
    $workRoot = 'C:\demo\out\work'
    $xtaskExe = Join-Path $workRoot 'target\debug\xtask.exe'
    $csshwExe = Join-Path $workRoot 'target\debug\csshw.exe'
    if (-not (Test-Path -LiteralPath $xtaskExe)) {
        throw "missing $xtaskExe; the host build did not produce xtask.exe on the writable mount"
    }
    if (-not (Test-Path -LiteralPath $csshwExe)) {
        throw "missing $csshwExe; the host build did not produce csshw.exe on the writable mount"
    }

    Write-Host '[bootstrap] running xtask record-demo --env local'
    $env:CSSHW_DEMO_WORKSPACE = $workRoot
    # Capture stdout+stderr to files in the writable mount so the
    # host can surface them when xtask fails. The sandbox VM shuts
    # down on exit, so anything that only lived on the VM's console
    # is otherwise lost.
    $xtaskStdout = 'C:\demo\out\xtask.stdout.log'
    $xtaskStderr = 'C:\demo\out\xtask.stderr.log'
    # --out points straight at the writable mount so the GIF lands
    # on the host without a post-run copy. Intermediate .mkv and
    # frames\ end up next to it for the same reason.
    try {
        $proc = Start-Process -FilePath $xtaskExe `
            -ArgumentList @('record-demo', '--env', 'local', '--no-overlay', '--out', 'C:\demo\out\csshw.gif') `
            -WorkingDirectory $workRoot `
            -RedirectStandardOutput $xtaskStdout `
            -RedirectStandardError $xtaskStderr `
            -PassThru -Wait -NoNewWindow
        if ($proc.ExitCode -ne 0) {
            $tail = ''
            foreach ($logPath in @($xtaskStderr, $xtaskStdout)) {
                if (Test-Path -LiteralPath $logPath) {
                    $content = (Get-Content -LiteralPath $logPath -Raw -ErrorAction SilentlyContinue)
                    if ($content) {
                        # Last ~1500 chars: enough for a Rust panic /
                        # anyhow chain without bloating done.flag.
                        $start = [Math]::Max(0, $content.Length - 1500)
                        $tail = $content.Substring($start).Trim()
                        if ($tail) { break }
                    }
                }
            }
            if (-not $tail) {
                $tail = '(no output captured; see C:\demo\out\xtask.{stdout,stderr}.log on the host out mount)'
            }
            throw "xtask record-demo exited with status $($proc.ExitCode): $tail"
        }
    } finally {
        Remove-Item Env:\CSSHW_DEMO_WORKSPACE -ErrorAction SilentlyContinue
    }

    if (-not (Test-Path -LiteralPath 'C:\demo\out\csshw.gif')) {
        throw 'expected C:\demo\out\csshw.gif after record-demo, but it is missing'
    }

    $status = 'ok'
}
catch {
    # PowerShell records the exception that escaped the `try` block in
    # $_; we surface its message into the sentinel so the host's
    # wait_for_sentinel diagnostic carries the real cause instead of
    # the placeholder.
    $status = "error: $($_.Exception.Message)"
}
finally {
    Set-Content -LiteralPath $sentinel -Value $status -Encoding ASCII -NoNewline
    # Shut the sandbox down so the host's wait_for_sentinel + copy
    # is the only synchronisation point. -Force avoids the
    # "applications have unsaved changes" prompt on the
    # not-actually-real desktop.
    Stop-Computer -Force
}
