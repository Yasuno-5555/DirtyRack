//! Rack State & Rail Drawing
//!
//! ラックの状態管理とレール描画。
//!
//! # 決定論的創造環境の憲法
//! 1. すべてのアクションは `handle_action` を経由する。
//! 2. `Commit` インテントのみが永続ヒストリー (`event_queue`) に記録される。
//! 3. 浮動小数点は bitパターン (u32) でシリアライズする。
//! 4. ID はアトミックカウンターにより決して再利用されない。

use dirtyrack_modules::runner::{Connection, GraphSnapshot};
pub use dirtyrack_modules::{
    AllocationPolicy, IntentBoundary, IntentClass, IntentMetadata, ModuleDescriptor,
    ModuleRegistry, ModuleState, PatchEvent, PortDirection, ProvenanceZone, RackDspNode, SeedScope,
    SignalType,
};
use egui::{vec2, Color32, Painter, Pos2, Rect, Stroke, Vec2};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub const HP_PIXELS: f32 = 15.0;
pub const RACK_HEIGHT: f32 = 380.0;

pub struct ModuleInstance {
    pub descriptor: Arc<ModuleDescriptor>,
    pub params: BTreeMap<String, f32>,
    pub output_values: Vec<f32>,
    pub input_values: Vec<f32>,
    pub stable_id: u64,
    pub dsp: Box<dyn RackDspNode>,
    pub hp_position: f32,
    pub row: usize,
    pub bypassed: bool,
}

impl ModuleInstance {
    pub fn new(descriptor: Arc<ModuleDescriptor>, sample_rate: f32) -> Self {
        let mut params = BTreeMap::new();
        for p in &descriptor.params {
            params.insert(p.name.to_string(), p.default);
        }

        let in_count = descriptor
            .ports
            .iter()
            .filter(|p| p.direction == PortDirection::Input)
            .count();
        let out_count = descriptor
            .ports
            .iter()
            .filter(|p| p.direction == PortDirection::Output)
            .count();

        Self {
            descriptor: Arc::clone(&descriptor),
            params,
            output_values: vec![0.0; out_count],
            input_values: vec![0.0; in_count],
            stable_id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            dsp: (descriptor.factory)(sample_rate),
            hp_position: 0.0,
            row: 0,
            bypassed: false,
        }
    }

    pub fn world_rect(&self) -> Rect {
        let x = self.hp_position * HP_PIXELS;
        let y = self.row as f32 * (RACK_HEIGHT + 20.0);
        Rect::from_min_size(
            Pos2::new(x, y),
            vec2(self.descriptor.hp_width as f32 * HP_PIXELS, RACK_HEIGHT),
        )
    }
}

pub struct Cable {
    pub from_module: usize,
    pub from_port: String,
    pub to_module: usize,
    pub to_port: String,
    pub color: Color32,
    pub channels: u8,
}

pub enum CableAction {
    StartDrag {
        module_idx: usize,
        port_name: String,
        is_output: bool,
    },
    EndDrag {
        pointer_pos: Pos2,
    },
    StartModuleDrag {
        module_idx: usize,
        press_pos: Pos2,
    },
    MoveModule {
        module_idx: usize,
        pointer_pos: Pos2,
    },
    DisconnectPort {
        module_idx: usize,
        port_name: String,
    },
    ParamUpdate {
        module_idx: usize,
        name: String,
        value: f32,
        intent: IntentBoundary,
    },
    RemoveModule {
        module_idx: usize,
    },
    ToggleBypass {
        module_idx: usize,
    },
    RandomizeParams {
        module_idx: usize,
    },
    InspectForensics {
        stable_id: u64,
    },
    CancelDrag,
}

pub struct DraggingCable {
    pub from_module: usize,
    pub from_port: String,
    pub is_from_output: bool,
}

pub struct DraggingModule {
    pub module_idx: usize,
    pub offset: Vec2,
}

pub struct RackState {
    pub modules: Vec<ModuleInstance>,
    pub cables: Vec<Cable>,
    pub dragging_cable: Option<DraggingCable>,
    pub dragging_module: Option<DraggingModule>,
    pub sample_rate: f32,
    pub project_seed: u64,
    pub aging: f32,
    pub event_queue: Vec<PatchEvent>,
    pub schema_version: u32,
}

