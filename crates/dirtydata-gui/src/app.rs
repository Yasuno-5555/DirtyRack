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
    action_tx: Sender<UserAction>,
    project_root: std::path::PathBuf,
    needs_save: bool,
}

impl DirtyDataApp {
    pub fn new(shadow_graph: Arc<ArcSwap<Graph>>, action_tx: Sender<UserAction>, layout: UiLayout) -> Self {
        Self {
            shadow_graph,
            layout,
            interaction: InteractionState::default(),
            action_tx,
            project_root: std::env::current_dir().unwrap_or_default(),
            needs_save: false,
        }
    }

    fn draw_node(&mut self, ui: &mut egui::Ui, id: StableId, node: &dirtydata_core::ir::Node, viewport: Rect, graph: &Graph) {
        let zoom = self.layout.zoom;
        let pan = Vec2::from(self.layout.pan);

        // Find or create layout (Heuristic Auto-Layout)
        let project_root = self.project_root.clone();
        let node_visuals = self.layout.nodes.entry(id).or_insert_with(|| {
            // Find parent to position relative to it
            let mut parent_pos = None;
            for edge in graph.edges.values() {
                if edge.target.node_id == id {
                    if let Some(visuals) = graph.nodes.get(&edge.source.node_id).and_then(|_| {
                        // This is tricky because self.layout is borrowed.
                        // We'll use a simplified version for now.
                        None::<&NodeVisuals>
                    }) {
                        // parent_pos = Some(Pos2::new(visuals.position[0], visuals.position[1]));
                    }
                }
            }

            let p = parent_pos.unwrap_or_else(|| Pos2::new(100.0, 100.0 + (id.to_string().as_bytes()[0] as f32 * 5.0)));
            NodeVisuals {
                position: [p.x + 200.0, p.y],
                is_collapsed: false,
            }
        });

        let mut world_pos = Pos2::new(node_visuals.position[0], node_visuals.position[1]);
        let screen_pos = world_pos * zoom + pan;
        let size = vec2(150.0, 80.0) * zoom;
        let rect = Rect::from_min_size(screen_pos, size);

        // Spatial Culling
        if !viewport.intersects(rect) {
            return;
        }

        // --- Interaction Logic ---
        let node_id = ui.make_persistent_id(id);
        let response = ui.interact(rect, node_id, egui::Sense::drag());
        
        if response.drag_started() {
            self.interaction.dragging_node = Some((id, response.interact_pointer_pos().unwrap() - screen_pos));
        }

        if let Some((dragging_id, offset)) = self.interaction.dragging_node {
            if dragging_id == id {
                if let Some(ptr_pos) = ui.ctx().pointer_interact_pos() {
                    let new_screen_pos = ptr_pos - offset;
                    world_pos = (new_screen_pos - pan) / zoom;
                    node_visuals.position = [world_pos.x, world_pos.y];
                    self.needs_save = true;
                }
            }
        }

        if response.drag_stopped() {
            self.interaction.dragging_node = None;
            let _ = self.layout.save(&project_root);
            self.needs_save = false;
        }

        let painter = ui.painter();
        // 1. Draw Background
        painter.rect_filled(rect, 4.0 * zoom, Color32::from_gray(30));

        // 2. Draw Stroke (Confidence Visualization)
        let time = ui.ctx().input(|i| i.time);
        let stroke = match node.confidence {
            ConfidenceScore::Verified => Stroke::new(2.0 * zoom, Color32::from_gray(180)),
            ConfidenceScore::Inferred => Stroke::new(2.0 * zoom, Color32::from_gray(120)),
            ConfidenceScore::Suspicious => {
                let alpha = (time * 5.0).sin().abs() as f32;
                Stroke::new(3.0 * zoom, Color32::from_rgba_unmultiplied(255, 50, 50, (alpha * 255.0) as u8))
            }
            ConfidenceScore::Unknown => Stroke::new(1.0 * zoom, Color32::from_gray(80)),
        };
        painter.rect_stroke(rect, 4.0 * zoom, stroke);

        if node.confidence != ConfidenceScore::Verified {
            ui.ctx().request_repaint();
        }

        // 3. Draw Name
        let name = node.config.get("name")
            .and_then(|v| v.as_string())
            .map(|s| s.clone())
            .unwrap_or_else(|| id.to_string());
        
        painter.text(
            rect.center_top() + vec2(0.0, 10.0 * zoom),
            egui::Align2::CENTER_TOP,
            name,
            egui::FontId::proportional(14.0 * zoom),
            Color32::WHITE,
        );

        // 4. Draw Ports
        for (i, port) in node.ports.iter().enumerate() {
            let is_input = port.direction == PortDirection::Input;
            let port_pos = if is_input {
                rect.left_top() + vec2(0.0, 30.0 + (i as f32 * 20.0)) * zoom
            } else {
                rect.right_top() + vec2(0.0, 30.0 + (i as f32 * 20.0)) * zoom
            };

            let port_ref = PortRef { node_id: id, port_name: port.name.clone() };
            self.draw_port(ui, port_pos, port_ref, &port.data_type, port.domain, is_input);
        }
    }

