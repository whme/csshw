Extend the Windows 10 visual-glitch fix from #189 to Windows 11. csshw
forces `conhost.exe` as the host terminal on every supported Windows
version, and the bottom-row / rightmost-column stale-cells bug after a
bulk attribute fill reproduces on the conhost shipped with Win11 too.
The post-fill `InvalidateRect` workaround is now always issued instead
of being gated on `is_windows_10`.
