# Changelog

## 2.0.0

- **Cleaner, native-first visual** — dropped the Material-You flourishes: no glow, no box shadows, no fake elevation, no extra borders; surfaces now follow the system theme and accent color. Motion cues (row/empty-state fades, status pop/shake, stack crossfades) are kept.
- **Simplified UI** — removed the clipboard auto-add toggle, the input "clear all" button, the header folder button and the placeholder label; links are still added by pasting into the input or via drag & drop.
- **Fixed a crash** — the debounced queue save tried to remove an already-fired GLib `SourceId`, which panicked on almost any interaction; saves are now coalesced with a generation counter instead.
- **Fixed the aggregate status line** — the combined speed/ETA line was built but never actually shown (a dead CSS class instead of widget visibility).
- **Performance** — queue persistence is debounced (~250 ms), and the row list is only rebuilt when the set of recognized URLs actually changes.
- Removed unused `DownloadStats` string fields and other dead code; status badges became plain colored text instead of pill chips.

## 1.4.0

- **Clipboard watch — auto-add links** — a toggle in the header (on by default) watches the system clipboard; copy any Twitch/YouTube link and it's added to the queue automatically, no pasting needed.
- **Drag & drop** — drop a link (or a block of text containing links) anywhere on the window to add it to the queue.
- **Custom download folder** — a slim "save to" row under the input opens a modern folder dialog (`GtkFileDialog`); the choice is persisted and remembered on restart. The **Open downloads folder** button now lives in the header too.
- **"Paste from clipboard" button** on the input card, alongside the clear button — both float over the field and only show when relevant.
- **Remove individual items** — each queue row gets a hover-revealed ✕ button to drop just that link out of the queue, instead of clearing everything.
- **Precise progress + live totals** — the progress bar now shows a fine-grained percentage, and a status line beneath it reports the combined download rate and a single ETA across every in-flight file ("2 files downloading — 12.4 MiB/s · 1:05 left").
- **Completion notification + chime** — when the queue finishes, a desktop notification is sent and a soft, calm sound plays (best-effort).
- **Cleaner output files** — `yt-dlp` is told to skip sidecar subtitle tracks and `.info.json` metadata files and to remux results to a single `mp4`, so downloads are bare, subtitle-free media with no leftover clutter. (Burned-in on-screen text in Twitch clips lives in the pixels and can't be removed after the fact — but any optional subtitle tracks are skipped.)
- **More motion** — queue rows fade/slide in, active status chips get a gentle breathing glow, and the download/cancel button re-tints its shadow when it toggles into destructive mode.
- Replaced the deprecated `GtkFileChooserNative` with the modern `GtkFileDialog` (keeps CI clean with `-D warnings`).
- Added unit tests for speed/ETA/byte parsing and size/clock formatting.

## 1.3.0

- **Queue persistence** — the pasted links are saved to `~/.config/clipper/queue.txt` and restored on the next launch, so a restart picks up exactly where you left off.
- **Click a finished row to reveal it** — once a download completes, its row becomes clickable and opens the saved file in the system file manager (the **Open Folder** action on the completion toast now actually does something too).
- Fixed the application not compiling against current `gtk4` (the `CssProvider::load_from_string` built-in had moved behind the `v4_12` feature and a stale trait import was left behind).
- Removed dead code: the unused `total` field on `WorkerMsg::JobStarted`, the unused `filename` in `DownloadStats`/progress template, and a redundant `create_dir_all` + local `download_directory` duplicate.
- Minor cleanups: replaced an O(n) `Vec::remove(0)` ring buffer with `VecDeque`, dropped an unnecessary full-queue `.clone()`, collapsed the single-helper `ui::components` module into `app`, and tidied a clippy `filter().next_back()`.

## 1.3.0

- **Added YouTube support** — the input box now accepts YouTube video, Shorts and Live links (`youtube.com`, `youtu.be`, `music.youtube.com`) alongside Twitch Clip links, mixed freely in the same paste. Playlists, channel pages and the bare homepage are rejected, same as any other unsupported link.
- Each queue row now shows a small brand-tinted icon chip — purple for Twitch, red for YouTube — so a mixed queue stays easy to scan.
- Renamed the internal `twitch` module to `link` and generalized its URL validation, display-name and download-status naming (`clip_display_name` → `media_display_name`, `download_clip` → `download_media`) to reflect that it now handles two platforms.
- The download folder moved from `~/Downloads/Twitch Clips/` to `~/Downloads/Clipper/` to match the broader scope; user-facing wording throughout ("items" instead of "clips") now reads naturally for either platform.
- Added a light Material-You-flavored accent layer on top of the native Adwaita stylesheet (`assets/style.css`): rounded, elevated input and button surfaces, an accent-colored focus ring on the input card, tonal status chips, a subtle accent wash across the window, and a small app-icon mark in the headerbar — all built from libadwaita's own theme colors, so light/dark mode and the user's accent color are unaffected.
- Added unit test coverage for YouTube URL validation and display-name extraction alongside the existing Twitch coverage.

## 1.2.0

- Redesigned the queue as a GNOME "Files"-style boxed list: each clip is an `AdwActionRow` with a leading icon, live status text (colored via the `accent`/`success`/`error` style classes), and its saved path or failure reason available as a tooltip.
- Added a proper empty state (`AdwStatusPage`) for when nothing is queued yet, matching how Nautilus shows an empty folder.
- Replaced the static status/stats labels with the headerbar's window subtitle ("3 clips queued", "Downloading 2/5…") and a floating `AdwToast` on completion, with an **Open Folder** action that opens the download directory.
- Replaced the plain red error text with an inline `AdwBanner`-style hint that only appears when there's something to say.
- **Downloads now run concurrently by default** (up to `MAX_CONCURRENT_DOWNLOADS = 3` at once) instead of one at a time; the overall progress bar now reflects the combined progress of every in-flight download.
- Cancelling now stops both running and not-yet-started jobs and reports how many completed vs. failed before the cancel.
- Rewrote the `yt-dlp` process handling to drain stdout/stderr on their own tasks and `select!` on process exit vs. cancellation, replacing the previous 20ms polling loop with an event-driven wait.
- The download button doubles as Cancel (turns into a destructive-styled button) while a run is active.

## 1.1.0

- Redesigned the UI to a minimal GTK/libadwaita style: removed the in-body "Clipper" title, the subtitle, the "Queue input" frame label, the "Links are added automatically" hint, the "Downloads are saved automatically…" footer note, and the standalone Clear button.
- The window title in the headerbar ("Clipper") is the only static label left; the text box's tooltip carries the input instructions instead of a visible hint.
- The queue count, status line and transfer stats now only appear when there's something to show, instead of sitting on screen as empty placeholders.
- Content is laid out in a centered `AdwClamp` for a tidier, more app-like width instead of stretching edge to edge.
- Clearing the queue is done by clearing the text box itself; there is no longer a separate Clear action.

## 1.0.2

- Renamed the application and Arch package to `clipper`.
- Removed unused UI state and dependency-warning dead code.
- Improved missing `yt-dlp` diagnostics.
- Updated packaging, desktop entry, README and release metadata.

## 1.0.1

- Initial downloader-only release.
