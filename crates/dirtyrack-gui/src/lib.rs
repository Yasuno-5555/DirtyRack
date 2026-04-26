//! DirtyRack GUI — Eurorack Simulator Interface
//!
//! ラックレール描画、フェースプレート、パッチケーブル物理、
//! モジュールブラウザ、リアルタイムオーディオエンジン統合。

pub mod browser;
pub mod cable;
pub mod engine;
pub mod faceplate;
pub mod rack;
pub mod visual_data;

use crate::rack::{CableAction, RackState};
use dirtyrack_modules::registry::ModuleRegistry;
use engine::RackAudioEngine;

pub fn run() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 820.0])
            .with_title("DirtyRack — Eurorack Simulator"),
        ..Default::default()
    };

    eframe::run_native(
        "dirtyrack",
        native_options,
        Box::new(|cc| {
            // Dark theme
            let mut style = (*cc.egui_ctx.style()).clone();
            style.visuals = egui::Visuals::dark();
            cc.egui_ctx.set_style(style);

            Ok(Box::new(DirtyRackApp::new()))
        }),
    )
}

pub struct DirtyRackApp {
    registry: ModuleRegistry,
    rack: RackState,
    engine: Option<RackAudioEngine>,
    visual_reader: Option<triple_buffer::Output<visual_data::VisualSnapshot>>,
    browser_open: bool,
    browser_search: String,
    pan: egui::Vec2,
    zoom: f32,
    selected_module_forensic: Option<u64>, // StableId of module being inspected
}

impl DirtyRackApp {
    pub fn new() -> Self {
        let registry = ModuleRegistry::new();
        let mut rack = RackState::new();
        rack.project_seed = 0xDE7E_B11D;

        let (engine, visual_reader) = match RackAudioEngine::new(rack.sample_rate) {
            Ok((e, v)) => (Some(e), Some(v)),
            Err(_) => (None, None),
        };

        Self {
            registry,
            rack,
            engine,
            visual_reader,
            browser_open: false,
            browser_search: String::new(),
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            selected_module_forensic: None,
        }
    }

    fn rebuild_engine(&mut self) {
        if let Some(engine) = &self.engine {
            let (snapshot, nodes) = self.rack.build_snapshot();
            engine.update_topology(snapshot, nodes);
        }
    }
}