impl RackState {
    pub fn new() -> Self {
        Self {
            modules: Vec::new(),
            cables: Vec::new(),
            dragging_cable: None,
            dragging_module: None,
            sample_rate: 44100.0,
            project_seed: 0xDE7E_B11D,
            aging: 0.0,
            event_queue: Vec::new(),
            schema_version: 2,
        }
    }

    pub fn handle_action(&mut self, action: CableAction, zoom: f32, pan: Vec2) {
        match action {
            CableAction::StartDrag {
                module_idx,
                port_name,
                is_output,
            } => {
                self.dragging_cable = Some(DraggingCable {
                    from_module: module_idx,
                    from_port: port_name,
                    is_from_output: is_output,
                });
            }
            CableAction::EndDrag { pointer_pos } => {
                if let Some(drag) = self.dragging_cable.take() {
                    let world_pos = (pointer_pos.to_vec2() - pan) / zoom;
                    let world_pos = world_pos.to_pos2();

                    if let Some((to_mod, to_port, is_to_output)) = self.find_port_at(world_pos) {
                        if drag.is_from_output != is_to_output {
                            let (src_mod, src_port, dst_mod, dst_port) = if drag.is_from_output {
                                (drag.from_module, drag.from_port, to_mod, to_port)
                            } else {
                                (to_mod, to_port, drag.from_module, drag.from_port)
                            };

                            if src_mod != dst_mod {
                                let color = crate::cable::CABLE_COLORS
                                    [self.cables.len() % crate::cable::CABLE_COLORS.len()];
                                let channels = self.modules[src_mod]
                                    .descriptor
                                    .ports
                                    .iter()
                                    .find(|p| p.name == src_port)
                                    .map(|p| p.max_channels)
                                    .unwrap_or(1);

                                self.cables.push(Cable {
                                    from_module: src_mod,
                                    from_port: src_port.clone(),
                                    to_module: dst_mod,
                                    to_port: dst_port.clone(),
                                    color,
                                    channels,
                                });
                                self.event_queue.push(PatchEvent::CableConnected {
                                    from_id: self.modules[src_mod].stable_id,
                                    from_port: src_port,
                                    to_id: self.modules[dst_mod].stable_id,
                                    to_port: dst_port,
                                });
                            }
                        }
                    }
                }
            }
            CableAction::DisconnectPort {
                module_idx,
                port_name,
            } => {
                let stable_id = self.modules[module_idx].stable_id;
                self.cables.retain(|c| {
                    let match_from = c.from_module == module_idx && c.from_port == port_name;
                    let match_to = c.to_module == module_idx && c.to_port == port_name;
                    if match_from || match_to {
                        self.event_queue.push(PatchEvent::CableDisconnected {
                            to_id: self.modules[c.to_module].stable_id,
                            to_port: c.to_port.clone(),
                        });
                        false
                    } else {
                        true
                    }
                });
            }
            CableAction::ParamUpdate {
                module_idx,
                name,
                value,
                intent,
            } => {
                if let Some(m) = self.modules.get_mut(module_idx) {
                    m.params.insert(name.clone(), value);
                    if let IntentBoundary::Commit(_, _) = intent {
                        self.event_queue.push(PatchEvent::ParamChanged {
                            stable_id: m.stable_id,
                            name,
                            value_bits: value.to_bits(),
                            intent,
                        });
                    }
                }
            }
            CableAction::StartModuleDrag {
                module_idx,
                press_pos,
            } => {
                let module = &self.modules[module_idx];
                let world_rect = module.world_rect();
                let screen_pos = (world_rect.min.to_vec2() * zoom + pan).to_pos2();
                self.dragging_module = Some(DraggingModule {
                    module_idx,
                    offset: screen_pos - press_pos,
                });
            }
            CableAction::MoveModule {
                module_idx: _,
                pointer_pos,
            } => {
                if let Some(drag) = &self.dragging_module {
                    let target_world_pos = (pointer_pos + drag.offset - pan) / zoom;
                    let hp_x = (target_world_pos.x / HP_PIXELS).round();
                    self.modules[drag.module_idx].hp_position = hp_x;
                }
            }
            CableAction::RemoveModule { module_idx } => {
                self.remove_module(module_idx);
            }
            CableAction::ToggleBypass { module_idx } => {
                if let Some(m) = self.modules.get_mut(module_idx) {
                    m.bypassed = !m.bypassed;
                    self.event_queue.push(PatchEvent::ParamChanged {
                        stable_id: m.stable_id,
                        name: "bypassed".to_string(),
                        value_bits: if m.bypassed { 1 } else { 0 },
                        intent: IntentBoundary::Commit(IntentClass::Structural, None),
                    });
                }
            }
            CableAction::RandomizeParams { module_idx } => {
                if let Some(m) = self.modules.get_mut(module_idx) {
                    let seed = m.stable_id; // Simple seed based on ID
                    for (i, p) in m.descriptor.params.iter().enumerate() {
                        // Deterministic "random" value between min and max
                        let hash = (seed.wrapping_mul(0x517cc1b727220a95).wrapping_add(i as u64))
                            as f32
                            / u64::MAX as f32;
                        let val = p.min + hash * (p.max - p.min);
                        m.params.insert(p.name.to_string(), val);
                        self.event_queue.push(PatchEvent::ParamChanged {
                            stable_id: m.stable_id,
                            name: p.name.to_string(),
                            value_bits: val.to_bits(),
                            intent: IntentBoundary::Commit(IntentClass::Performance, None),
                        });
                    }
                }
            }
            CableAction::InspectForensics { .. } => {}
            CableAction::CancelDrag => {
                self.dragging_cable = None;
                self.dragging_module = None;
            }
        }
    }