    fn draw_port(&mut self, ui: &mut egui::Ui, pos: Pos2, port_ref: PortRef, data_type: &DataType, domain: ExecutionDomain, is_input: bool) {
        let painter = ui.painter();
        let color = match domain {
            ExecutionDomain::Sample => Color32::from_rgb(100, 200, 255),
            ExecutionDomain::Block => Color32::from_rgb(255, 200, 100),
            ExecutionDomain::Timeline => Color32::from_rgb(200, 100, 255),
            ExecutionDomain::Background => Color32::from_gray(100),
        };
        let rect = Rect::from_center_size(pos, vec2(10.0, 10.0));
        let response = ui.interact(rect, ui.make_persistent_id(&port_ref), egui::Sense::click_and_drag());

        // Draw shape
        match domain {
            ExecutionDomain::Sample => {
                painter.circle_filled(pos, 5.0, color);
            }
            ExecutionDomain::Block => {
                // Diamond
                let s = 5.0;
                let points = vec![
                    pos + vec2(0.0, -s),
                    pos + vec2(s, 0.0),
                    pos + vec2(0.0, s),
                    pos + vec2(-s, 0.0),
                ];
                painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
            }
            ExecutionDomain::Timeline => {
                // Triangle
                let s = 5.0;
                let points = vec![
                    pos + vec2(0.0, -s),
                    pos + vec2(s, s),
                    pos + vec2(-s, s),
                ];
                painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
            }
            ExecutionDomain::Background => {
                painter.circle_stroke(pos, 5.0, Stroke::new(1.0, color));
            }
        }

        if response.drag_started() && !is_input {
            self.interaction.dragging_cable = Some(port_ref.clone());
        }

        if response.drag_stopped() && is_input {
            if let Some(src_port) = self.interaction.dragging_cable.take() {
                // Connect!
                self.interaction.pending_edges.insert((src_port.clone(), port_ref.clone()));
                self.action_tx.send(UserAction::Connect {
                    from: src_port.node_id.to_string(), // Need to resolve by ID for now or use node name
                    from_port: Some(src_port.port_name),
                    to: port_ref.node_id.to_string(),
                    to_port: Some(port_ref.port_name),
                }).ok();
            }
        }
    }

