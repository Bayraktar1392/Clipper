use crate::link::url::Source;

#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub url: String,
    pub source: Source,
}

#[derive(Debug, Clone)]
pub struct DownloadStats {
    pub percent: f64,
    /// Numeric bytes already written, for aggregate speed/ETA math.
    pub downloaded_bytes: u64,
    /// Numeric total bytes when known (0 when unknown).
    pub total_bytes: u64,
    /// Current transfer rate in bytes/second (0 when unknown).
    pub speed_bps: u64,
}

#[derive(Debug)]
pub enum WorkerMsg {
    QueueStarted { total: usize },
    JobStarted { index: usize },
    Progress { index: usize, stats: DownloadStats },
    JobDone { index: usize, path: String },
    JobError { index: usize, error: String },
    QueueDone { completed: usize, failed: usize },
    Cancelled { completed: usize, failed: usize },
    Fatal { error: String },
}
