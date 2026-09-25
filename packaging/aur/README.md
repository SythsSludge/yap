# AUR packages

- `yap-yiffspot`: builds a tagged release from source.
- `yap-yiffspot-git`: builds the latest commit.

All install `yap` and `yiff` (the same program), completions, the man page and a desktop
entry.

## Releasing a new version

1. Bump `version` in `Cargo.toml`, commit, and push a `vX.Y.Z` tag.
2. In `yap-yiffspot`, set `pkgver`, reset `pkgrel=1`, and run `updpkgsums`.
3. `makepkg --printsrcinfo > .SRCINFO` in each, then push each folder to its AUR repo
   (`ssh://aur@aur.archlinux.org/<pkgname>.git`).
