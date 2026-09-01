use crate::{
    config,
    error::AppError,
    link::{
        downloader,
        url::{media_display_name, validate_media_url},
    },
    state::{DownloadStats, QueueEntry, WorkerMsg},
    ui::window,
};
use adw::prelude::*;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};
use tokio::{
    runtime::Runtime,
    sync::{Semaphore, watch},
    task::JoinSet,
};

/// How many items download at once by default. High enough to make good
/// use of typical connections, low enough to stay polite to source CDNs
/// and to keep per-item progress readable.
const MAX_CONCURRENT_DOWNLOADS: usize = 3;

pub fn build_ui(app: &adw::Application) {
    if let Some(existing) = app.active_window() {
        existing.present();
        return;
    }

    // "Open Folder" action backing the completion toast button.
    let open_folder = gtk::gio::SimpleAction::new("open-folder", None);
    open_folder.connect_activate(|_, _| config::open_download_directory());
    app.add_action(&open_folder);

    let ui = Rc::new(window::build(app));
    let queue: Rc<RefCell<Vec<QueueEntry>>> = Rc::new(RefCell::new(Vec::new()));
    let rows: Rc<RefCell<Vec<window::QueueRow>>> = Rc::new(RefCell::new(Vec::new()));
    let progress_state: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));
    let stats_state: Rc<RefCell<Vec<Option<crate::state::DownloadStats>>>> =
        Rc::new(RefCell::new(Vec::new()));
    let running = Rc::new(Cell::new(false));
    let active_total = Rc::new(Cell::new(0usize));
    let done_count = Rc::new(Cell::new(0usize));
    let cancel_tx: Rc<RefCell<Option<watch::Sender<bool>>>> = Rc::new(RefCell::new(None));
    let (tx, rx) = mpsc::channel::<WorkerMsg>();

    // Reflect the current download directory in the "save to" row up front.
    let current_dir = config::download_directory()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| String::from("Choose a folder…"));
    ui.folder_label.set_text(&current_dir);
    ui.window_title.set_tooltip_text(Some(&current_dir));

    // Automatically rebuild the queue whenever the user pastes or edits the
    // input. There is no separate "add" or "clear" action: the text box is
    // the queue, and the empty-state page takes over when it's empty.
    {
        let ui = ui.clone();
        let queue = queue.clone();
        let rows = rows.clone();
        let progress_state = progress_state.clone();
        let stats_state = stats_state.clone();
        let running = running.clone();
        let prev_urls: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let save_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));

        let buffer = ui.url_view.buffer();
        buffer.connect_changed(move |buffer| {
            if running.get() {
                return;
            }

            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);

            // Debounced persist: coalesce rapid edits into a single write
            // ~250ms after the last change. A generation counter keeps only
            // the final write live — it never removes a previously scheduled
            // source (that panics once the one-shot has already fired).
            let save_text = text.clone();
            let generation = save_gen.get() + 1;
            save_gen.set(generation);
            let save_gen = save_gen.clone();
            gtk::glib::timeout_add_local_once(Duration::from_millis(250), move || {
                if save_gen.get() == generation {
                    config::save_queue(&save_text);
                }
            });

            let (next, invalid) = parse_queue(&text);
            let urls: Vec<String> = next.iter().map(|entry| entry.url.clone()).collect();
            let unchanged = *prev_urls.borrow() == urls;
            let count = urls.len();

            *progress_state.borrow_mut() = vec![0.0; count];
            *stats_state.borrow_mut() = vec![None; count];
            *queue.borrow_mut() = next;

            // Only rebuild the widget tree when the set of recognized links
            // actually changed — typing a separator or fixing a rejected
            // token shouldn't churn (and re-animate) every row.
            if !unchanged {
                for row in rows.borrow_mut().drain(..) {
                    ui.queue.remove(&row.row);
                }
                for entry in queue.borrow().iter() {
                    let display = media_display_name(&entry.url, entry.source);
                    let row = window::queue_row(&display, &entry.url, entry.source);
                    let row_url = entry.url.clone();
                    let row_buffer = ui.url_view.buffer().clone();
                    row.remove
                        .connect_clicked(move |_| remove_url_from_buffer(&row_buffer, &row_url));
                    ui.queue.append(&row.row);
                    rows.borrow_mut().push(row);
                }

                ui.view_stack
                    .set_visible_child_name(if count > 0 { "list" } else { "empty" });
                ui.window_title.set_subtitle(&queued_subtitle(count));
            }
            *prev_urls.borrow_mut() = urls;

            if invalid > 0 && count == 0 {
                set_hint(&ui.hint, Some("No valid Twitch or YouTube links found."));
            } else if invalid > 0 {
                set_hint(
                    &ui.hint,
                    Some(&format!("{invalid} invalid link(s) ignored.")),
                );
            } else {
                set_hint(&ui.hint, None);
            }
        });
    }

    // Restore the previously persisted queue so a restart picks up where
    // the user left off. Setting the text fires the change handler above,
    // which rebuilds the rows and schedules a debounced re-save.
    let restored = config::load_queue();
    if !restored.trim().is_empty() {
        ui.url_view.buffer().set_text(&restored);
    }

    // "Save to folder" row — opens a folder chooser and persists the new
    // download directory, updating the label and tooltip right away.
    {
        let ui = ui.clone();
        let browse = ui.folder_button.clone();
        browse.connect_clicked(move |_| {
            let current = config::download_directory().ok();
            let parent = ui.window_title.root().and_downcast::<gtk::Window>();
            let dialog = gtk::FileDialog::builder()
                .title("Choose a download folder")
                .build();
            if let Some(dir) = current {
                dialog.set_initial_folder(Some(&gtk::gio::File::for_path(&dir)));
            }
            let ui = ui.clone();
            let callback = move |result: Result<gtk::gio::File, gtk::glib::Error>| {
                if let Ok(file) = result
                    && let Some(path) = file.path()
                {
                    config::set_download_directory(&path);
                    let display = path.display().to_string();
                    ui.folder_label.set_text(&display);
                    ui.window_title.set_tooltip_text(Some(&display));
                }
            };
            dialog.select_folder(parent.as_ref(), None::<&gtk::gio::Cancellable>, callback);
        });
    }

    // Drag & drop: drop a link (or text containing links) anywhere on the
    // window to add it to the queue. The whole window surface is the drop
    // target, so it's forgiving about where exactly you drop.
    {
        let ui = ui.clone();
        let running = running.clone();
        if let Some(root) = ui.window_title.root().and_downcast::<gtk::Window>() {
            let drop_target = gtk::DropTargetAsync::new(
                Some(gtk::gdk::ContentFormats::new(&["text/plain"])),
                gtk::gdk::DragAction::COPY,
            );
            let ui_drop = ui.clone();
            drop_target.connect_drop(move |_target, drop, _x, _y| {
                if running.get() {
                    return false;
                }
                let ui = ui_drop.clone();
                drop.read_value_async(
                    gtk::gdk::glib::Type::STRING,
                    gtk::glib::Priority::DEFAULT,
                    None::<&gtk::gio::Cancellable>,
                    move |result| {
                        if let Ok(value) = result
                            && let Ok(text) = value.get::<String>()
                        {
                            ingest_text(&ui, &text);
                        }
                    },
                );
                true
            });
            root.add_controller(drop_target);
        }
    }

    // UI receives worker messages without blocking GTK's main thread.
    {
        let ui = ui.clone();
        let rows = rows.clone();
        let running = running.clone();
        let cancel_tx = cancel_tx.clone();
        let progress_state = progress_state.clone();
        let stats_state = stats_state.clone();
        let active_total = active_total.clone();
        let done_count = done_count.clone();

        gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
            let reset_after_run = || {
                running.set(false);
                ui.url_view.set_sensitive(true);
                ui.download_button.set_sensitive(true);
                ui.download_button.remove_css_class("destructive-action");
                ui.download_button.add_css_class("suggested-action");
                ui.download_label.set_text("Download");
                ui.process_stack.set_visible_child_name("icon");
                ui.spinner.stop();
                ui.progress.set_visible(false);
                ui.status.set_visible(false);
                ui.window_title
                    .set_subtitle(&queued_subtitle(active_total.get()));
            };

            while let Ok(message) = rx.try_recv() {
                match message {
                    WorkerMsg::QueueStarted { total } => {
                        running.set(true);
                        active_total.set(total);
                        done_count.set(0);
                        ui.url_view.set_sensitive(false);
                        ui.download_button.set_sensitive(true);
                        ui.download_button.remove_css_class("suggested-action");
                        ui.download_button.add_css_class("destructive-action");
                        ui.download_label.set_text("Cancel");
                        ui.progress.set_visible(true);
                        ui.progress.set_fraction(0.0);
                        ui.progress.set_text(Some("0%"));
                        ui.window_title
                            .set_subtitle(&format!("Downloading 0/{total}"));
                        ui.process_stack.set_visible_child_name("spinner");
                        ui.spinner.start();
                    }
                    WorkerMsg::JobStarted { index, .. } => {
                        if let Some(row) = rows.borrow().get(index) {
                            row.set_state(window::RowState::Active, "Downloading");
                        }
                    }
                    WorkerMsg::Progress { index, stats } => {
                        if let Some(row) = rows.borrow().get(index) {
                            row.set_state(window::RowState::Active, "Downloading");
                        }
                        if let Some(slot) = progress_state.borrow_mut().get_mut(index) {
                            *slot = stats.percent;
                        }
                        if let Some(slot) = stats_state.borrow_mut().get_mut(index) {
                            *slot = Some(stats.clone());
                        }
                        update_overall_progress(&ui, &progress_state, &stats_state);
                    }
                    WorkerMsg::JobDone { index, path } => {
                        mark_row_complete(
                            &rows,
                            &progress_state,
                            &stats_state,
                            index,
                            window::RowState::Done,
                            "Done",
                        );
                        if let Some(row) = rows.borrow().get(index) {
                            row.set_tooltip(&format!("Saved to {path} — click to reveal"));
                            row.set_reveal_target(PathBuf::from(&path));
                        }
                        update_overall_progress(&ui, &progress_state, &stats_state);
                        done_count.set(done_count.get() + 1);
                        ui.window_title.set_subtitle(&format!(
                            "Downloading {}/{}",
                            done_count.get(),
                            active_total.get()
                        ));
                    }
                    WorkerMsg::JobError { index, error } => {
                        mark_row_complete(
                            &rows,
                            &progress_state,
                            &stats_state,
                            index,
                            window::RowState::Error,
                            "Error",
                        );
                        if let Some(row) = rows.borrow().get(index) {
                            row.set_error_detail(&error);
                        }
                        update_overall_progress(&ui, &progress_state, &stats_state);
                        done_count.set(done_count.get() + 1);
                        ui.window_title.set_subtitle(&format!(
                            "Downloading {}/{}",
                            done_count.get(),
                            active_total.get()
                        ));
                    }
                    WorkerMsg::QueueDone { completed, failed } => {
                        reset_after_run();
                        let summary = if failed == 0 {
                            format!(
                                "Downloaded {completed} item{}",
                                if completed == 1 { "" } else { "s" }
                            )
                        } else {
                            format!("Finished · {completed} downloaded, {failed} failed")
                        };
                        let toast = adw::Toast::new(&summary);
                        toast.set_button_label(Some("Open Folder"));
                        toast.set_action_name(Some("app.open-folder"));
                        ui.toast_overlay.add_toast(toast);
                        if completed > 0 {
                            play_completion_sound();
                            notify(
                                &ui,
                                "Downloads complete",
                                &format!(
                                    "{completed} item{} saved to the downloads folder",
                                    if completed == 1 { "" } else { "s" }
                                ),
                            );
                        }
                        cancel_tx.borrow_mut().take();
                    }
                    WorkerMsg::Cancelled { completed, failed } => {
                        reset_after_run();
                        ui.toast_overlay.add_toast(adw::Toast::new(&format!(
                            "Cancelled · {completed} downloaded, {failed} failed"
                        )));
                        cancel_tx.borrow_mut().take();
                    }
                    WorkerMsg::Fatal { error } => {
                        reset_after_run();
                        set_hint(&ui.hint, Some(&error));
                        ui.toast_overlay
                            .add_toast(adw::Toast::new("Download failed — see details below"));
                        cancel_tx.borrow_mut().take();
                    }
                }
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    // One action only: Download, which becomes Cancel while the queue is
    // running. It processes the automatically built queue concurrently.
    {
        let ui = ui.clone();
        let queue = queue.clone();
        let rows = rows.clone();
        let progress_state = progress_state.clone();
        let stats_state = stats_state.clone();
        let running = running.clone();
        let cancel_tx = cancel_tx.clone();
        let worker_tx = tx.clone();

        let download_button = ui.download_button.clone();
        download_button.connect_clicked(move |_| {
            if running.get() {
                if let Some(sender) = cancel_tx.borrow().as_ref() {
                    let _ = sender.send(true);
                }
                ui.window_title.set_subtitle("Cancelling…");
                return;
            }

            let entries = queue.borrow().clone();
            if entries.is_empty() {
                ui.toast_overlay.add_toast(adw::Toast::new(
                    "Paste one or more Twitch or YouTube links first",
                ));
                return;
            }

            for row in rows.borrow().iter() {
                row.set_state(window::RowState::Waiting, "Waiting");
                row.reset_subtitle();
            }
            *progress_state.borrow_mut() = vec![0.0; entries.len()];
            *stats_state.borrow_mut() = vec![None; entries.len()];
            set_hint(&ui.hint, None);

            let (cancel_send, cancel_recv) = watch::channel(false);
            *cancel_tx.borrow_mut() = Some(cancel_send);
            let worker_tx = worker_tx.clone();

            thread::spawn(move || {
                let runtime = match Runtime::new() {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = worker_tx.send(WorkerMsg::Fatal {
                            error: error.to_string(),
                        });
                        return;
                    }
                };

                let result = runtime.block_on(run_queue(entries, &worker_tx, cancel_recv));
                if let Err(error) = result {
                    let _ = worker_tx.send(WorkerMsg::Fatal {
                        error: error.to_string(),
                    });
                }
            });
        });
    }
}

