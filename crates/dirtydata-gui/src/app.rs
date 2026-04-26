use crate::{InteractionState, NodeVisuals, UiLayout, CommandPalette};
use arc_swap::ArcSwap;
use crossbeam_channel::Sender;
use dirtydata_core::actions::UserAction;
use dirtydata_core::ir::Graph;
use dirtydata_core::patch::Patch;
use dirtydata_core::types::{ConfidenceScore, DataType, ExecutionDomain, PortDirection, PortRef, StableId};
use egui::{vec2, Color32, Painter, Pos2, Rect, Stroke, Vec2};
use std::sync::Arc;

pub struct DirtyDataApp {
    shadow_graph: Arc<ArcSwap<Graph>>,
    layout: UiLayout,
    interaction: InteractionState,
    editor: crate::editor::NodeEditor,
    shared_state: Option<Arc<dirtydata_runtime::SharedState>>,
    action_tx: Sender<UserAction>,
    project_root: std::path::PathBuf,
    needs_save: bool,
    command_palette: Option<CommandPalette>,
    history: Vec<Patch>,
    morph_state: MorphState,
    engine_tx: Option<Sender<dirtydata_runtime::EngineCommand>>,
}

#[derive(PartialEq, Clone, Copy)]
pub enum MorphCurve {
    Linear,
    Logarithmic,
    Exponential,
}

pub struct MorphState {
    pub a: Option<Graph>,
    pub b: Option<Graph>,
    pub c: Option<Graph>,
    pub d: Option<Graph>,
    pub x: f32,
    pub y: f32,
    pub curve: MorphCurve,
    pub midi_cc_x: Option<u8>,
    pub midi_cc_y: Option<u8>,
}

impl DirtyDataApp {
    pub fn new(
        shadow_graph: Arc<ArcSwap<Graph>>,
        action_tx: Sender<UserAction>,
        layout: UiLayout,
        shared_state: Option<Arc<dirtydata_runtime::SharedState>>,
        project_root: std::path::PathBuf,
        engine_tx: Option<Sender<dirtydata_runtime::EngineCommand>>,
    ) -> Self {
        Self {
            shadow_graph,
            layout,
            interaction: InteractionState::default(),
            editor: crate::editor::NodeEditor::new(),
            shared_state,
            action_tx,
            project_root,
            needs_save: false,
            command_palette: None,
            history: Vec::new(),
            morph_state: MorphState { a: None, b: None, c: None, d: None, x: 0.5, y: 0.5, curve: MorphCurve::Linear, midi_cc_x: None, midi_cc_y: None },
            engine_tx,
        }
    }

    fn draw_oscilloscope(&self, ui: &mut egui::Ui, shared: &dirtydata_runtime::SharedState) {
        let rect = ui.max_rect();
        let scope_rect = Rect::from_min_size(
            rect.left_bottom() + vec2(20.0, -120.0),
            vec2(300.0, 100.0),
        );

        let painter = ui.painter();
        painter.rect_filled(scope_rect, 4.0, Color32::from_rgba_unmultiplied(20, 20, 20, 180));
        painter.rect_stroke(scope_rect, 4.0, Stroke::new(1.0, Color32::from_gray(100)));

        let mut points = Vec::new();
        let mut i = 0;
        // Drain current buffer
        while let Some(val) = shared.scope_buffer.pop() {
            if i >= 300 { break; }
            let x = scope_rect.left() + i as f32;
            let y = scope_rect.center().y - val * 40.0;
            points.push(Pos2::new(x, y));
            i += 1;
        }

        if points.len() > 1 {
            painter.add(egui::Shape::line(points, Stroke::new(1.5, Color32::from_rgb(0, 255, 200))));
        }
        
        ui.ctx().request_repaint(); // Keep scope moving
    }
}

