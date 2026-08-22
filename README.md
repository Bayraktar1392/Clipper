# Clipper

A small native GTK4/libadwaita Linux desktop app for downloading Twitch Clips in the best quality available.

The UI follows GNOME/Files-style minimalism: no static labels, no extra buttons, no boilerplate text. Paste one or many Twitch Clip URLs into the single input box, the queue builds itself, and **Download** does the rest. Each clip gets its own row with live progress, and up to three clips download at the same time by default.

## Features

- Native Rust + GTK4 + libadwaita UI, laid out like a GNOME "Files"/Nautilus dialog: a boxed list of rows with icons, an empty-state page, a floating toast on completion with an **Open Folder** action, and an inline banner for problems instead of permanent on-screen text.
- One or many Twitch Clip URLs in a single input box — just paste, no separate "add" step.
- Duplicate and invalid URLs are filtered out automatically as you type.
- **Concurrent downloads by default** — up to 3 clips download at once (tunable via `MAX_CONCURRENT_DOWNLOADS` in `src/app.rs`), each with its own live progress row.
- Uses `yt-dlp` with `bestvideo*+bestaudio/best`; no artificial upscaling or re-encoding — FFmpeg is only invoked by yt-dlp itself when it needs to merge separate video/audio streams.
- Per-row progress, transfer speed and ETA while downloading; the saved path (or the error) is available as a tooltip once a row finishes.
- Cancelling stops every in-flight `yt-dlp` subprocess and any not-yet-started jobs cleanly.
- Output goes to `~/Downloads/Twitch Clips/`.

## System requirements

Arch Linux:

```bash
sudo pacman -S --needed base-devel rust pkgconf gtk4 libadwaita yt-dlp ffmpeg
```

## Build from source

```bash
cargo fmt --all
cargo check
cargo test
cargo clippy --all-targets --all-features
cargo build --release
./target/release/clipper
```

## Install locally

```bash
sudo install -Dm755 target/release/clipper /usr/bin/clipper
sudo install -Dm644 assets/clipper.desktop /usr/share/applications/clipper.desktop
sudo install -Dm644 assets/icons/hicolor/scalable/apps/clipper.svg /usr/share/icons/hicolor/scalable/apps/clipper.svg
```

Then run:

```bash
clipper
```

## GitHub release

This repository is prepared for GitHub Actions. Push a semantic version tag such as `v1.2.0` and the release workflow builds the application and publishes the source archive + checksum.

## AUR

The AUR packaging files are in `packaging/aur/`.

Before submission, replace `YOUR_GITHUB_USERNAME` and maintainer metadata in `packaging/aur/PKGBUILD`, create the GitHub release, replace the source checksum, and regenerate `.SRCINFO`:

```bash
cd packaging/aur
makepkg --printsrcinfo > .SRCINFO
makepkg -Cfsi
namcap PKGBUILD
namcap clipper-*.pkg.tar.zst
```

## License

MIT
