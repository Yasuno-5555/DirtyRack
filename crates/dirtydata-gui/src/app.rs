use crate::{InteractionState, NodeVisuals, UiLayout};
use arc_swap::ArcSwap;
use crossbeam_channel::Sender;
use dirtydata_core::actions::UserAction;
use dirtydata_core::ir::Graph;
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
}

impl DirtyDataApp {
    pub fn new(
        shadow_graph: Arc<ArcSwap<Graph>>,
        action_tx: Sender<UserAction>,
        layout: UiLayout,
        shared_state: Option<Arc<dirtydata_runtime::SharedState>>
    ) -> Self {
        Self {
            shadow_graph,
            layout,
            interaction: InteractionState::default(),
            editor: crate::editor::NodeEditor::new(),
            shared_state,
            action_tx,
            project_root: std::env::current_dir().unwrap_or_default(),
            needs_save: false,
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

        egui::CentralPanel::default().show(ctx, |ui| {
            let viewport = ui.max_rect();

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

            // --- Node Editor ---
            self.editor.draw(ui, &graph, &mut self.layout, &mut self.interaction, self.shared_state.as_deref());

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
    }
}
