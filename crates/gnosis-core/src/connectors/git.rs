use crate::connectors::types::GitProtoData;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Local Git enrichment via the `git` CLI when available.
/// Falls back gracefully when not in a repository or git is missing.
#[derive(Clone, Debug, Default)]
pub struct GitContext {
    pub repository_root: Option<PathBuf>,
    pub branch: Option<String>,
}

impl GitContext {
    pub fn detect(root: &Path) -> Self {
        let repository_root = git_output(root, &["rev-parse", "--show-toplevel"])
            .ok()
            .map(|s| PathBuf::from(s.trim()));
        let branch = repository_root.as_ref().and_then(|r| {
            git_output(r, &["rev-parse", "--abbrev-ref", "HEAD"])
                .ok()
                .map(|s| s.trim().to_string())
        });
        Self {
            repository_root,
            branch,
        }
    }

    pub fn enrich(&self, path: &Path) -> Option<GitProtoData> {
        let repo = self.repository_root.as_ref()?;
        let rel = path.strip_prefix(repo).unwrap_or(path);
        let rel_str = rel.to_string_lossy();

        let tracked = git_output(repo, &["ls-files", "--error-unmatch", &rel_str])
            .map(|_| true)
            .ok()
            .or(Some(false));

        let log = git_output(
            repo,
            &[
                "log",
                "-1",
                "--format=%H%x00%an%x00%aI%x00%s",
                "--",
                &rel_str,
            ],
        )
        .ok();

        let (last_commit_id, last_commit_author, last_commit_time, last_commit_summary) =
            if let Some(line) = log {
                let parts: Vec<&str> = line.trim().split('\0').collect();
                (
                    parts.first().map(|s| s.to_string()),
                    parts.get(1).map(|s| s.to_string()),
                    parts.get(2).map(|s| s.to_string()),
                    parts.get(3).map(|s| s.to_string()),
                )
            } else {
                (None, None, None, None)
            };

        Some(GitProtoData {
            repository_root: repo.clone(),
            branch: self.branch.clone(),
            tracked,
            last_commit_id,
            last_commit_author,
            last_commit_time,
            last_commit_summary,
        })
    }
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