    fn draw_connections(&mut self, ui: &egui::Ui, graph: &Graph) {
        let painter = ui.painter();
        let zoom = self.layout.zoom;
        let pan = Vec2::from(self.layout.pan);

        // Clear pending edges that are now in the graph
        self.interaction.pending_edges.retain(|(src, tgt)| {
            !graph.edges.values().any(|e| e.source == *src && e.target == *tgt)
        });

        // 1. Draw Real Edges
        for edge in graph.edges.values() {
            if graph.nodes.contains_key(&edge.source.node_id) && graph.nodes.contains_key(&edge.target.node_id) {
                let src_pos = self.get_port_pos(edge.source.node_id, &edge.source.port_name, false) * zoom + pan;
                let tgt_pos = self.get_port_pos(edge.target.node_id, &edge.target.port_name, true) * zoom + pan;

                self.draw_bezier(painter, src_pos, tgt_pos, Color32::from_gray(150), false, zoom);
            }
        }

        // 2. Optimistic Rendering: Pending Edges
        for (src, tgt) in &self.interaction.pending_edges {
            let src_pos = self.get_port_pos(src.node_id, &src.port_name, false) * zoom + pan;
            let tgt_pos = self.get_port_pos(tgt.node_id, &tgt.port_name, true) * zoom + pan;
            self.draw_bezier(painter, src_pos, tgt_pos, Color32::YELLOW, true, zoom);
        }

        // 3. Dragging Cable
        if let Some(src) = &self.interaction.dragging_cable {
            if let Some(ptr_pos) = painter.ctx().pointer_interact_pos() {
                let src_pos = self.get_port_pos(src.node_id, &src.port_name, false) * zoom + pan;
                self.draw_bezier(painter, src_pos, ptr_pos, Color32::WHITE, true, zoom);
            }
        }
    }

    fn draw_bezier(&self, painter: &Painter, src: Pos2, tgt: Pos2, color: Color32, dashed: bool, zoom: f32) {
        let control_scale = (tgt.x - src.x).abs().max(20.0 * zoom) * 0.5;
        let cp1 = src + vec2(control_scale, 0.0);
        let cp2 = tgt - vec2(control_scale, 0.0);

        let stroke = Stroke::new(2.0 * zoom, color);

        // Simplified: just a line for now, or use cubic_bezier
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
        
        painter.add(egui::Shape::line(points, stroke));
    }

    fn get_port_pos(&self, node_id: StableId, _port_name: &str, is_input: bool) -> Pos2 {
        if let Some(visuals) = self.layout.nodes.get(&node_id) {
            let pos = Pos2::new(visuals.position[0], visuals.position[1]);
            // Simplified calculation for MVP
            if is_input {
                pos + vec2(0.0, 30.0)
            } else {
                pos + vec2(150.0, 30.0)
            }
        } else {
            Pos2::ZERO
        }
    }
    fn draw_intent_zones(&self, ui: &mut egui::Ui, _viewport: Rect) {
        let zoom = self.layout.zoom;
        let pan = Vec2::from(self.layout.pan);
        let painter = ui.ctx().layer_painter(egui::LayerId::background());

        for (id, zone_rect_raw) in &self.layout.intent_zones {
            let world_rect = Rect::from_min_max(
                Pos2::new(zone_rect_raw[0], zone_rect_raw[1]),
                Pos2::new(zone_rect_raw[2], zone_rect_raw[3]),
            );
            let screen_rect = Rect::from_min_max(
                world_rect.min * zoom + pan,
                world_rect.max * zoom + pan,
            );

            painter.rect_filled(screen_rect, 8.0 * zoom, Color32::from_rgba_unmultiplied(100, 100, 255, 30));
            painter.rect_stroke(screen_rect, 8.0 * zoom, Stroke::new(1.0 * zoom, Color32::from_rgba_unmultiplied(100, 100, 255, 100)));
            
            painter.text(
                screen_rect.left_top() + vec2(10.0, 10.0) * zoom,
                egui::Align2::LEFT_TOP,
                format!("Intent: {}", id),
                egui::FontId::proportional(12.0 * zoom),
                Color32::from_gray(180),
            );
        }
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
            ui.painter().rect_filled(viewport, 0.0, Color32::from_gray(15));

            // --- Layered Rendering ---
            
            // 1. Intent Zones (Background)
            self.draw_intent_zones(ui, viewport);

            // 2. Connections
            self.draw_connections(ui, &graph);

            // 3. Nodes
            let node_ids: Vec<StableId> = graph.nodes.keys().cloned().collect();
            for id in node_ids {
                if let Some(node) = graph.nodes.get(&id) {
                    self.draw_node(ui, id, node, viewport, &graph);
                }
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


