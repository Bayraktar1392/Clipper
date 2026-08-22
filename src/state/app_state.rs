#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct DownloadStats {
    pub percent: f64,
    pub speed: String,
    pub eta: String,
    pub downloaded: String,
    pub total: String,
    pub filename: String,
}

#[derive(Debug)]
pub enum WorkerMsg {
    QueueStarted { total: usize },
    JobStarted { index: usize, total: usize },
    Progress { index: usize, stats: DownloadStats },
    JobDone { index: usize, path: String },
    JobError { index: usize, error: String },
    QueueDone { completed: usize, failed: usize },
    Cancelled { completed: usize, failed: usize },
    Fatal { error: String },
}
