# AUR packaging

This directory contains the Arch Linux package metadata for `clipper`.

Before submitting, replace `YOUR_GITHUB_USERNAME`, maintainer name, and email in `PKGBUILD`, then regenerate `.SRCINFO`:

```bash
makepkg --printsrcinfo > .SRCINFO
```

For a real release, replace `sha256sums=('SKIP')` with the checksum of the GitHub source archive (for example using `updpkgsums`).

Local validation:

```bash
makepkg -Cfsi
namcap PKGBUILD
namcap clipper-*.pkg.tar.zst
```

For a clean-chroot validation, use `pkgctl build` or the Arch devtools clean-chroot workflow.