/// Recomputes and displays the overall progress bar from every row's
/// individual percentage — the only way an aggregate figure makes sense
/// once several items can be downloading at the same time. Also computes
/// total transfer rate and a combined ETA for the status line under it.
/// Marks a row as complete (done or error) and updates progress tracking.
fn mark_row_complete(
    rows: &Rc<RefCell<Vec<window::QueueRow>>>,
    progress_state: &Rc<RefCell<Vec<f64>>>,
    stats_state: &Rc<RefCell<Vec<Option<crate::state::DownloadStats>>>>,
    index: usize,
    state: window::RowState,
    status_text: &str,
) {
    if let Some(row) = rows.borrow().get(index) {
        row.set_state(state, status_text);
        row.reset_subtitle();
    }
    if let Some(slot) = progress_state.borrow_mut().get_mut(index) {
        *slot = 100.0;
    }
    if let Some(slot) = stats_state.borrow_mut().get_mut(index) {
        *slot = None;
    }
}

fn update_overall_progress(
    ui: &window::Ui,
    progress_state: &Rc<RefCell<Vec<f64>>>,
    stats_state: &Rc<RefCell<Vec<Option<crate::state::DownloadStats>>>>,
) {
    let state = progress_state.borrow();
    if state.is_empty() {
        return;
    }
    let overall = state.iter().sum::<f64>() / (state.len() as f64 * 100.0);
    let overall = overall.clamp(0.0, 1.0);
    ui.progress.set_fraction(overall);
    ui.progress
        .set_text(Some(&format!("{:.1}%", overall * 100.0)));

    // Aggregate the raw numbers for the status line: sum the active
    // transfer speeds and the remaining bytes, then derive a combined ETA.
    let stats = stats_state.borrow();
    let mut speed_bps = 0u64;
    let mut remaining_bytes = 0u64;
    let mut active = 0usize;
    for slot in stats.iter().flatten() {
        active += 1;
        speed_bps += slot.speed_bps;
        if slot.total_bytes > 0 {
            let done = slot.downloaded_bytes.min(slot.total_bytes);
            remaining_bytes = remaining_bytes.saturating_add(slot.total_bytes - done);
        }
    }

    if active > 0 && (overall < 1.0) {
        let mut parts = Vec::new();
        if speed_bps > 0 {
            parts.push(format!(
                "{} · {} left",
                downloader::format_bytes(speed_bps),
                downloader::format_eta(Some(remaining_bytes as f64 / speed_bps as f64))
            ));
        } else {
            parts.push("rate unknown".to_string());
        }
        ui.status.set_text(&format!(
            "{} file{} downloading — {}",
            active,
            if active == 1 { "" } else { "s" },
            parts.join("  ")
        ));
        ui.status.set_visible(true);
    } else {
        ui.status.set_visible(false);
    }
}

