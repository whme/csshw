# Changelog

<!-- changelogging: start -->

## 0.18.1 (2025-10-07)

### Bug Fixes

- Fix wrong example for the documentation of the -p/--port CLI option


## 0.18.0 (2025-10-07)

### Features

- Dedicated ports per host are now supported. E.g.: csshw.exe -p 33 host1:11 host2:22 host3. (#61)


## 0.17.0 (2025-04-15)

### Features

- Dedicated usernames per host are now supported. E.g.: `csshw.exe -u userA user1@host1 hostA1 hostA2`. (#49)
- Hosts/Cluster Tag(s) now support [brace expansion](https://www.gnu.org/software/bash/manual/html_node/Brace-Expansion.html).
  E.g. `csshw.exe "host{1..3}" host5` which will be resolved to `csshw.exe host1 host2 host3 host5`.
  Note: the windows Powershell and maybe other windows shells do not support brace expansion but interpret curly braces (`{}`) and other special characters which might cause issue.
  To avoid this, the hostname using brace expansion should be quoted as shown in the example above. (#46)