impl eframe::App for DirtyDataApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let graph = self.shadow_graph.load();

        // --- Side Panel (History) ---
        egui::SidePanel::left("history_panel").resizable(true).show(ctx, |ui| {
            ui.heading("Project History");
            ui.separator();
            
            egui::ScrollArea::vertical().show(ui, |ui| {
                for patch in self.history.iter().rev() {
                    ui.group(|ui| {
                        ui.label(format!("Patch: {}", &patch.identity.to_string()[..8]));
                        ui.label(format!("Ops: {}", patch.operations.len()));
                        if let Some(intent) = &patch.intent_ref {
                            ui.label(format!("Intent: {}", &intent.to_string()[..8]));
                        }
                        if ui.button("Checkout").clicked() {
                            // TODO: Implementation for checking out historical states
                        }
                    });
                }
            });
        });

        // --- Right Panel (Sample Editor) ---
        if let Some(editor) = &mut self.interaction.sample_editor {
            egui::SidePanel::right("sample_editor_panel").resizable(true).min_width(300.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Sample Editor");
                    if ui.button("X").clicked() {
                        self.interaction.sample_editor = None;
                    }
                });
                ui.separator();
                
                if let Some(e) = &mut self.interaction.sample_editor {
                    ui.label(format!("Editing Node: {}", &e.target_node.to_string()[..8]));
                    
                    // Controls
                    ui.add(egui::Slider::new(&mut e.clip_gain, 0.0..=2.0).text("Clip Gain"));
                    ui.add(egui::Slider::new(&mut e.start_pos, 0.0..=e.end_pos).text("Start Pos"));
                    ui.add(egui::Slider::new(&mut e.end_pos, e.start_pos..=1.0).text("End Pos"));
                    ui.add(egui::Slider::new(&mut e.fade_in, 0.0..=0.5).text("Fade In"));
                    ui.add(egui::Slider::new(&mut e.fade_out, 0.0..=0.5).text("Fade Out"));
                    
                    ui.separator();
                    
                    // Waveform View
                    let (rect, _resp) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), egui::Sense::hover());
                    let painter = ui.painter();
                    painter.rect_filled(rect, 4.0, Color32::from_gray(20));
                    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, Color32::from_gray(100)));
                    
                    let mut points = Vec::new();
                    let width = rect.width();
                    let height = rect.height();
                    let mid_y = rect.center().y;
                    
                    // Draw mock waveform
                    if !e.dummy_waveform.is_empty() {
                        for i in 0..width as usize {
                            let idx = (i as f32 / width * e.dummy_waveform.len() as f32) as usize;
                            if idx < e.dummy_waveform.len() {
                                let val = e.dummy_waveform[idx] * e.clip_gain;
                                let x = rect.left() + i as f32;
                                let y = mid_y - val * (height / 2.0);
                                points.push(Pos2::new(x, y));
                            }
                        }
                        painter.add(egui::Shape::line(points, Stroke::new(1.0, Color32::from_rgb(100, 255, 100))));
                    }
                    
                    // Draw Fades and Regions
                    let start_x = rect.left() + width * e.start_pos;
                    let end_x = rect.left() + width * e.end_pos;
                    painter.vline(start_x, rect.y_range(), Stroke::new(1.5, Color32::WHITE));
                    painter.vline(end_x, rect.y_range(), Stroke::new(1.5, Color32::WHITE));
                    
                    let fade_in_x = start_x + width * e.fade_in;
                    let fade_out_x = end_x - width * e.fade_out;
                    painter.line_segment([Pos2::new(start_x, rect.bottom()), Pos2::new(fade_in_x, rect.top())], Stroke::new(1.0, Color32::YELLOW));
                    painter.line_segment([Pos2::new(fade_out_x, rect.top()), Pos2::new(end_x, rect.bottom())], Stroke::new(1.0, Color32::YELLOW));
                }
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let viewport = ui.max_rect();

            // --- Breadcrumbs Navigation ---
            if !self.interaction.graph_path.is_empty() {
                ui.horizontal(|ui| {
                    if ui.button("Root").clicked() {
                        self.interaction.graph_path.clear();
                    }
                    let mut pop_count = 0;
                    for (i, id) in self.interaction.graph_path.iter().enumerate() {
                        ui.label(">");
                        if ui.button(format!("SubGraph {}", &id.to_string()[..6])).clicked() {
                            pop_count = self.interaction.graph_path.len() - (i + 1);
                        }
                    }
                    for _ in 0..pop_count {
                        self.interaction.graph_path.pop();
                    }
                });
                ui.separator();
            }

            // --- Pan & Zoom Interaction ---
            if ui.rect_contains_pointer(viewport) {
                let scroll = ui.input(|i| i.smooth_scroll_delta);
                if scroll.y != 0.0 {
                    let zoom_delta = 1.0 + scroll.y * 0.001;
                    self.layout.zoom = (self.layout.zoom * zoom_delta).clamp(0.1, 5.0);
                    self.needs_save = true;
                }

                if ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle)) {
                    let delta = ui.input(|i| i.pointer.delta());
                    self.layout.pan[0] += delta.x;
                    self.layout.pan[1] += delta.y;
                    self.needs_save = true;
                }
            }

            // Background
            ui.painter().rect_filled(viewport, 0.0, Color32::from_gray(10));

            // Resolve current active graph
            let mut active_graph = graph.as_ref().clone();
            for id in &self.interaction.graph_path {
                if let Some(node) = active_graph.nodes.get(id) {
                    if let Some(json) = node.config.get("graph_json").and_then(|v| v.as_string()) {
                        if let Ok(g) = serde_json::from_str::<Graph>(json) {
                            active_graph = g;
                        }
                    }
                }
            }

            // --- Node Editor ---
            self.editor.draw(ui, &active_graph, &mut self.layout, &mut self.interaction, self.shared_state.as_deref(), &self.action_tx);

            // --- Vector Performance Layer (Warped Snapshot) ---
            egui::TopBottomPanel::bottom("performance_panel").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Vector Snapshot").strong().color(Color32::from_rgb(255, 100, 200)));
                        ui.horizontal(|ui| {
                            if ui.button("Save A").clicked() { self.morph_state.a = Some(active_graph.clone()); }
                            if ui.button("Save B").clicked() { self.morph_state.b = Some(active_graph.clone()); }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Save C").clicked() { self.morph_state.c = Some(active_graph.clone()); }
                            if ui.button("Save D").clicked() { self.morph_state.d = Some(active_graph.clone()); }
                        });
                    });
                    
                    ui.separator();
                    
                    // Draw XY Pad
                    let pad_size = 120.0;
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(pad_size, pad_size), egui::Sense::click_and_drag());
                    let painter = ui.painter();
                    painter.rect_filled(rect, 4.0, Color32::from_gray(20));
                    painter.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, Color32::from_gray(100)));
                    
                    // Draw labels
                    painter.text(rect.left_top() + egui::vec2(5.0, 5.0), egui::Align2::LEFT_TOP, "A", egui::FontId::proportional(12.0), Color32::GRAY);
                    painter.text(rect.right_top() + egui::vec2(-5.0, 5.0), egui::Align2::RIGHT_TOP, "B", egui::FontId::proportional(12.0), Color32::GRAY);
                    painter.text(rect.left_bottom() + egui::vec2(5.0, -5.0), egui::Align2::LEFT_BOTTOM, "C", egui::FontId::proportional(12.0), Color32::GRAY);
                    painter.text(rect.right_bottom() + egui::vec2(-5.0, -5.0), egui::Align2::RIGHT_BOTTOM, "D", egui::FontId::proportional(12.0), Color32::GRAY);
                    
                    if response.dragged() || response.clicked() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let rel = pos - rect.min;
                            self.morph_state.x = (rel.x / rect.width()).clamp(0.0, 1.0);
                            self.morph_state.y = (rel.y / rect.height()).clamp(0.0, 1.0);
                        }
                    }
                    
                    let puck_pos = rect.min + egui::vec2(self.morph_state.x * rect.width(), self.morph_state.y * rect.height());
                    painter.circle_filled(puck_pos, 6.0, Color32::from_rgb(255, 100, 200));
                    
                    ui.separator();
                    
                    ui.vertical(|ui| {
                        ui.label("Curve:");
                        ui.radio_value(&mut self.morph_state.curve, MorphCurve::Linear, "Lin");
                        ui.radio_value(&mut self.morph_state.curve, MorphCurve::Logarithmic, "Log");
                        ui.radio_value(&mut self.morph_state.curve, MorphCurve::Exponential, "Exp");
                    });

                    ui.separator();

                    // Parameter interpolation logic (Bilinear with Presence Mask)
                    if response.dragged() || response.clicked() {
                        if let Some(engine_tx) = &self.engine_tx {
                            let mut all_ids = std::collections::HashSet::new();
                            for g in [&self.morph_state.a, &self.morph_state.b, &self.morph_state.c, &self.morph_state.d].iter().filter_map(|x| x.as_ref()) {
                                for id in g.nodes.keys() {
                                    all_ids.insert(*id);
                                }
                            }

                            for id in all_ids {
                                let mut all_params = std::collections::HashSet::new();
                                for g in [&self.morph_state.a, &self.morph_state.b, &self.morph_state.c, &self.morph_state.d].iter().filter_map(|x| x.as_ref()) {
                                    if let Some(node) = g.nodes.get(&id) {
                                        for key in node.config.keys() {
                                            all_params.insert(key.clone());
                                        }
                                    }
                                }
                                
                                for key in all_params {
                                    let get_val = |g: &Option<Graph>| -> Option<f64> {
                                        g.as_ref()?.nodes.get(&id)?.config.get(&key).and_then(|v| {
                                            if let dirtydata_core::types::ConfigValue::Float(f) = v { Some(*f) } else { None }
                                        })
                                    };
                                    
                                    let va = get_val(&self.morph_state.a);
                                    let vb = get_val(&self.morph_state.b);
                                    let vc = get_val(&self.morph_state.c);
                                    let vd = get_val(&self.morph_state.d);
                                    
                                    // Bilinear interpolation with presence mask
                                    let curve = self.morph_state.curve;
                                    let lerp = |v1: Option<f64>, v2: Option<f64>, t: f64| -> Option<f64> {
                                        match (v1, v2) {
                                            (Some(a), Some(b)) => {
                                                match curve {
                                                    MorphCurve::Linear => Some(a + (b - a) * t),
                                                    MorphCurve::Logarithmic => {
                                                        let la = a.max(0.0001).ln();
                                                        let lb = b.max(0.0001).ln();
                                                        Some((la + (lb - la) * t).exp())
                                                    },
                                                    MorphCurve::Exponential => Some(a + (b - a) * t.powf(2.0)),
                                                }
                                            },
                                            (Some(a), None) => Some(a),
                                            (None, Some(b)) => Some(b),
                                            (None, None) => None,
                                        }
                                    };
                                    
                                    let top = lerp(va, vb, self.morph_state.x as f64);
                                    let bottom = lerp(vc, vd, self.morph_state.x as f64);
                                    let final_val = lerp(top, bottom, self.morph_state.y as f64);
                                    
                                    if let Some(val) = final_val {
                                        let _ = engine_tx.send(dirtydata_runtime::EngineCommand::UpdateParameter(dirtydata_runtime::ParameterUpdate {
                                            node_id: id,
                                            param: key,
                                            value: val as f32,
                                        }));
                                    }
                                }
                            }
                        }
                    }
                });
            });

            // --- Oscilloscope (Overlay) ---
            if let Some(shared) = &self.shared_state {
                self.draw_oscilloscope(ui, shared);
            }
        });

        // Auto-save if needed
        if self.needs_save && !ctx.input(|i| i.pointer.any_down()) {
            let _ = self.layout.save(&self.project_root);
            self.needs_save = false;
        }

        // If dragging cable, ensure we repaint
        if self.interaction.dragging_cable.is_some() || self.interaction.dragging_node.is_some() {
            ctx.request_repaint();
        }

        // --- Keyboard Shortcuts ---
        if ctx.input(|i| i.key_pressed(egui::Key::Tab)) && self.command_palette.is_none() {
            self.command_palette = Some(CommandPalette::new());
        }

        // Cmd+D for Duplication
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::D)) {
            let selected: Vec<StableId> = self.editor.selection.iter().cloned().collect();
            for id in selected {
                // Emit duplication action (handled by storage worker)
                let _ = self.action_tx.send(UserAction::DuplicateNode { node_id: id });
            }
        }

        // Space for Connection (Auto-complete dragging cable)
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) && self.interaction.dragging_cable.is_some() {
            // Find nearest compatible port logic would go here
        }

        // --- Command Palette Overlay ---
        if let Some(mut palette) = self.command_palette.take() {
            egui::Window::new("Create Node")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Search for a node...");
                    let resp = ui.text_edit_singleline(&mut palette.query);
                    resp.request_focus();
                    
                    palette.update_search();
                    
                    for (i, result) in palette.results.iter().enumerate() {
                        let is_selected = i == palette.selected_idx;
                        let label = if is_selected { format!("> {}", result.label) } else { result.label.clone() };
                        if ui.selectable_label(is_selected, label).clicked() || 
                           (is_selected && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            let _ = self.action_tx.send(result.action.clone());
                            self.command_palette = None;
                            return;
                        }
                    }

                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.command_palette = None;
                    } else {
                        self.command_palette = Some(palette);
                    }
                });
        }
    }
}
