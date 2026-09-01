use crate::{
    error::{AppError, AppResult},
    state::DownloadStats,
};
use std::{
    collections::VecDeque,
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

/// Downloads a single clip or video with `yt-dlp`, streaming progress to `stats_tx`
/// and reacting immediately to cancellation. Safe to run several of these
/// concurrently: each call owns its own child process and pipes.
pub async fn download_media(
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
            "download:%(progress.percent)s|%(progress.speed)s|%(progress.eta)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s",
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
        // Keep the output clean and self-contained: never drop a `.info.json`
        // metadata file beside the video, and don't fetch separate subtitle
        // files. Videos that only carry burned-in on-screen text (common on
        // Twitch clips) can't be de-OCR'd, but any optional sidecar tracks are
        // skipped, so the result is the bare, subtitle-free media.
        .args(["--no-write-info-json", "--no-write-subs", "--no-embed-subs"])
        // Force a single common container so the result is one predictable
        // file (no loose `.webm`/`.m4a` leftovers from a video+audio merge).
        .args(["--remux-video", "mp4"])
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
    let mut tail: VecDeque<String> = VecDeque::with_capacity(STDERR_TAIL_LINES);
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(stats) = parse_progress_line(&line) {
            let _ = stats_tx.send(stats);
        } else if !line.trim().is_empty() {
            if tail.len() == STDERR_TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
    }
    tail.into_iter().collect()
}

fn parse_progress_line(line: &str) -> Option<DownloadStats> {
    let payload = line.strip_prefix("download:")?;
    let mut parts = payload.splitn(5, '|');
    let percent = parts.next()?.trim().parse::<f64>().ok()?.clamp(0.0, 100.0);
    let speed_raw = parts.next()?.trim();
    parts.next()?;
    let downloaded_bytes = parts.next()?.trim().parse::<u64>().ok()?;
    let total_bytes = parts.next()?.trim().parse::<u64>().unwrap_or(0);

    Some(DownloadStats {
        percent,
        speed_bps: parse_speed_bps(speed_raw),
        downloaded_bytes,
        total_bytes,
    })
}

/// Parses a yt-dlp speed token like `8.20MiB/s` into bytes/second.
fn parse_speed_bps(raw: &str) -> u64 {
    let raw = raw.trim();
    if raw.is_empty() {
        return 0;
    }
    let (num, unit) = raw
        .split_once('/')
        .and_then(|(num, _)| {
            let idx = num
                .char_indices()
                .find(|(_, c)| !c.is_ascii_digit() && *c != '.' && *c != '-')
                .map(|(i, _)| i);
            idx.map(|i| (&num[..i], &num[i..]))
        })
        .unwrap_or((raw, ""));
    let Ok(value) = num.trim().parse::<f64>() else {
        return 0;
    };
    let multiplier = match unit.trim().to_ascii_lowercase().as_str() {
        "b" => 1.0,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        "kb" => 1000.0,
        "mb" => 1_000_000.0,
        "gb" => 1_000_000_000.0,
        _ => 1.0,
    };
    (value * multiplier) as u64
}

/// Renders seconds as a compact `mm:ss` (or `h:mm:ss`) clock.
pub fn format_eta(secs: Option<f64>) -> String {
    let secs = match secs {
        Some(s) => s.round() as u64,
        None => return "—".into(),
    };
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// Renders a byte count as a human-friendly size.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes == 0 {
        return "0 B".into();
    }
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, format_eta, parse_progress_line, parse_speed_bps};

    #[test]
    fn parses_progress() {
        let stats = parse_progress_line("download:42.5|8.2MiB/s|12|12902793|30303887")
            .expect("progress should parse");
        assert!((stats.percent - 42.5).abs() < f64::EPSILON);
        assert_eq!(stats.downloaded_bytes, 12_902_793);
        assert_eq!(stats.total_bytes, 30_303_887);
        assert!((stats.speed_bps as f64 - 8.2 * 1024.0 * 1024.0).abs() < 1.0);
    }

    #[test]
    fn parses_speed_units() {
        assert_eq!(parse_speed_bps("1KiB/s"), 1024);
        assert_eq!(parse_speed_bps("1.5MiB/s"), (1.5 * 1024.0 * 1024.0) as u64);
        assert_eq!(
            parse_speed_bps("7.25GiB/s"),
            (7.25 * 1024.0 * 1024.0 * 1024.0) as u64
        );
        assert_eq!(parse_speed_bps(""), 0);
        assert_eq!(parse_speed_bps("garbage"), 0);
    }

    #[test]
    fn formats_sizes_and_eta() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(12_902_793), "12.3 MiB");
        assert_eq!(format_eta(Some(12.0)), "0:12");
        assert_eq!(format_eta(Some(3723.0)), "1:02:03");
        assert_eq!(format_eta(None), "—");
    }

    #[test]
    fn ignores_non_progress_lines() {
        assert!(parse_progress_line("[download] Destination: foo.mp4").is_none());
    }

    #[test]
    fn treats_missing_speed_and_eta_as_zero() {
        let stats = parse_progress_line("download:10||NA|1048576|10485760")
            .expect("progress should still parse with blank fields");
        assert_eq!(stats.speed_bps, 0);
        assert_eq!(stats.downloaded_bytes, 1_048_576);
        assert_eq!(stats.total_bytes, 10_485_760);
    }
}