fn queued_subtitle(count: usize) -> String {
    if count == 0 {
        String::new()
    } else {
        format!("{count} item{} queued", if count == 1 { "" } else { "s" })
    }
}

/// Strips the surrounding punctuation each token is allowed to carry (e.g.
/// the `(clip)` from a pasted chat message) so bare URLs are extracted.
fn normalize_token(token: &str) -> &str {
    token.trim().trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '<' | '>' | ',' | ';' | '(' | ')' | '[' | ']'
        )
    })
}

/// Splits the raw input text into validated queue entries while counting
/// the tokens that failed validation, so the change handler can build the
/// queue and report the number of ignored links in a single pass.
fn parse_queue(text: &str) -> (Vec<QueueEntry>, usize) {
    let mut entries: Vec<QueueEntry> = Vec::new();
    let mut invalid = 0usize;
    for token in text.split_whitespace() {
        let normalized = normalize_token(token);
        if normalized.is_empty() {
            continue;
        }
        match validate_media_url(normalized) {
            Ok((url, source)) => {
                let canonical = url.to_string();
                if !entries.iter().any(|entry| entry.url == canonical) {
                    entries.push(QueueEntry {
                        url: canonical,
                        source,
                    });
                }
            }
            Err(_) => invalid += 1,
        }
    }
    (entries, invalid)
}

