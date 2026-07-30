//! Human-readable list and detail views for jobs.

use crate::jobs::types::{Job, JobStatus, JobSummary};
use chrono::{DateTime, Utc};

/// One-line list formatting for a job.
pub fn format_job_list_line(job: &Job) -> String {
    let path_hint = job_path_hint(job);
    format!(
        "{:<10} {:<16} {:<10} {:<19} {:<8} {}",
        job.status.as_str(),
        truncate(&job.kind, 16),
        short_id(job.id.as_str()),
        format_timestamp(list_timestamp(job)),
        format_duration_cell(job),
        path_hint
    )
}

/// Multi-line detailed view of a single job.
pub fn format_job_detail(job: &Job) -> String {
    let mut lines = Vec::new();
    lines.push(format!("id:        {}", job.id));
    lines.push(format!("scan_id:   {}", job.scan_id));
    lines.push(format!("kind:      {}", job.kind));
    lines.push(format!("status:    {}", job.status));
    lines.push(format!("attempts:  {}", job.attempts));
    lines.push(format!(
        "worker:    {}",
        job.worker_id.as_deref().unwrap_or("-")
    ));
    lines.push(format!("created:   {}", job.created_at.to_rfc3339()));
    lines.push(format!("updated:   {}", job.updated_at.to_rfc3339()));
    lines.push(format!(
        "started:   {}",
        job.started_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| "-".into())
    ));
    lines.push(format!(
        "finished:  {}",
        job.finished_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| "-".into())
    ));
    lines.push(format!(
        "available: {}",
        job.available_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| "-".into())
    ));
    if let Some(dur) = job_duration(job) {
        lines.push(format!("duration:  {dur}"));
    }
    lines.push("args:".into());
    lines.push(indent_json(&job.args));
    match job.status {
        JobStatus::Completed => {
            lines.push("result:".into());
            if let Some(result) = &job.result {
                lines.push(indent_json(result));
            } else {
                lines.push("  (none)".into());
            }
        }
        JobStatus::Failed | JobStatus::Stopped => {
            lines.push(format!(
                "error:     {}",
                job.error.as_deref().unwrap_or("(none)")
            ));
        }
        JobStatus::Pending | JobStatus::Running | JobStatus::Paused => {
            if let Some(err) = &job.error {
                lines.push(format!("error:     {err}"));
            }
        }
    }
    lines.join("\n")
}

/// Header + rows for a list view, including optional summary counts.
pub fn format_job_list(
    jobs: &[Job],
    summary: Option<&JobSummary>,
    status_filter: Option<JobStatus>,
) -> String {
    format_job_list_filtered(jobs, summary, status_filter, None)
}

/// List view with an optional scan-id filter label.
pub fn format_job_list_filtered(
    jobs: &[Job],
    summary: Option<&JobSummary>,
    status_filter: Option<JobStatus>,
    scan_filter: Option<&str>,
) -> String {
    let mut out = String::new();
    if let Some(s) = summary {
        out.push_str(&format!(
            "jobs: {} total  pending={} running={} paused={} completed={} failed={} stopped={}\n",
            s.total(),
            s.pending,
            s.running,
            s.paused,
            s.completed,
            s.failed,
            s.stopped
        ));
    }
    if let Some(scan) = scan_filter {
        out.push_str(&format!("filter: scan_id={scan}\n"));
    }
    if let Some(st) = status_filter {
        out.push_str(&format!("filter: status={st}\n"));
    }
    if jobs.is_empty() {
        out.push_str("(no jobs)");
        return out;
    }
    out.push_str(&format!(
        "{:<10} {:<16} {:<10} {:<19} {:<8} PATH/HINT\n",
        "STATUS", "KIND", "ID", "TIMESTAMP", "TOOK"
    ));
    for job in jobs {
        out.push_str(&format_job_list_line(job));
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// Prefer finished → started → updated for the list timestamp column.
fn list_timestamp(job: &Job) -> DateTime<Utc> {
    job.finished_at.or(job.started_at).unwrap_or(job.updated_at)
}

fn format_timestamp(ts: DateTime<Utc>) -> String {
    ts.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn format_duration_cell(job: &Job) -> String {
    job_duration(job).unwrap_or_else(|| "-".into())
}

/// Elapsed time for terminal jobs (`finished - started`, else `finished - created`).
fn job_duration(job: &Job) -> Option<String> {
    if !matches!(
        job.status,
        JobStatus::Completed | JobStatus::Failed | JobStatus::Stopped
    ) {
        return None;
    }
    let end = job.finished_at.or(Some(job.updated_at))?;
    let start = job.started_at.unwrap_or(job.created_at);
    let ms = (end - start).num_milliseconds();
    if ms < 0 {
        return None;
    }
    Some(format_duration_ms(ms as u64))
}

fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        let secs = ms as f64 / 1000.0;
        if secs < 10.0 {
            format!("{secs:.1}s")
        } else {
            format!("{}s", ms / 1000)
        }
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        if secs == 0 {
            format!("{mins}m")
        } else {
            format!("{mins}m{secs}s")
        }
    }
}

fn job_path_hint(job: &Job) -> String {
    job.args
        .get("relative_path")
        .and_then(|v| v.as_str())
        .or_else(|| job.args.get("path").and_then(|v| v.as_str()))
        .map(|s| truncate(s, 48))
        .unwrap_or_else(|| "-".into())
}

fn short_id(id: &str) -> String {
    // Prefer last segment after ':' for job:uuid style ids.
    let bare = id.rsplit(':').next().unwrap_or(id);
    if bare.len() > 8 {
        bare[..8].to_string()
    } else {
        bare.to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn indent_json(value: &serde_json::Value) -> String {
    let pretty = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    pretty
        .lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::types::Job;
    use chrono::Duration;

    #[test]
    fn list_and_detail_render() {
        let mut job = Job::new(
            "scan",
            "analyze_object",
            serde_json::json!({"relative_path": "src/a.rs"}),
        );
        job.status = JobStatus::Completed;
        job.started_at = Some(job.created_at);
        job.finished_at = Some(job.created_at + Duration::milliseconds(1500));
        job.result = Some(serde_json::json!({"status": "understood"}));
        let list = format_job_list(std::slice::from_ref(&job), None, Some(JobStatus::Completed));
        assert!(list.contains("TIMESTAMP"));
        assert!(list.contains("TOOK"));
        assert!(list.contains("completed"));
        assert!(list.contains("1.5s"));
        assert!(list.contains("src/a.rs"));
        let detail = format_job_detail(&job);
        assert!(detail.contains("analyze_object"));
        assert!(detail.contains("understood"));
        assert!(detail.contains("duration:  1.5s"));
    }

    #[test]
    fn duration_formats() {
        assert_eq!(format_duration_ms(42), "42ms");
        assert_eq!(format_duration_ms(1500), "1.5s");
        assert_eq!(format_duration_ms(12_000), "12s");
        assert_eq!(format_duration_ms(125_000), "2m5s");
    }
}
