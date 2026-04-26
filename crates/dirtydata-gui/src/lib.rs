use arc_swap::ArcSwap;
use dirtydata_core::ir::Graph;
use dirtydata_core::types::{StableId, PortRef, PatchSource, TrustLevel, IntentId};
use dirtydata_core::patch::Patch;
use dirtydata_core::storage::Storage;
use dirtydata_core::actions::{self, UserPatchFile};
use dirtydata_runtime::nodes::MidiEvent;
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
    pub marquee: Option<egui::Rect>,
    pub pending_edges: HashSet<(PortRef, PortRef)>,
    pub quick_replace_target: Option<StableId>,
    pub quick_replace_input: String,
    pub graph_path: Vec<StableId>,
    pub sample_editor: Option<SampleEditorState>,
}

#[derive(Debug, Clone)]
pub struct SampleEditorState {
    pub target_node: StableId,
    pub clip_gain: f32,
    pub fade_in: f32,
    pub fade_out: f32,
    pub start_pos: f32,
    pub end_pos: f32,
    pub dummy_waveform: Vec<f32>,
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
pub mod editor;

#[derive(Debug, Clone)]
pub struct CommandPalette {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub selected_idx: usize,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub label: String,
    pub action: actions::UserAction,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            selected_idx: 0,
        }
    }

    pub fn update_search(&mut self) {
        let mut items = Vec::new();
        // Templates / Aliases
        items.push(("Sine Oscillator", actions::UserAction::AddSource { name: "Sine".into(), channels: 2 }));
        items.push(("Noise", actions::UserAction::AddSource { name: "Noise".into(), channels: 2 }));
        items.push(("Gain / Amp", actions::UserAction::AddProcessor { name: "Gain".into(), channels: 2 }));
        items.push(("LPF (Biquad)", actions::UserAction::AddProcessor { name: "Filter".into(), channels: 2 }));
        items.push(("Compressor", actions::UserAction::AddProcessor { name: "Compressor".into(), channels: 2 }));
        items.push(("Delay", actions::UserAction::AddProcessor { name: "Delay".into(), channels: 2 }));
        items.push(("ADSR Envelope", actions::UserAction::AddProcessor { name: "Envelope".into(), channels: 1 }));
        
        // Simple fuzzy/substring match
        let query = self.query.to_lowercase();
        self.results = items.into_iter()
            .filter(|(label, _)| {
                let l = label.to_lowercase();
                l.contains(&query) || (query == "lpf" && l.contains("filter")) || (query == "vca" && l.contains("gain"))
            })
            .map(|(label, action)| SearchResult { label: label.into(), action })
            .collect();
        
        if self.selected_idx >= self.results.len() {
            self.selected_idx = 0;
        }
    }
}

pub fn run_gui() -> eframe::Result<()> {
    let cwd = std::env::current_dir().expect("failed to get current dir");
    let ir_path = cwd.join(".dirtydata").join("ir").join("current.json");

    let initial_graph = if ir_path.exists() {
        let json = std::fs::read_to_string(&ir_path).expect("failed to read IR");
        serde_json::from_str(&json).unwrap_or_default()
    } else {
        Graph::default()
    };

    let shadow_graph = Arc::new(ArcSwap::from_pointee(initial_graph.clone()));
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

    let shared_state_ptr = Arc::new(dirtydata_runtime::SharedState::new());
    let (_midi_tx, midi_rx) = crossbeam_channel::unbounded::<dirtydata_runtime::nodes::MidiEvent>();
    let engine = Arc::new(dirtydata_runtime::AudioEngine::new(shared_state_ptr.clone(), midi_rx));
    let _ = engine.command_tx.send(dirtydata_runtime::EngineCommand::ReplaceGraph(initial_graph.clone()));

    let shared_state = Some(shared_state_ptr);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("DirtyData - The Workbench"),
        ..Default::default()
    };

    eframe::run_native(
        "dirtydata_gui",
        native_options,
        Box::new(|_cc| {
            // Leak watcher to keep it running
            Box::leak(Box::new(_watcher));
            Ok(Box::new(app::DirtyDataApp::new(shadow_graph, action_tx, layout, shared_state, cwd.clone(), Some(engine.command_tx.clone()))))
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


