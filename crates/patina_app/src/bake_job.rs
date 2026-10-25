//! Runs mesh-map bakes on a worker thread so the editor stays responsive.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use patina_bake::{BakeInput, BakeSettings, Baker};
use patina_core::{Image, MeshMap};
use patina_mesh::Mesh;
use patina_render::wgpu;

pub enum BakeMessage {
    /// Maps finished for one texture set.
    Baked {
        set: usize,
        maps: Vec<(MeshMap, Image)>,
    },
    Failed(String),
    Finished {
        cancelled: bool,
    },
}

/// What to bake, captured when the user presses Bake.
pub struct BakeRequest {
    pub low: Arc<Mesh>,
    pub high_path: Option<PathBuf>,
    pub cage_path: Option<PathBuf>,
    /// (texture set index, mesh part index, display name)
    pub sets: Vec<(usize, usize, String)>,
    pub settings: BakeSettings,
}

pub struct BakeJob {
    /// Overall progress (0–1) and a description of the current step.
    pub progress: Arc<Mutex<(f32, String)>>,
    pub cancel: Arc<AtomicBool>,
    pub rx: Receiver<BakeMessage>,
}

impl BakeJob {
    pub fn start(device: wgpu::Device, queue: wgpu::Queue, request: BakeRequest) -> Self {
        let progress = Arc::new(Mutex::new((0.0, "Preparing…".to_owned())));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let (p, c) = (progress.clone(), cancel.clone());
        std::thread::Builder::new()
            .name("patina-bake".into())
            .spawn(move || {
                let set_progress = |v: f32, text: &str| {
                    if let Ok(mut g) = p.lock() {
                        *g = (v, text.to_owned());
                    }
                };
                let load = |path: &Option<PathBuf>, what: &str| -> Result<Option<Mesh>, String> {
                    match path {
                        Some(path) => {
                            set_progress(0.0, &format!("Loading {what} {}…", path.display()));
                            patina_mesh::load(path)
                                .map(Some)
                                .map_err(|e| format!("{what}: {e}"))
                        }
                        None => Ok(None),
                    }
                };
                let (high, cage) = match (
                    load(&request.high_path, "high poly"),
                    load(&request.cage_path, "cage"),
                ) {
                    (Ok(h), Ok(c)) => (h, c),
                    (Err(e), _) | (_, Err(e)) => {
                        let _ = tx.send(BakeMessage::Failed(e));
                        let _ = tx.send(BakeMessage::Finished { cancelled: false });
                        return;
                    }
                };
                let baker = Baker::new(&device, &queue);
                let n = request.sets.len().max(1) as f32;
                let mut cancelled = false;
                for (i, (set, part, name)) in request.sets.iter().enumerate() {
                    let input = BakeInput {
                        low: &request.low,
                        high: high.as_ref(),
                        cage: cage.as_ref(),
                        part: *part,
                    };
                    let mut report =
                        |v: f32| set_progress((i as f32 + v) / n, &format!("Baking {name}"));
                    match baker.bake(&input, &request.settings, &mut report, &c) {
                        Ok(maps) => {
                            let _ = tx.send(BakeMessage::Baked { set: *set, maps });
                        }
                        Err(patina_bake::BakeError::Cancelled) => {
                            cancelled = true;
                            break;
                        }
                        Err(e) => {
                            let _ = tx.send(BakeMessage::Failed(format!("{name}: {e}")));
                        }
                    }
                }
                let _ = tx.send(BakeMessage::Finished { cancelled });
            })
            .expect("could not start the bake thread");
        Self {
            progress,
            cancel,
            rx,
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn progress(&self) -> (f32, String) {
        self.progress.lock().map(|g| g.clone()).unwrap_or_default()
    }
}
