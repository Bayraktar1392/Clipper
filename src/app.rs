use crate::{
    error::AppError,
    state::{DownloadStats, QueueEntry, WorkerMsg},
    twitch::{
        downloader,
        url::{clip_display_name, validate_clip_url},
    },
    ui::{components, window},
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

/// How many clips download at once by default. High enough to make good
/// use of typical connections, low enough to stay polite to Twitch's CDN
/// and to keep per-clip progress readable.
const MAX_CONCURRENT_DOWNLOADS: usize = 3;

pub fn build_ui(app: &adw::Application) {
    if let Some(existing) = app.active_window() {
        existing.present();
        return;
    }

    let ui = Rc::new(window::build(app));
    let queue: Rc<RefCell<Vec<QueueEntry>>> = Rc::new(RefCell::new(Vec::new()));
    let rows: Rc<RefCell<Vec<window::QueueRow>>> = Rc::new(RefCell::new(Vec::new()));
    let progress_state: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));
    let running = Rc::new(Cell::new(false));
    let active_total = Rc::new(Cell::new(0usize));
    let done_count = Rc::new(Cell::new(0usize));
    let cancel_tx: Rc<RefCell<Option<watch::Sender<bool>>>> = Rc::new(RefCell::new(None));
    let (tx, rx) = mpsc::channel::<WorkerMsg>();

    // Automatically rebuild the queue whenever the user pastes or edits the
    // input. There is no separate "add" or "clear" action: the text box is
    // the queue, and the empty-state page takes over when it's empty.
    {
        let ui = ui.clone();
        let queue = queue.clone();
        let rows = rows.clone();
        let progress_state = progress_state.clone();
        let running = running.clone();
        let buffer = ui.url_view.buffer();
        buffer.connect_changed(move |buffer| {
            if running.get() {
                return;
            }

            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
            ui.placeholder.set_visible(text.trim().is_empty());

            let mut next = Vec::new();
            let mut invalid = 0usize;

            for token in text.split_whitespace() {
                let normalized = token.trim().trim_matches(|c: char| {
                    matches!(
                        c,
                        '"' | '\'' | '<' | '>' | ',' | ';' | '(' | ')' | '[' | ']'
                    )
                });
                if normalized.is_empty() {
                    continue;
                }

                match validate_clip_url(normalized) {
                    Ok(url) => {
                        let canonical = url.to_string();
                        if !next.iter().any(|entry: &QueueEntry| entry.url == canonical) {
                            next.push(QueueEntry { url: canonical });
                        }
                    }
                    Err(_) => invalid += 1,
                }
            }

            *queue.borrow_mut() = next.clone();
            *progress_state.borrow_mut() = vec![0.0; next.len()];

            for row in rows.borrow_mut().drain(..) {
                ui.queue.remove(&row.row);
            }
            for entry in &next {
                let display = clip_display_name(&entry.url);
                let row = window::queue_row(&display, &entry.url);
                ui.queue.append(&row.row);
                rows.borrow_mut().push(row);
            }

            let count = next.len();
            ui.view_stack
                .set_visible_child_name(if count > 0 { "list" } else { "empty" });
            ui.window_title.set_subtitle(&queued_subtitle(count));

            if invalid > 0 && count == 0 {
                components::set_hint(&ui.hint, Some("No valid Twitch Clip URLs found."));
            } else if invalid > 0 {
                components::set_hint(
                    &ui.hint,
                    Some(&format!("{invalid} invalid link(s) ignored.")),
                );
            } else {
                components::set_hint(&ui.hint, None);
            }
        });
    }

    // UI receives worker messages without blocking GTK's main thread.
    {
        let ui = ui.clone();
        let rows = rows.clone();
        let running = running.clone();
        let cancel_tx = cancel_tx.clone();
        let progress_state = progress_state.clone();
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
                            row.set_state(window::RowState::Active, "0%");
                        }
                    }
                    WorkerMsg::Progress { index, stats } => {
                        if let Some(row) = rows.borrow().get(index) {
                            row.set_state(
                                window::RowState::Active,
                                &format!("{:.0}%", stats.percent),
                            );
                            row.set_transfer(&format!(
                                "{} / {}  ·  {}  ·  ETA {}",
                                stats.downloaded, stats.total, stats.speed, stats.eta
                            ));
                        }
                        if let Some(slot) = progress_state.borrow_mut().get_mut(index) {
                            *slot = stats.percent;
                        }
                        update_overall_progress(&ui, &progress_state);
                    }
                    WorkerMsg::JobDone { index, path } => {
                        if let Some(row) = rows.borrow().get(index) {
                            row.set_state(window::RowState::Done, "Done");
                            row.reset_subtitle();
                            row.set_tooltip(&format!("Saved to {path}"));
                        }
                        if let Some(slot) = progress_state.borrow_mut().get_mut(index) {
                            *slot = 100.0;
                        }
                        update_overall_progress(&ui, &progress_state);
                        done_count.set(done_count.get() + 1);
                        ui.window_title.set_subtitle(&format!(
                            "Downloading {}/{}",
                            done_count.get(),
                            active_total.get()
                        ));
                    }
                    WorkerMsg::JobError { index, error } => {
                        if let Some(row) = rows.borrow().get(index) {
                            row.set_state(window::RowState::Error, "Error");
                            row.reset_subtitle();
                            row.set_error_detail(&error);
                        }
                        if let Some(slot) = progress_state.borrow_mut().get_mut(index) {
                            *slot = 100.0;
                        }
                        update_overall_progress(&ui, &progress_state);
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
                                "Downloaded {completed} clip{}",
                                if completed == 1 { "" } else { "s" }
                            )
                        } else {
                            format!("Finished · {completed} downloaded, {failed} failed")
                        };
                        ui.toast_overlay.add_toast(adw::Toast::new(&summary));
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
                        components::set_hint(&ui.hint, Some(&error));
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
                ui.toast_overlay
                    .add_toast(adw::Toast::new("Paste one or more Twitch Clip URLs first"));
                return;
            }

            for row in rows.borrow().iter() {
                row.set_state(window::RowState::Waiting, "Waiting");
                row.reset_subtitle();
            }
            *progress_state.borrow_mut() = vec![0.0; entries.len()];
            components::set_hint(&ui.hint, None);

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
/// once several clips can be downloading at the same time.
fn update_overall_progress(ui: &window::Ui, progress_state: &Rc<RefCell<Vec<f64>>>) {
    let state = progress_state.borrow();
    if state.is_empty() {
        return;
    }
    let overall = state.iter().sum::<f64>() / (state.len() as f64 * 100.0);
    let overall = overall.clamp(0.0, 1.0);
    ui.progress.set_fraction(overall);
    ui.progress
        .set_text(Some(&format!("{:.0}%", overall * 100.0)));
}