    pub fn find_port_at(&self, pos: Pos2) -> Option<(usize, String, bool)> {
        for (m_idx, module) in self.modules.iter().enumerate() {
            let rect = module.world_rect();
            if rect.contains(pos) {
                for port in &module.descriptor.ports {
                    let p_pos = Pos2::new(
                        rect.left() + port.position[0] * rect.width(),
                        rect.top() + port.position[1] * rect.height(),
                    );
                    if p_pos.distance(pos) < 15.0 {
                        return Some((
                            m_idx,
                            port.name.to_string(),
                            port.direction == PortDirection::Output,
                        ));
                    }
                }
            }
        }
        None
    }

    pub fn port_world_pos(&self, module_idx: usize, port_name: &str) -> Option<Pos2> {
        let module = self.modules.get(module_idx)?;
        let port = module
            .descriptor
            .ports
            .iter()
            .find(|p| p.name == port_name)?;
        let rect = module.world_rect();
        Some(Pos2::new(
            rect.left() + port.position[0] * rect.width(),
            rect.top() + port.position[1] * rect.height(),
        ))
    }

    pub fn add_module(&mut self, descriptor: Arc<ModuleDescriptor>) {
        let mut inst = ModuleInstance::new(Arc::clone(&descriptor), self.sample_rate);
        let mut next_hp: f32 = 0.0;
        for m in &self.modules {
            let end = m.hp_position + m.descriptor.hp_width as f32;
            if end > next_hp {
                next_hp = end;
            }
        }
        inst.hp_position = next_hp;
        self.event_queue.push(PatchEvent::ModuleAdded {
            id: descriptor.id.to_string(),
            stable_id: inst.stable_id,
            ancestry: None,
            zone: descriptor.zone,
        });
        self.modules.push(inst);
    }

    pub fn remove_module(&mut self, idx: usize) {
        if idx >= self.modules.len() {
            return;
        }
        let stable_id = self.modules[idx].stable_id;
        self.event_queue
            .push(PatchEvent::ModuleRemoved { stable_id });
        self.cables.retain(|c| {
            if c.from_module == idx || c.to_module == idx {
                self.event_queue.push(PatchEvent::CableDisconnected {
                    to_id: self.modules[c.to_module].stable_id,
                    to_port: c.to_port.clone(),
                });
                false
            } else {
                true
            }
        });
        for cable in &mut self.cables {
            if cable.from_module > idx {
                cable.from_module -= 1;
            }
            if cable.to_module > idx {
                cable.to_module -= 1;
            }
        }
        self.modules.remove(idx);
    }

