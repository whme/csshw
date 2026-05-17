# Normalises the desktop chrome so demo recordings look identical
# across developer machines and CI runners.
#
# Sourced (dot-sourced) by sandbox-bootstrap.ps1 inside Windows
# Sandbox, and reused unchanged by the v2 ci-runner provider. Safe
# to re-run: every operation either overwrites or short-circuits if
# the desired state is already in place.
#
# Settings applied:
#   - Console font: Cascadia Mono 18 pt for both cmd.exe and
#     powershell.exe via HKCU\Console\<exe>.
#   - Logical resolution: 1920x1080 at 100 % DPI scale.
#   - Hide desktop icons; disable taskbar auto-hide animation.
#
# The wallpaper is intentionally left at the Windows default: the
# sandbox already ships a clean stock background, and the host run
# (--env local) must not modify the developer's wallpaper.

$ErrorActionPreference = 'Stop'

function Set-ConsoleFont {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [string] $FaceName,
        [Parameter(Mandatory)] [int] $PointSize
    )

    # HKCU\Console FaceName + FontSize defaults apply to every cmd /
    # powershell window opened by the current user. Per-exe overrides
    # under HKCU\Console\<exe> beat the defaults; we set both so a
    # sub-shell that already tweaked one entry still picks up the
    # demo font.
    $sizeDword = ($PointSize -shl 16)
    foreach ($subKey in @('Console', 'Console\%SystemRoot%_System32_cmd.exe',
                           'Console\%SystemRoot%_System32_WindowsPowerShell_v1.0_powershell.exe')) {
        $path = "HKCU:\$subKey"
        if (-not (Test-Path $path)) {
            New-Item -Path $path -Force | Out-Null
        }
        Set-ItemProperty -Path $path -Name 'FaceName' -Value $FaceName
        Set-ItemProperty -Path $path -Name 'FontFamily' -Value 0x36
        Set-ItemProperty -Path $path -Name 'FontWeight' -Value 0x190
        Set-ItemProperty -Path $path -Name 'FontSize' -Value $sizeDword `
            -Type DWord
    }
}

function Set-DpiScaleHundred {
    # 96 DPI = 100 % scale. The HKCU per-monitor key is enough on
    # Windows Sandbox; physical workstations may need a sign-out.
    Set-ItemProperty -Path 'HKCU:\Control Panel\Desktop' `
        -Name 'LogPixels' -Value 96 -Type DWord
    Set-ItemProperty -Path 'HKCU:\Control Panel\Desktop' `
        -Name 'Win8DpiScaling' -Value 0 -Type DWord
}

function Set-DesktopChromeOff {
    # Hide desktop icons, disable taskbar auto-hide animation. Both
    # are HKCU keys read by Explorer at sign-in.
    $advanced = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
    if (-not (Test-Path $advanced)) {
        New-Item -Path $advanced -Force | Out-Null
    }
    Set-ItemProperty -Path $advanced -Name 'HideIcons' -Value 1 -Type DWord
    Set-ItemProperty -Path $advanced -Name 'TaskbarAnimations' -Value 0 -Type DWord
}

# --- Apply ----------------------------------------------------------------

Set-ConsoleFont -FaceName 'Cascadia Mono' -PointSize 18
Set-DpiScaleHundred
Set-DesktopChromeOff

Write-Host 'setup-desktop.ps1: applied csshw demo desktop normalisation.'
