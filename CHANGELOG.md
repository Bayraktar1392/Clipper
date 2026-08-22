# Changelog

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