    pub fn build_snapshot(&self) -> (GraphSnapshot, Vec<Box<dyn RackDspNode>>) {
        let n = self.modules.len();
        let mut order = Vec::with_capacity(n);
        let mut visited = vec![false; n];
        let mut visiting = vec![false; n];

        fn dfs(
            idx: usize,
            modules: &[ModuleInstance],
            cables: &[Cable],
            visited: &mut [bool],
            visiting: &mut [bool],
            order: &mut Vec<usize>,
        ) {
            if visited[idx] {
                return;
            }
            visiting[idx] = true;
            for cable in cables {
                if cable.to_module == idx {
                    let from = cable.from_module;
                    if !visited[from] && !visiting[from] {
                        dfs(from, modules, cables, visited, visiting, order);
                    }
                }
            }
            visiting[idx] = false;
            visited[idx] = true;
            order.push(idx);
        }

        for i in 0..n {
            dfs(
                i,
                &self.modules,
                &self.cables,
                &mut visited,
                &mut visiting,
                &mut order,
            );
        }

        let mut node_ids = Vec::with_capacity(n);
        let mut new_nodes = Vec::with_capacity(n);
        for &idx in &order {
            let m = &self.modules[idx];
            node_ids.push(m.stable_id);
            // In a real implementation, we might need to recreate nodes from state,
            // but for now we'll just use a factory if we don't want to move out.
            // Actually, let's just create a placeholder or assume we move them later.
            // For DirtyRack, we often recreate nodes from extract_state/inject_state.
            new_nodes.push((m.descriptor.factory)(self.sample_rate));
        }

        let port_counts = order
            .iter()
            .map(|&idx| {
                let m = &self.modules[idx];
                let ins = m
                    .descriptor
                    .ports
                    .iter()
                    .filter(|p| p.direction == PortDirection::Input)
                    .count();
                let outs = m
                    .descriptor
                    .ports
                    .iter()
                    .filter(|p| p.direction == PortDirection::Output)
                    .count();
                (ins, outs)
            })
            .collect();

        let mut index_map = vec![0; n];
        for (new_idx, &old_idx) in order.iter().enumerate() {
            index_map[old_idx] = new_idx;
        }

        let mut connections = Vec::new();
        for cable in &self.cables {
            let from_new = index_map[cable.from_module];
            let to_new = index_map[cable.to_module];

            let from_port_idx = self.modules[cable.from_module]
                .descriptor
                .ports
                .iter()
                .filter(|p| p.direction == PortDirection::Output)
                .position(|p| p.name == cable.from_port)
                .unwrap_or(0);
            let to_port_idx = self.modules[cable.to_module]
                .descriptor
                .ports
                .iter()
                .filter(|p| p.direction == PortDirection::Input)
                .position(|p| p.name == cable.to_port)
                .unwrap_or(0);

            connections.push(Connection {
                from_module: from_new,
                from_port: from_port_idx,
                to_module: to_new,
                to_port: to_port_idx,
            });
        }

        (
            GraphSnapshot {
                order: (0..n).collect(),
                connections,
                port_counts,
                node_ids,
            },
            new_nodes,
        )
    }
}

pub fn draw_rack_rails(painter: &Painter, viewport: Rect, zoom: f32, pan: Vec2) {
    let rail_color = Color32::from_rgb(60, 55, 50);
    let screw_color = Color32::from_rgb(120, 115, 100);
    let rail_h = 12.0 * zoom;
    for row in 0..1 {
        let base_y = row as f32 * (RACK_HEIGHT + 20.0) * zoom + pan.y;
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(viewport.left(), base_y),
                vec2(viewport.width(), rail_h),
            ),
            0.0,
            rail_color,
        );
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(viewport.left(), base_y + RACK_HEIGHT * zoom - rail_h),
                vec2(viewport.width(), rail_h),
            ),
            0.0,
            rail_color,
        );
        let screw_spacing = 10.0 * HP_PIXELS * zoom;
        let mut x = pan.x % screw_spacing;
        while x < viewport.width() {
            for rail_y in [
                base_y + rail_h * 0.5,
                base_y + RACK_HEIGHT * zoom - rail_h * 0.5,
            ] {
                painter.circle_filled(Pos2::new(x, rail_y), 3.0 * zoom, screw_color);
                painter.circle_stroke(
                    Pos2::new(x, rail_y),
                    3.0 * zoom,
                    Stroke::new(0.5, Color32::from_gray(80)),
                );
            }
            x += screw_spacing;
        }
    }
}
