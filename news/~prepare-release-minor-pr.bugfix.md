Fixed `cargo xtask prepare-release` pushing the version-bump commit
directly to the maintenance branch when cutting a minor release. It
now branches off a `release-X.Y.Z` branch for the version bump and
opens a GitHub pull request against the maintenance branch via
`gh pr create`. The maintenance branch itself is no longer created
blindly: the task fetches `origin`, checks whether the branch already
exists locally and/or on the remote, and creates, pushes, or checks
out as appropriate (and bails if the local copy is behind `origin`).
Patch releases continue to push directly to the current maintenance
branch.
