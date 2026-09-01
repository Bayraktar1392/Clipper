# Clipper

A small native GTK4/libadwaita Linux desktop app for downloading Twitch Clips and YouTube videos in the best quality available — mix and match both kinds of links in the same box.

The UI is intentionally sober and entirely native: no glow, no fake elevation, no extra outlines — every surface and color comes straight from the system's own light/dark theme and accent color. Paste one or many Twitch Clip or YouTube links into the single input box, the queue builds itself, and **Download** does the rest. Each item gets its own row with live progress, and up to three items download at the same time by default.

## Features

- Native Rust + GTK4 + libadwaita UI, laid out like a GNOME "Files"/Nautilus dialog: a boxed list of rows with brand-tinted icons, an empty-state page, a floating toast on completion with an **Open Folder** action, and an inline hint for problems instead of permanent on-screen text.
- **Twitch Clips and YouTube videos in the same input box** — paste any mix of `twitch.tv`/`clips.twitch.tv` clip links and `youtube.com`/`youtu.be` video, Shorts or Live links; each is recognized automatically and shown with its own brand color.
- Duplicate and unsupported URLs (channel pages, playlists, the homepage, etc.) are filtered out automatically as you type.
- **Concurrent downloads by default** — up to 3 items download at once (tunable via `MAX_CONCURRENT_DOWNLOADS` in `src/app.rs`), each with its own live progress row.
- Uses `yt-dlp` with `bestvideo*+bestaudio/best`; no artificial upscaling or re-encoding — FFmpeg is only invoked by yt-dlp itself when it needs to merge separate video/audio streams.
- Per-row progress, transfer speed and ETA while downloading; the saved path (or the error) is available as a tooltip once a row finishes.
- **Click a finished row to reveal it** in the system file manager, or use the **Open Folder** action on the completion toast to jump to `~/Downloads/Clipper/`.
- **The queue is persisted** — your pasted links are restored automatically the next time you launch the app.
- Cancelling stops every in-flight `yt-dlp` subprocess and any not-yet-started jobs cleanly.
- Output goes to `~/Downloads/Clipper/`.

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

This repository is prepared for GitHub Actions. Push a semantic version tag such as `v1.3.0` and the release workflow builds the application and publishes the source archive + checksum.

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