/// Shows or hides the small inline validation hint under the input box.
fn set_hint(label: &gtk::Label, text: Option<&str>) {
    match text {
        Some(message) => {
            label.set_text(message);
            label.set_visible(true);
        }
        None => label.set_visible(false),
    }
}

/// Removes one URL from the input buffer. This edits the text, which fires
/// the shared change handler and rebuilds the queue without the removed
/// item — a single source of truth for what's queued.
fn remove_url_from_buffer(buffer: &gtk::TextBuffer, target: &str) {
    let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
    let result: Vec<&str> = text
        .split_whitespace()
        .filter(|token| {
            let normalized = normalize_token(token);
            !normalized.is_empty() && normalized != target
        })
        .collect();
    buffer.set_text(&result.join("\n"));
}

/// Scans dropped text for Twitch/YouTube links that aren't already in the
/// input, and appends any it finds.
fn ingest_text(ui: &window::Ui, text: &str) {
    let buffer = ui.url_view.buffer();
    let existing = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
    let mut additions = Vec::new();
    for token in text.split_whitespace() {
        let normalized = normalize_token(token);
        if normalized.is_empty() {
            continue;
        }
        if validate_media_url(normalized).is_ok() && !existing.contains(normalized) {
            additions.push(normalized.to_string());
        }
    }
    if additions.is_empty() {
        return;
    }
    let mut end = buffer.end_iter();
    if !existing.trim().is_empty() {
        buffer.insert(&mut end, "\n");
    }
    buffer.insert(&mut end, &additions.join("\n"));
}

