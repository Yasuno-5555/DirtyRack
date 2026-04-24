use arc_swap::ArcSwap;
use dirtydata_core::ir::Graph;
use dirtydata_core::types::{StableId, PortRef, PatchSource, TrustLevel, IntentId};
use dirtydata_core::patch::Patch;
use dirtydata_core::storage::Storage;
use dirtydata_core::actions::{self, UserPatchFile};
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use crossbeam_channel::{unbounded, Receiver};

/// Cosmetic State — 表示に関する設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiLayout {
    pub pan: [f32; 2],
    pub zoom: f32,
    pub nodes: HashMap<StableId, NodeVisuals>,
    pub intent_zones: HashMap<IntentId, [f32; 4]>,
}

impl UiLayout {
    pub fn load(cwd: &std::path::Path) -> Self {
        let path = cwd.join(".dirtydata").join("ui_layout.json");
        if path.exists() {
            let json = std::fs::read_to_string(path).unwrap_or_default();
            serde_json::from_str(&json).unwrap_or_else(|_| Self::default())
        } else {
            Self::default()
        }
    }

    pub fn save(&self, cwd: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let path = cwd.join(".dirtydata").join("ui_layout.json");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeVisuals {
    pub position: [f32; 2],
    pub is_collapsed: bool,
}

#[derive(Debug, Default)]
pub struct InteractionState {
    pub dragging_node: Option<(StableId, egui::Vec2)>,
    pub dragging_cable: Option<PortRef>,
    pub pending_edges: HashSet<(PortRef, PortRef)>,
}

impl Default for UiLayout {
    fn default() -> Self {
        Self {
            pan: [0.0, 0.0],
            zoom: 1.0,
            nodes: HashMap::new(),
            intent_zones: HashMap::new(),
        }
    }
}

pub mod app;

pub fn run_gui() -> eframe::Result<()> {
    let cwd = std::env::current_dir().expect("failed to get current dir");
    let ir_path = cwd.join(".dirtydata").join("ir").join("current.json");

    let initial_graph = if ir_path.exists() {
        let json = std::fs::read_to_string(&ir_path).expect("failed to read IR");
        serde_json::from_str(&json).unwrap_or_default()
    } else {
        Graph::default()
    };

    let shadow_graph = Arc::new(ArcSwap::from_pointee(initial_graph));
    let shadow_graph_clone = shadow_graph.clone();

    let layout = UiLayout::load(&cwd);

    // Action Queue
    let (action_tx, action_rx) = unbounded();
    let action_cwd = cwd.clone();
    
    // Spawn Action Worker Thread
    std::thread::spawn(move || {
        action_worker_loop(action_cwd, action_rx);
    });

    // Spawn watcher for real-time shadow graph sync
    let ir_path_clone = ir_path.clone();
    // We need to keep the watcher alive. Leaking for prototype or return it.
    let mut _watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(_event) = res {
            if let Ok(json) = std::fs::read_to_string(&ir_path_clone) {
                if let Ok(graph) = serde_json::from_str::<Graph>(&json) {
                    shadow_graph_clone.store(Arc::new(graph));
                }
            }
        }
    }).unwrap();

    if let Some(parent) = ir_path.parent() {
        let _ = _watcher.watch(parent, RecursiveMode::NonRecursive);
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("DirtyData - The Silent Projector"),
        ..Default::default()
    };

    eframe::run_native(
        "dirtydata_gui",
        native_options,
        Box::new(|_cc| {
            // Leak watcher to keep it running
            Box::leak(Box::new(_watcher));
            Ok(Box::new(app::DirtyDataApp::new(shadow_graph, action_tx, layout)))
        }),
    )
}

fn action_worker_loop(cwd: std::path::PathBuf, rx: Receiver<actions::UserAction>) {
    while let Ok(action) = rx.recv() {
        if let Err(e) = apply_action_to_storage(&cwd, action) {
            eprintln!("Failed to apply action: {}", e);
        }
    }
}

fn apply_action_to_storage(cwd: &std::path::Path, action: actions::UserAction) -> Result<(), Box<dyn std::error::Error>> {
    let storage = Storage::open(cwd)?;
    let mut graph = storage.load_graph()?;

    // Compile action → operations
    let ops = actions::compile_actions(&[action], &graph)?;

    // Create and apply patch
    let current_branch = storage.read_head()?;
    let parent_patch = storage.read_branch(&current_branch)?;
    
    let mut patch = Patch::from_operations_with_provenance(ops, PatchSource::UserDirect, TrustLevel::Trusted);
    if let Some(p_id) = parent_patch {
        patch = patch.with_parents(vec![p_id]);
    }
    
    graph.apply(&patch)?;

    // Atomic Save
    storage.save_patch(&patch)?;
    storage.save_graph(&graph)?;
    
    Ok(())
}