impl eframe::App for DirtyRackApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- Read Visual Projection ---
        let visual_snapshot = self
            .visual_reader
            .as_mut()
            .map(|r| r.read().clone())
            .unwrap_or_default();

        // --- Top Toolbar ---
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("⚡ DirtyRack")
                        .color(egui::Color32::from_rgb(255, 100, 50)),
                );
                ui.separator();

                if ui.button("➕ Add Module").clicked() {
                    self.browser_open = !self.browser_open;
                }

                ui.separator();
                ui.label(format!("Modules: {}", self.rack.modules.len()));
                ui.label(format!("Cables: {}", self.rack.cables.len()));

                ui.separator();
                ui.label("🕰 Aging:");
                if ui
                    .add(egui::Slider::new(&mut self.rack.aging, 0.0..=1.0).text(""))
                    .changed()
                {
                    if let Some(engine) = &self.engine {
                        let _ = engine.sync_aging(self.rack.aging);
                    }
                }
            });
        });

        // --- Forensic Inspector Windows ---
        if let Some(stable_id) = self.selected_module_forensic {
            let mut open = true;
            if let Some(v_state) = visual_snapshot.modules.get(&stable_id) {
                egui::Window::new(format!("Forensic: {}", stable_id))
                    .open(&mut open)
                    .show(ctx, |ui| {
                        if let Some(forensic) = &v_state.forensic {
                            ui.label(egui::RichText::new("Drift Inspector").strong());
                            ui.label(&forensic.internal_state_summary);

                            // Visualize Drift per Voice
                            egui_plot::Plot::new("drift_plot")
                                .height(100.0)
                                .show(ui, |plot_ui| {
                                    let mut points = Vec::new();
                                    for v in 0..16 {
                                        points.push(egui_plot::PlotPoint::new(
                                            v as f64,
                                            forensic.current_drift[v] as f64,
                                        ));
                                    }
                                    plot_ui.bar_chart(
                                        egui_plot::BarChart::new(
                                            points
                                                .into_iter()
                                                .map(|p| egui_plot::Bar::new(p.x, p.y))
                                                .collect(),
                                        )
                                        .name("Current Drift"),
                                    );
                                });

                            ui.separator();
                            ui.label(egui::RichText::new("Personality Offsets").strong());
                            for v in (0..16).step_by(4) {
                                ui.horizontal(|ui| {
                                    for i in 0..4 {
                                        ui.label(format!(
                                            "V{}: {:.3}",
                                            v + i,
                                            forensic.personality_offsets[v + i]
                                        ));
                                    }
                                });
                            }
                        } else {
                            ui.label("No forensic data available for this node.");
                        }
                    });
            }
            if !open {
                self.selected_module_forensic = None;
            }
        }

        // --- Module Browser Panel ---
        if self.browser_open {
            browser::draw_browser(
                ctx,
                &self.registry,
                &mut self.rack,
                &mut self.browser_open,
                &mut self.browser_search,
            );
            self.rebuild_engine();
        }

        // --- Main Rack Area ---
        egui::CentralPanel::default().show(ctx, |ui| {
            let viewport = ui.max_rect();
            let painter = ui.painter().clone();

            // --- Background Interaction (Pan & Zoom & Menu) ---
            let bg_id = ui.make_persistent_id("rack_bg");
            let bg_resp = ui.interact(viewport, bg_id, egui::Sense::click_and_drag());

            // Zoom
            if bg_resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta);
                if scroll.y != 0.0 {
                    self.zoom = (self.zoom * (1.0 + scroll.y * 0.001)).clamp(0.3, 3.0);
                }
            }

            // Pan (Left drag on background or Middle drag)
            if bg_resp.dragged_by(egui::PointerButton::Primary)
                || bg_resp.dragged_by(egui::PointerButton::Middle)
            {
                self.pan += bg_resp.drag_delta();
            }

            // Right Click Context Menu
            bg_resp.context_menu(|ui| {
                ui.label("Rack Menu");
                if ui.button("➕ Add Module").clicked() {
                    self.browser_open = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("🧹 Clear All Cables").clicked() {
                    self.rack.cables.clear();
                    ui.close_menu();
                }
            });

            // Background + Rails (immutable paint pass)
            {
                painter.rect_filled(viewport, 0.0, egui::Color32::from_rgb(25, 22, 20));
                rack::draw_rack_rails(&painter, viewport, self.zoom, self.pan);
            }

            // Draw modules (mutable interaction pass)
            let mut cable_action = None;
            for i in 0..self.rack.modules.len() {
                let action = faceplate::draw_module(
                    ui,
                    &mut self.rack,
                    i,
                    self.zoom,
                    self.pan,
                    &visual_snapshot,
                );
                if let Some(a) = action {
                    cable_action = Some(a);
                }
            }

            // Handle cable actions
            if let Some(action) = cable_action {
                match action {
                    CableAction::InspectForensics { stable_id } => {
                        self.selected_module_forensic = Some(stable_id);
                    }
                    CableAction::ParamUpdate { .. } => {
                        self.rack.handle_action(action, self.zoom, self.pan);
                    }
                    _ => {
                        self.rack.handle_action(action, self.zoom, self.pan);
                        self.rebuild_engine();
                    }
                }
            }

            // Draw cables (immutable paint pass)
            {
                cable::draw_cables(&painter, &self.rack, self.zoom, self.pan);

                if self.rack.dragging_cable.is_some() {
                    if let Some(ptr) = ctx.pointer_interact_pos() {
                        cable::draw_dragging_cable(&painter, &self.rack, ptr, self.zoom, self.pan);
                    }
                    ctx.request_repaint();
                }
            }
        });

        // Request repaint for audio-driven visuals
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}