/// Fires a desktop notification. Best-effort: if there's no way to send
/// one (no application), it quietly does nothing.
fn notify(ui: &window::Ui, title: &str, body: &str) {
    let Some(application) = ui
        .window_title
        .root()
        .and_then(|root| root.downcast::<gtk::ApplicationWindow>().ok())
        .and_then(|window| window.application())
    else {
        return;
    };
    let notification = gtk::gio::Notification::new(title);
    notification.set_body(Some(body));
    let icon = gtk::gio::ThemedIcon::new("clipper");
    notification.set_icon(&icon);
    application.send_notification(Some("clipper-complete"), &notification);
}

/// Plays a light, calm completion chime. Best-effort — tries common system
/// players and quietly ignores failures.
fn play_completion_sound() {
    use std::process::Command;
    for player in ["canberra-gtk-play", "paplay", "pw-play", "aplay"] {
        let args: &[&str] = match player {
            "canberra-gtk-play" => &["-i", "complete"],
            "paplay" | "pw-play" => &["/usr/share/sounds/freedesktop/stereo/complete.oga"],
            _ => &[],
        };
        if Command::new(player).args(args).spawn().is_ok() {
            return;
        }
    }
}

/// Downloads every queued item, running up to [`MAX_CONCURRENT_DOWNLOADS`]
/// at once. Each job reports its own progress by index, so the UI can show
/// several downloads side by side.
async fn run_queue(
    entries: Vec<QueueEntry>,
    tx: &mpsc::Sender<WorkerMsg>,
    cancel_rx: watch::Receiver<bool>,
) -> crate::error::AppResult<()> {
    let total = entries.len();
    let _ = tx.send(WorkerMsg::QueueStarted { total });
    let download_dir = config::download_directory()?;

    let permits = MAX_CONCURRENT_DOWNLOADS.min(total).max(1);
    let semaphore = Arc::new(Semaphore::new(permits));
    let mut jobs: JoinSet<(usize, crate::error::AppResult<PathBuf>)> = JoinSet::new();

    for (index, entry) in entries.into_iter().enumerate() {
        let tx = tx.clone();
        let cancel_rx = cancel_rx.clone();
        let download_dir = download_dir.clone();
        let semaphore = Arc::clone(&semaphore);

        jobs.spawn(async move {
            let permit = match semaphore.acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => return (index, Err(AppError::Cancelled)),
            };

            if *cancel_rx.borrow() {
                return (index, Err(AppError::Cancelled));
            }

            let _ = tx.send(WorkerMsg::JobStarted { index });

            let (stats_tx, mut stats_rx) = tokio::sync::mpsc::unbounded_channel::<DownloadStats>();
            let stats_forward = tx.clone();
            let stats_task = tokio::spawn(async move {
                while let Some(stats) = stats_rx.recv().await {
                    let _ = stats_forward.send(WorkerMsg::Progress { index, stats });
                }
            });

            let result =
                downloader::download_media(&entry.url, &download_dir, &stats_tx, cancel_rx).await;
            drop(stats_tx);
            let _ = stats_task.await;

            drop(permit);
            (index, result)
        });
    }

    let mut completed = 0usize;
    let mut failed = 0usize;
    let mut any_cancelled = false;

    while let Some(outcome) = jobs.join_next().await {
        let (index, result) = match outcome {
            Ok(value) => value,
            Err(join_error) => {
                failed += 1;
                let _ = tx.send(WorkerMsg::JobError {
                    index: usize::MAX,
                    error: format!("Worker task failed: {join_error}"),
                });
                continue;
            }
        };

        match result {
            Ok(path) => {
                completed += 1;
                let _ = tx.send(WorkerMsg::JobDone {
                    index,
                    path: path.display().to_string(),
                });
            }
            Err(AppError::Cancelled) => any_cancelled = true,
            Err(error) => {
                failed += 1;
                let _ = tx.send(WorkerMsg::JobError {
                    index,
                    error: error.to_string(),
                });
            }
        }
    }

    if any_cancelled {
        let _ = tx.send(WorkerMsg::Cancelled { completed, failed });
    } else {
        let _ = tx.send(WorkerMsg::QueueDone { completed, failed });
    }

    Ok(())
}
