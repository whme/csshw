Fixed the released 0.19.0 binary panicking with `Failed to create process`
when launched. The daemon and client child consoles were spawned by the
hard-coded name `csshw.exe`, but the release workflow packaged the binary
as `csshw.<version>.exe`, so `CreateProcess` could not find it. Child
consoles are now spawned via `std::env::current_exe()`, and the release
workflow keeps the executable inside the archive named `csshw.exe`.
