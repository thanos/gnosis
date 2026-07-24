use crate::config::ScanConfig;
use crate::connectors::types::ObjectDescriptor;
use crate::error::{GnosisError, Result};
use crate::events::PipelineEvent;
use crossbeam_channel::Sender;
use ignore::WalkBuilder;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct FilesystemConnector {
    config: ScanConfig,
}

impl FilesystemConnector {
    pub fn new(config: ScanConfig) -> Self {
        Self { config }
    }

    pub fn discover(
        &self,
        events: &Sender<PipelineEvent>,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Vec<ObjectDescriptor>> {
        let root = self.config.root.canonicalize().map_err(|e| {
            GnosisError::Pipeline(format!(
                "cannot resolve root {}: {e}",
                self.config.root.display()
            ))
        })?;

        let mut builder = WalkBuilder::new(&root);
        builder.hidden(false);
        builder.git_ignore(true);
        builder.git_global(true);
        builder.git_exclude(true);
        builder.follow_links(false);
        builder.standard_filters(true);

        let excluded: Vec<String> = self.config.excluded_paths.clone();
        let skip_name = self.config.skip_output_dir_name.clone();
        builder.filter_entry(move |entry| {
            let name = entry.file_name().to_string_lossy();
            if name == skip_name {
                return false;
            }
            !excluded.iter().any(|e| name == e.as_str())
        });

        let skip_name = self.config.skip_output_dir_name.clone();
        let mut objects = Vec::new();

        for result in builder.build() {
            if cancel.load(Ordering::Relaxed) {
                return Err(GnosisError::Cancelled);
            }

            let entry = match result {
                Ok(e) => e,
                Err(err) => {
                    let _ = events.send(PipelineEvent::Warning {
                        message: format!("walk error: {err}"),
                    });
                    continue;
                }
            };

            let path = entry.path();
            // Skip the configured output directory anywhere under root.
            if path
                .components()
                .any(|c| c.as_os_str().to_string_lossy() == skip_name)
            {
                continue;
            }

            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(err) => {
                    let _ = events.send(PipelineEvent::Warning {
                        message: format!("stat {}: {err}", path.display()),
                    });
                    continue;
                }
            };

            let is_dir = meta.is_dir();
            if is_dir {
                continue; // catalog files; directories appear via parent paths in ProtoData
            }

            let size = meta.len();
            let modified = meta.modified().ok();
            let desc = ObjectDescriptor::from_path(&root, path, false, size, modified);

            let _ = events.send(PipelineEvent::ObjectDiscovered {
                id: desc.id.clone(),
                path: desc.relative_path.clone(),
            });
            objects.push(desc);
        }

        Ok(objects)
    }
}

pub fn read_object_bytes(path: &Path, max_size: u64) -> Result<Vec<u8>> {
    let meta = fs::metadata(path)?;
    if meta.len() > max_size {
        // Read only the bounded prefix.
        let mut file = fs::File::open(path)?;
        let mut buf = vec![0u8; max_size as usize];
        use std::io::Read;
        let n = file.read(&mut buf)?;
        buf.truncate(n);
        return Ok(buf);
    }
    Ok(fs::read(path)?)
}

pub fn collect_neighbors(path: &Path, limit: usize) -> Vec<String> {
    let parent = match path.parent() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let mut names = Vec::new();
    if let Ok(rd) = fs::read_dir(parent) {
        for entry in rd.flatten() {
            if names.len() >= limit {
                break;
            }
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    names
}

pub fn fingerprint_bytes(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

pub fn permissions_string(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(format!("{:o}", meta.permissions().mode() & 0o777))
    }
    #[cfg(not(unix))]
    {
        Some(if meta.permissions().readonly() {
            "readonly".into()
        } else {
            "readwrite".into()
        })
    }
}

#[allow(dead_code)]
pub fn ensure_within_root(root: &Path, path: &Path) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if path.starts_with(&root) {
        Ok(path)
    } else {
        Err(GnosisError::Pipeline(format!(
            "path {} escapes root {}",
            path.display(),
            root.display()
        )))
    }
}
