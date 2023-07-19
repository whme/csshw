<h1 style="border: none; text-align: center;">
    <p style="margin-bottom: 1px;">csshW</p>
    <hr style="margin: auto; margin-bottom: 2px; margin-top: 0; width: 10em; border-radius: 50% / 50%;">
    <p style="font-size: 50%;">
        Cluster SSH tool for Windows inspired by
        <a
            href="https://github.com/brockgr/csshx"
            style="color: inherit; font-style: italic;"
        > csshX</a>
    </p>
</h1>

<div style="width: 50%; min-width: 350px; margin: auto; text-align: left;">
    <h2>Pre-requisites</h2>
    <ul>
        <li>A working <a href="https://learn.microsoft.com/en-us/windows/wsl/install">WSL-2</a> installation</li>
        <li><code>Default terminal application</code> is set to <code>Windows Console Host</code> in the windows Terminal Startup Settings (Windows 11 only)</li>
    </ul>
    <h2>Overview</h2>
    <p>
        csshW consist of 3 executables:
        <ul>
            <li><code>csshw</code> - a launcher that starts the daemon application and serves as main entry point</li>
            <li><code>csshw-daemon</code> - spawns and positions the client windows and propagates any key-strokes to them</li>
            <li><code>csshw-client</code> - establishes an SSH connection and replays key-strokes received from the daemon
        </ul>
        csshW will launch 1 daemon and N client windows (with N being the number of hosts to SSH onto).<br>
        Key-strokes performed while having the daemon console focussed will be sent to all clients simoultaneously and be replayed by them.<br>
        Focussing a client will cause any key-strokes to be sent to this client only.
    </p>
    <h2>Download/Installation</h2>
    csshW is a portable application and is not installed.<br>
    To download the csshW application refer to the <a href="https://github.com/whme/csshw/releases">Releases 📦</a> page.
    <h2>Contributing</h2>
    csshW uses pre-commit githooks to enforce good code style.<br>
    <h3>Setup development environment</h3>
    #TODO
</div>