use egui::{vec2, Color32, Painter, Pos2, Rect, Stroke, Vec2, Rounding, Shadow};
use dirtydata_core::ir::{Graph, Node};
use dirtydata_core::types::{StableId, PortRef, PortDirection, ExecutionDomain};
use dirtydata_core::actions::UserAction;
use dirtydata_runtime::SharedState;
use crossbeam_channel::Sender;
use std::collections::{HashMap, HashSet};
use crate::{UiLayout, NodeVisuals, InteractionState};

pub struct NodeEditor {
    pub selection: HashSet<StableId>,
}

impl NodeEditor {
    pub fn new() -> Self {
        Self {
            selection: HashSet::new(),
        }
    }

    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        graph: &Graph,
        layout: &mut UiLayout,
        interaction: &mut InteractionState,
        shared_state: Option<&SharedState>,
        action_tx: &Sender<UserAction>,
    ) {
        let viewport = ui.max_rect();
        let painter = ui.painter();
        let zoom = layout.zoom;
        let pan = Vec2::from(layout.pan);

        // 1. Grid (Background)
        self.draw_grid(painter, viewport, zoom, pan);

        // 2. Connections
        self.draw_connections(painter, graph, layout, interaction, zoom, pan, shared_state);

        // 3. Nodes
        let node_ids: Vec<StableId> = graph.nodes.keys().cloned().collect();
        for id in node_ids {
            if let Some(node) = graph.nodes.get(&id) {
                self.draw_node(ui, id, node, layout, interaction, shared_state, action_tx);
            }
        }

        // 4. Marquee Selection logic
        self.handle_marquee(ui, graph, layout, interaction);

        // 5. Quick Replace Hotkey & Popup
        if ui.input(|i| i.key_pressed(egui::Key::Tab)) {
            if let Some(&id) = self.selection.iter().next() {
                interaction.quick_replace_target = Some(id);
                interaction.quick_replace_input.clear();
            }
        }

        if let Some(target_id) = interaction.quick_replace_target {
            if let Some(node) = graph.nodes.get(&target_id) {
                if let Some(visuals) = layout.nodes.get(&target_id) {
                    let world_pos = Pos2::new(visuals.position[0], visuals.position[1]);
                    let screen_pos = world_pos * zoom + pan;
                    
                    let window_rect = egui::Rect::from_min_size(screen_pos - vec2(0.0, 30.0), vec2(160.0, 30.0));
                    
                    egui::Window::new("Quick Replace")
                        .fixed_pos(window_rect.min)
                        .title_bar(false)
                        .resizable(false)
                        .show(ui.ctx(), |ui| {
                            let response = ui.text_edit_singleline(&mut interaction.quick_replace_input);
                            response.request_focus();
                            
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                let node_name = node.config.get("name").and_then(|v| v.as_string()).cloned().unwrap_or_default();
                                let _ = action_tx.send(UserAction::ReplaceNode {
                                    name: node_name,
                                    new_kind_name: interaction.quick_replace_input.clone(),
                                });
                                interaction.quick_replace_target = None;
                            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                interaction.quick_replace_target = None;
                            }
                        });
                }
            } else {
                interaction.quick_replace_target = None;
            }
        }
    }

    fn draw_grid(&self, painter: &Painter, viewport: Rect, zoom: f32, pan: Vec2) {
        let grid_size = 50.0 * zoom;
        let color = Color32::from_gray(25);
        
        let start_x = (pan.x % grid_size) - grid_size;
        let start_y = (pan.y % grid_size) - grid_size;

        for x in 0..=((viewport.width() / grid_size) as i32 + 2) {
            let x_pos = start_x + x as f32 * grid_size;
            painter.line_segment([Pos2::new(x_pos, viewport.top()), Pos2::new(x_pos, viewport.bottom())], Stroke::new(1.0, color));
        }

        for y in 0..=((viewport.height() / grid_size) as i32 + 2) {
            let y_pos = start_y + y as f32 * grid_size;
            painter.line_segment([Pos2::new(viewport.left(), y_pos), Pos2::new(viewport.right(), y_pos)], Stroke::new(1.0, color));
        }
    }

    fn draw_node(
        &mut self,
        ui: &mut egui::Ui,
        id: StableId,
        node: &Node,
        layout: &mut UiLayout,
        interaction: &mut InteractionState,
        shared_state: Option<&SharedState>,
        action_tx: &Sender<UserAction>,
    ) {
        let zoom = layout.zoom;
        let pan = Vec2::from(layout.pan);
        let visuals = layout.nodes.entry(id).or_insert(NodeVisuals {
            position: [100.0, 100.0],
            is_collapsed: false,
        });

        let world_pos = Pos2::new(visuals.position[0], visuals.position[1]);
        let screen_pos = world_pos * zoom + pan;
        let size = vec2(160.0, 100.0) * zoom;
        let rect = Rect::from_min_size(screen_pos, size);

        let response = ui.interact(rect, ui.make_persistent_id(id), egui::Sense::click_and_drag());
        
        if response.clicked() {
            if !ui.input(|i| i.modifiers.shift) {
                self.selection.clear();
            }
            self.selection.insert(id);
        }

        if response.double_clicked() {
            if node.kind == dirtydata_core::types::NodeKind::SubGraph {
                interaction.graph_path.push(id);
            } else if node.config.contains_key("file_path") || node.config.contains_key("sample") || node.kind == dirtydata_core::types::NodeKind::Source {
                let mut dummy = Vec::new();
                for i in 0..400 {
                    dummy.push((i as f32 * 0.1).sin() * (i as f32 * 0.05).cos() * 0.8);
                }
                interaction.sample_editor = Some(crate::SampleEditorState {
                    target_node: id,
                    clip_gain: 1.0,
                    fade_in: 0.1,
                    fade_out: 0.1,
                    start_pos: 0.0,
                    end_pos: 1.0,
                    dummy_waveform: dummy,
                });
            }
        }

        if response.dragged() {
            let delta = response.drag_delta() / zoom;
            visuals.position[0] += delta.x;
            visuals.position[1] += delta.y;
            
            // Grid Snapping (20px)
            visuals.position[0] = (visuals.position[0] / 20.0).round() * 20.0;
            visuals.position[1] = (visuals.position[1] / 20.0).round() * 20.0;

            self.selection.insert(id); // Select on drag
        }

        let painter = ui.painter();

        // Glassmorphism Body
        let bg_color = Color32::from_rgba_unmultiplied(40, 40, 50, 200);
        painter.rect_filled(rect, 8.0 * zoom, bg_color);
        
        // Neon Glow Border / Confidence Color
        let border_color = if self.selection.contains(&id) {
            Color32::from_rgb(255, 200, 0)
        } else if node.confidence == dirtydata_core::types::ConfidenceScore::Suspicious {
            Color32::from_rgb(255, 50, 50)
        } else {
            Color32::from_rgba_unmultiplied(100, 100, 255, 100)
        };
        painter.rect_stroke(rect, 8.0 * zoom, Stroke::new(1.5 * zoom, border_color));

        // Freeze Button (Top Right)
        let freeze_rect = Rect::from_min_size(rect.right_top() + vec2(-30.0, 5.0) * zoom, vec2(25.0, 25.0) * zoom);
        let freeze_resp = ui.interact(freeze_rect, ui.make_persistent_id(("freeze", id)), egui::Sense::click());
        painter.rect_filled(freeze_rect, 4.0 * zoom, Color32::from_gray(60));
        painter.text(freeze_rect.center(), egui::Align2::CENTER_CENTER, "❄", egui::FontId::proportional(12.0 * zoom), Color32::WHITE);
        
        if freeze_resp.clicked() {
            let node_name = node.config.get("name").and_then(|v| v.as_string()).cloned().unwrap_or_else(|| "Unknown".into());
            // Emit Freeze Action
            let _ = action_tx.send(UserAction::FreezeNode {
                name: node_name,
                length_secs: 10.0, // Default
            });
        }

        // Node Title
        painter.text(
            rect.center_top() + vec2(0.0, 10.0 * zoom),
            egui::Align2::CENTER_TOP,
            format!("{:?}", node.kind),
            egui::FontId::proportional(14.0 * zoom),
            Color32::WHITE,
        );

        // Metering Visualization
        if let Some(shared) = shared_state {
            if let Some(level) = shared.node_levels.get(&id) {
                let peak = *level;
                let meter_width = (rect.width() - 20.0 * zoom).max(0.0);
                let meter_rect = Rect::from_min_size(
                    rect.left_bottom() + vec2(10.0, -15.0) * zoom,
                    vec2(meter_width * peak.min(1.0), 4.0 * zoom),
                );
                painter.rect_filled(meter_rect, 2.0 * zoom, Color32::from_rgb(100, 255, 100));
            }
        }

        // Ports
        for (i, port) in node.ports.iter().enumerate() {
            let is_input = port.direction == PortDirection::Input;
            let x = if is_input { rect.left() } else { rect.right() };
            let y = rect.top() + (35.0 + i as f32 * 20.0) * zoom;
            let port_pos = Pos2::new(x, y);

            let port_ref = PortRef { node_id: id, port_name: port.name.clone() };
            self.draw_port(ui, port_pos, port_ref, is_input, zoom, interaction);
        }

        // --- Interactive Controls ---
        self.draw_node_controls(ui, rect, id, node, action_tx, zoom);

        // --- Visualizer Overlay ---
        self.draw_visualizer(ui, rect, node, shared_state, zoom);
    }

    fn draw_visualizer(&self, ui: &mut egui::Ui, rect: Rect, node: &Node, _shared_state: Option<&SharedState>, zoom: f32) {
        let painter = ui.painter();
        let name = node.config.get("name").and_then(|v| v.as_string()).map(|s| s.as_str()).unwrap_or("");
        
        let vis_rect = Rect::from_min_size(
            rect.center() + vec2(-50.0, 0.0) * zoom,
            vec2(100.0, 40.0) * zoom,
        );

        match name {
            "Filter" | "Biquad" => {
                painter.rect_filled(vis_rect, 2.0 * zoom, Color32::from_gray(30));
                let cutoff = node.config.get("frequency").and_then(|v| v.as_float()).unwrap_or(1000.0) as f32;
                
                // Draw a simple filter curve (representative)
                let mut points = Vec::new();
                for i in 0..50 {
                    let f = (i as f32 / 50.0).powf(2.0) * 20000.0;
                    let mag = if f < cutoff { 1.0 } else { (cutoff / f).powi(2) };
                    let px = vis_rect.left() + (i as f32 / 50.0) * vis_rect.width();
                    let py = vis_rect.bottom() - mag * vis_rect.height() * 0.8;
                    points.push(Pos2::new(px, py));
                }
                painter.add(egui::Shape::line(points, Stroke::new(1.5 * zoom, Color32::from_rgb(100, 255, 100))));
                
                // Cutoff Overlay Line
                let cutoff_x = vis_rect.left() + (cutoff / 20000.0).sqrt() * vis_rect.width();
                painter.line_segment(
                    [Pos2::new(cutoff_x, vis_rect.top()), Pos2::new(cutoff_x, vis_rect.bottom())],
                    Stroke::new(1.0 * zoom, Color32::WHITE.gamma_multiply(0.5))
                );
            }
            "Oscillator" | "Sine" => {
                // Mini oscilloscope inside node
                painter.rect_filled(vis_rect, 2.0 * zoom, Color32::from_gray(30));
                
                let mut points = Vec::new();
                for i in 0..20 {
                    let val = (i as f32 * 0.5).sin(); // Placeholder sine wave
                    let px = vis_rect.left() + (i as f32 / 20.0) * vis_rect.width();
                    let py = vis_rect.center().y - val * vis_rect.height() * 0.4;
                    points.push(Pos2::new(px, py));
                }
                painter.add(egui::Shape::line(points, Stroke::new(1.0 * zoom, Color32::from_rgb(0, 255, 255))));
            }
            _ => {}
        }
    }

    fn draw_node_controls(&self, ui: &mut egui::Ui, rect: Rect, _id: StableId, node: &Node, action_tx: &Sender<UserAction>, zoom: f32) {
        let name = node.config.get("name").and_then(|v| v.as_string()).map(|s| s.as_str()).unwrap_or("");
        
        let control_rect = Rect::from_min_size(
            rect.left_bottom() + vec2(10.0, -35.0) * zoom,
            vec2(80.0, 20.0) * zoom,
        );

        match name {
            "Gain" => {
                let mut val = node.config.get("gain").and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
                ui.put(control_rect, |ui: &mut egui::Ui| {
                    let resp = ui.add(egui::Slider::new(&mut val, 0.0..=2.0).text("G").show_value(false));
                    if resp.changed() {
                        let _ = action_tx.send(UserAction::SetConfig {
                            node: node.config.get("name").and_then(|v| v.as_string()).cloned().unwrap_or_default(),
                            key: "gain".into(),
                            value: serde_json::json!(val),
                        });
                    }
                    resp
                });
            }
            "Filter" | "Biquad" => {
                let mut val = node.config.get("frequency").and_then(|v| v.as_float()).unwrap_or(1000.0) as f32;
                ui.put(control_rect, |ui: &mut egui::Ui| {
                    let resp = ui.add(egui::Slider::new(&mut val, 20.0..=20000.0).logarithmic(true).text("F").show_value(false));
                    if resp.changed() {
                        let _ = action_tx.send(UserAction::SetConfig {
                            node: node.config.get("name").and_then(|v| v.as_string()).cloned().unwrap_or_default(),
                            key: "frequency".into(),
                            value: serde_json::json!(val),
                        });
                    }
                    resp
                });
            }
            _ => {}
        }
    }

    fn draw_port(
        &self,
        ui: &mut egui::Ui,
        pos: Pos2,
        port_ref: PortRef,
        is_input: bool,
        zoom: f32,
        interaction: &mut InteractionState,
    ) {
        let painter = ui.painter();
        let color = Color32::from_rgb(100, 200, 255);
        let rect = Rect::from_center_size(pos, vec2(10.0, 10.0) * zoom);
        
        let response = ui.interact(rect, ui.make_persistent_id(&port_ref), egui::Sense::click_and_drag());
        
        painter.circle_filled(pos, 5.0 * zoom, color);
        if response.hovered() {
            painter.circle_stroke(pos, 8.0 * zoom, Stroke::new(1.0, Color32::WHITE));
        }

        if response.drag_started() && !is_input {
            interaction.dragging_cable = Some(port_ref);
        }
    }

    fn draw_connections(
        &self,
        painter: &Painter,
        graph: &Graph,
        layout: &UiLayout,
        interaction: &InteractionState,
        zoom: f32,
        pan: Vec2,
        shared_state: Option<&SharedState>,
    ) {
        for edge in graph.edges.values() {
            if let (Some(src_v), Some(tgt_v)) = (layout.nodes.get(&edge.source.node_id), layout.nodes.get(&edge.target.node_id)) {
                let src_pos = (Pos2::new(src_v.position[0] + 160.0, src_v.position[1] + 35.0)) * zoom + pan;
                let tgt_pos = (Pos2::new(tgt_v.position[0], tgt_v.position[1] + 35.0)) * zoom + pan;
                
                // Determine color based on port type
                let color = if let Some(node) = graph.nodes.get(&edge.source.node_id) {
                    if let Some(port) = node.ports.iter().find(|p| p.name == edge.source.port_name) {
                        match port.data_type {
                            dirtydata_core::types::DataType::Audio { .. } => Color32::from_rgb(0, 200, 255),
                            dirtydata_core::types::DataType::Control => Color32::from_rgb(200, 100, 255),
                            _ => Color32::from_gray(150),
                        }
                    } else { Color32::from_gray(150) }
                } else { Color32::from_gray(150) };

                // Get real-time level for "texture"
                let level = shared_state.and_then(|s| s.node_levels.get(&edge.source.node_id).map(|r| *r)).unwrap_or(0.0);

                self.draw_bezier(painter, src_pos, tgt_pos, color, zoom, level, edge.source.node_id, shared_state);
            }
        }

        if let Some(src) = &interaction.dragging_cable {
            if let Some(src_v) = layout.nodes.get(&src.node_id) {
                let src_pos = (Pos2::new(src_v.position[0] + 160.0, src_v.position[1] + 35.0)) * zoom + pan;
                if let Some(ptr_pos) = painter.ctx().pointer_interact_pos() {
                    self.draw_bezier(painter, src_pos, ptr_pos, Color32::WHITE, zoom, 0.0, StableId::new(), None);
                }
            }
        }
    }

    fn draw_bezier(&self, painter: &Painter, src: Pos2, tgt: Pos2, color: Color32, zoom: f32, level: f32, edge_source_node_id: StableId, shared_state: Option<&SharedState>) {
        let control_scale = (tgt.x - src.x).abs().max(20.0 * zoom) * 0.5;
        let cp1 = src + vec2(control_scale, 0.0);
        let cp2 = tgt - vec2(control_scale, 0.0);

        let points: Vec<Pos2> = (0..=20)
            .map(|i| {
                let t = i as f32 / 20.0;
                let it = 1.0 - t;
                let p = src.to_vec2() * it * it * it
                    + cp1.to_vec2() * 3.0 * it * it * t
                    + cp2.to_vec2() * 3.0 * it * t * t
                    + tgt.to_vec2() * t * t * t;
                p.to_pos2()
            })
            .collect();
        
        let thickness = (1.5 + level * 3.0) * zoom;
        painter.add(egui::Shape::line(points.clone(), Stroke::new(thickness, color.gamma_multiply(0.6))));

        // Signal Flow Animation (Dots)
        let time = painter.ctx().input(|i| i.time);
        let speed = 2.0 + level * 5.0;
        let t_offset = (time as f32 * speed) % 1.0;
        
        for j in 0..3 {
            let t = (t_offset + j as f32 * 0.33) % 1.0;
            let idx = (t * (points.len() - 1) as f32) as usize;
            if let Some(pos) = points.get(idx) {
                painter.circle_filled(*pos, 2.5 * zoom, color);
            }
        }

        // --- Phase 5.6 Cable Oscilloscope ---
        if let Some(shared) = shared_state {
            // Auto-subscribe if not already
            if !shared.probe_buffers.contains_key(&edge_source_node_id) {
                 shared.probe_buffers.insert(edge_source_node_id, std::sync::Arc::new(crossbeam_queue::ArrayQueue::new(128)));
            }

            if let Some(buf) = shared.probe_buffers.get(&edge_source_node_id) {
                let mut samples = Vec::new();
                while let Some(s) = buf.pop() {
                    samples.push(s);
                }
                
                if !samples.is_empty() {
                    let mut scope_points = Vec::new();
                    for (i, s) in samples.iter().enumerate() {
                        let t = i as f32 / samples.len() as f32;
                        let p_idx = (t * (points.len() - 1) as f32) as usize;
                        if let Some(base_pos) = points.get(p_idx) {
                            // Perpendicular offset for oscilloscope
                            let normal = if p_idx + 1 < points.len() {
                                let diff = points[p_idx + 1] - points[p_idx];
                                let len = diff.length();
                                let dir = if len > 0.0 { diff / len } else { vec2(1.0, 0.0) };
                                vec2(-dir.y, dir.x)
                            } else {
                                vec2(0.0, 1.0)
                            };
                            let offset = normal * (*s * 15.0 * zoom);
                            scope_points.push(*base_pos + offset);
                        }
                    }
                    if scope_points.len() > 1 {
                        painter.add(egui::Shape::line(scope_points, Stroke::new(1.0 * zoom, color.gamma_multiply(0.8))));
                    }
                }
            }
        }
        
        painter.ctx().request_repaint();
    }

    fn handle_marquee(&mut self, ui: &mut egui::Ui, graph: &Graph, layout: &UiLayout, interaction: &mut InteractionState) {
        let zoom = layout.zoom;
        let pan = Vec2::from(layout.pan);
        let painter = ui.painter();
        
        let response = ui.interact(ui.max_rect(), ui.id().with("background"), egui::Sense::drag());
        
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                interaction.marquee = Some(Rect::from_min_max(pos, pos));
                if !ui.input(|i| i.modifiers.shift) {
                    self.selection.clear();
                }
            }
        }

        if response.dragged() {
            if let (Some(mut marquee), Some(pos)) = (interaction.marquee, response.interact_pointer_pos()) {
                marquee.max = pos;
                interaction.marquee = Some(marquee);
                
                // Update selection
                for (id, visuals) in &layout.nodes {
                    let node_rect = Rect::from_min_size(
                        Pos2::new(visuals.position[0], visuals.position[1]) * zoom + pan,
                        vec2(160.0, 100.0) * zoom
                    );
                    if marquee.intersects(node_rect) {
                        self.selection.insert(*id);
                    }
                }
            }
        }

        if response.drag_released() {
            interaction.marquee = None;
        }

        if let Some(marquee) = interaction.marquee {
            painter.rect_stroke(marquee, 2.0, Stroke::new(1.0, Color32::from_rgb(255, 200, 0)));
            painter.rect_filled(marquee, 0.0, Color32::from_rgba_unmultiplied(255, 200, 0, 20));
        }
    }
}