fn queued_subtitle(count: usize) -> String {
    if count == 0 {
        String::new()
    } else {
        format!("{count} clip{} queued", if count == 1 { "" } else { "s" })
    }
}

/// Downloads every queued clip, running up to [`MAX_CONCURRENT_DOWNLOADS`]
/// at once. Each job reports its own progress by index, so the UI can show
/// several clips downloading side by side.
async fn run_queue(
    entries: Vec<QueueEntry>,
    tx: &mpsc::Sender<WorkerMsg>,
    cancel_rx: watch::Receiver<bool>,
) -> crate::error::AppResult<()> {
    let total = entries.len();
    let _ = tx.send(WorkerMsg::QueueStarted { total });
    let download_dir = download_directory()?;
    tokio::fs::create_dir_all(&download_dir).await?;

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

            let _ = tx.send(WorkerMsg::JobStarted { index, total });

            let (stats_tx, mut stats_rx) = tokio::sync::mpsc::unbounded_channel::<DownloadStats>();
            let stats_forward = tx.clone();
            let stats_task = tokio::spawn(async move {
                while let Some(stats) = stats_rx.recv().await {
                    let _ = stats_forward.send(WorkerMsg::Progress { index, stats });
                }
            });

            let result =
                downloader::download_clip(&entry.url, &download_dir, &stats_tx, cancel_rx).await;
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

fn download_directory() -> crate::error::AppResult<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::CommandFailed("filesystem".into(), "HOME is not set".into()))?;
    Ok(home.join("Downloads").join("Twitch Clips"))
}
