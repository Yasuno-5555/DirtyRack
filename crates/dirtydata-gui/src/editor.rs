use egui::{vec2, Color32, Painter, Pos2, Rect, Stroke, Vec2, Rounding, Shadow};
use dirtydata_core::ir::{Graph, Node};
use dirtydata_core::types::{StableId, PortRef, PortDirection, ExecutionDomain};
use dirtydata_runtime::SharedState;
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
    ) {
        let viewport = ui.max_rect();
        let painter = ui.painter();
        let zoom = layout.zoom;
        let pan = Vec2::from(layout.pan);

        // 1. Grid (Background)
        self.draw_grid(painter, viewport, zoom, pan);

        // 2. Connections
        self.draw_connections(painter, graph, layout, interaction, zoom, pan);

        // 3. Nodes
        let node_ids: Vec<StableId> = graph.nodes.keys().cloned().collect();
        for id in node_ids {
            if let Some(node) = graph.nodes.get(&id) {
                self.draw_node(ui, id, node, layout, interaction, shared_state);
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

        let response = ui.interact(rect, ui.make_persistent_id(id), egui::Sense::drag());
        
        if response.dragged() {
            let delta = response.drag_delta() / zoom;
            visuals.position[0] += delta.x;
            visuals.position[1] += delta.y;
        }

        let painter = ui.painter();

        // Glassmorphism Body
        let bg_color = Color32::from_rgba_unmultiplied(40, 40, 50, 200);
        painter.rect_filled(rect, 8.0 * zoom, bg_color);
        
        // Neon Glow Border
        let border_color = if self.selection.contains(&id) {
            Color32::from_rgb(255, 200, 0)
        } else {
            Color32::from_rgba_unmultiplied(100, 100, 255, 100)
        };
        painter.rect_stroke(rect, 8.0 * zoom, Stroke::new(1.5 * zoom, border_color));

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
    ) {
        for edge in graph.edges.values() {
            if let (Some(src_v), Some(tgt_v)) = (layout.nodes.get(&edge.source.node_id), layout.nodes.get(&edge.target.node_id)) {
                let src_pos = (Pos2::new(src_v.position[0] + 160.0, src_v.position[1] + 35.0)) * zoom + pan;
                let tgt_pos = (Pos2::new(tgt_v.position[0], tgt_v.position[1] + 35.0)) * zoom + pan;
                self.draw_bezier(painter, src_pos, tgt_pos, Color32::from_rgba_unmultiplied(100, 100, 255, 150), zoom);
            }
        }

        if let Some(src) = &interaction.dragging_cable {
            if let Some(src_v) = layout.nodes.get(&src.node_id) {
                let src_pos = (Pos2::new(src_v.position[0] + 160.0, src_v.position[1] + 35.0)) * zoom + pan;
                if let Some(ptr_pos) = painter.ctx().pointer_interact_pos() {
                    self.draw_bezier(painter, src_pos, ptr_pos, Color32::WHITE, zoom);
                }
            }
        }
    }

    fn draw_bezier(&self, painter: &Painter, src: Pos2, tgt: Pos2, color: Color32, zoom: f32) {
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
        
        painter.add(egui::Shape::line(points, Stroke::new(2.0 * zoom, color)));
    }
}
