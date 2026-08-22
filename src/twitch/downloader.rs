use crate::{
    error::{AppError, AppResult},
    state::DownloadStats,
};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, ChildStderr, ChildStdout, Command},
    sync::{mpsc, watch},
};

/// How many trailing stderr lines we keep around to build an error message
/// from if yt-dlp exits non-zero. Just enough context without holding on
/// to an unbounded log for a process we don't otherwise care about.
const STDERR_TAIL_LINES: usize = 12;

/// Downloads a single clip with `yt-dlp`, streaming progress to `stats_tx`
/// and reacting immediately to cancellation. Safe to run several of these
/// concurrently: each call owns its own child process and pipes.
pub async fn download_clip(
    url: &str,
    download_dir: &Path,
    stats_tx: &mpsc::UnboundedSender<DownloadStats>,
    mut cancel_rx: watch::Receiver<bool>,
) -> AppResult<PathBuf> {
    tokio::fs::create_dir_all(download_dir).await?;

    let mut child = spawn_yt_dlp(url, download_dir)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::ProcessStart("yt-dlp".into(), "stdout unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::ProcessStart("yt-dlp".into(), "stderr unavailable".into()))?;

    // Drain both pipes concurrently on their own tasks so a full pipe
    // buffer can never stall the other stream or the child itself.
    let stdout_task = tokio::spawn(collect_final_path(stdout, download_dir.to_path_buf()));
    let stderr_task = tokio::spawn(collect_progress(stderr, stats_tx.clone()));

    let status = tokio::select! {
        status = child.wait() => status?,
        changed = cancel_rx.changed() => {
            if changed.is_ok() && *cancel_rx.borrow() {
                return Err(cancel(&mut child, stdout_task, stderr_task).await);
            }
            // The channel closed without ever asking us to cancel — keep
            // waiting for the process the normal way.
            child.wait().await?
        }
    };

    let final_path = stdout_task.await.unwrap_or(None);
    let stderr_tail = stderr_task.await.unwrap_or_default();

    if !status.success() {
        let detail = stderr_tail.join("\n");
        return Err(AppError::CommandFailed(
            "yt-dlp".into(),
            if detail.is_empty() {
                format!("exit status {status}")
            } else {
                detail
            },
        ));
    }

    final_path.filter(|path| path.exists()).ok_or_else(|| {
        AppError::CommandFailed(
            "yt-dlp".into(),
            "download completed but output file was not found".into(),
        )
    })
}

fn spawn_yt_dlp(url: &str, download_dir: &Path) -> AppResult<Child> {
    Command::new("yt-dlp")
        .args([
            "--no-playlist",
            "--newline",
            "--progress",
            "--progress-delta",
            "0.2",
            "--progress-template",
            "download:%(progress.percent)s|%(progress.speed)s|%(progress.eta)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.filename)s",
            "--retries",
            "10",
            "--fragment-retries",
            "10",
            "--concurrent-fragments",
            "8",
            "--format",
            "bestvideo*+bestaudio/best",
            "--paths",
        ])
        .arg(format!("home:{}", download_dir.display()))
        .args(["--output", "%(title).180s [%(id)s].%(ext)s", "--print", "after_move:filepath"])
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::MissingDependency("yt-dlp".into())
            } else {
                AppError::ProcessStart("yt-dlp".into(), e.to_string())
            }
        })
}

/// Kills the child and stops the reader tasks after a cancellation request.
/// The readers are aborted rather than awaited: their output no longer
/// matters, and waiting for a pipe that a killed process stopped writing
/// to would just delay the cancellation.
async fn cancel(
    child: &mut Child,
    stdout_task: tokio::task::JoinHandle<Option<PathBuf>>,
    stderr_task: tokio::task::JoinHandle<Vec<String>>,
) -> AppError {
    let _ = child.kill().await;
    stdout_task.abort();
    stderr_task.abort();
    AppError::Cancelled
}

/// Reads yt-dlp's stdout to completion and returns the last existing path
/// it printed (`--print after_move:filepath` emits the final file once
/// the download — and any needed remux — has finished).
async fn collect_final_path(stdout: ChildStdout, download_dir: PathBuf) -> Option<PathBuf> {
    let mut lines = BufReader::new(stdout).lines();
    let mut final_path = None;
    while let Ok(Some(line)) = lines.next_line().await {
        let candidate = PathBuf::from(line.trim());
        let candidate = if candidate.is_absolute() {
            candidate
        } else {
            download_dir.join(candidate)
        };
        if candidate.exists() {
            final_path = Some(candidate);
        }
    }
    final_path
}

/// Reads yt-dlp's stderr to completion, forwarding parsed progress lines
/// and keeping a short rolling tail of anything else for error reporting.
async fn collect_progress(
    stderr: ChildStderr,
    stats_tx: mpsc::UnboundedSender<DownloadStats>,
) -> Vec<String> {
    let mut lines = BufReader::new(stderr).lines();
    let mut tail: Vec<String> = Vec::with_capacity(STDERR_TAIL_LINES);
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(stats) = parse_progress_line(&line) {
            let _ = stats_tx.send(stats);
        } else if !line.trim().is_empty() {
            if tail.len() == STDERR_TAIL_LINES {
                tail.remove(0);
            }
            tail.push(line);
        }
    }
    tail
}

fn parse_progress_line(line: &str) -> Option<DownloadStats> {
    let payload = line.strip_prefix("download:")?;
    let mut parts = payload.splitn(6, '|');
    let percent = parts.next()?.trim().parse::<f64>().ok()?.clamp(0.0, 100.0);
    let speed = parts.next()?.trim().to_string();
    let eta = parts.next()?.trim().to_string();
    let downloaded = parts.next()?.trim().to_string();
    let total = parts.next()?.trim().to_string();
    let filename = parts.next().unwrap_or_default().trim().to_string();

    Some(DownloadStats {
        percent,
        speed: if speed.is_empty() {
            "—".into()
        } else {
            speed
        },
        eta: if eta.is_empty() || eta == "NA" {
            "—".into()
        } else {
            eta
        },
        downloaded,
        total,
        filename,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_progress_line;

    #[test]
    fn parses_progress() {
        let stats = parse_progress_line(
            "download:42.5|8.2MiB/s|00:12|12.3MiB|28.9MiB|Example Clip [abc].mp4",
        )
        .expect("progress should parse");
        assert!((stats.percent - 42.5).abs() < f64::EPSILON);
        assert_eq!(stats.speed, "8.2MiB/s");
        assert_eq!(stats.eta, "00:12");
        assert_eq!(stats.filename, "Example Clip [abc].mp4");
    }

    #[test]
    fn ignores_non_progress_lines() {
        assert!(parse_progress_line("[download] Destination: foo.mp4").is_none());
    }

    #[test]
    fn treats_missing_speed_and_eta_as_placeholders() {
        let stats = parse_progress_line("download:10|| NA |1MiB|10MiB|clip.mp4")
            .expect("progress should still parse with blank fields");
        assert_eq!(stats.speed, "—");
        assert_eq!(stats.eta, "—");
    }
}
