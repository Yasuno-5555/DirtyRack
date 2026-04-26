//! Intent Engine — 意味の構造化
//!
//! DirtyData において、パッチは単なる状態変更の羅列ではない。
//! Intent（意図）という上位概念があり、パッチは「それを実現するための Strategy の結果」である。

pub mod attribution;

pub use attribution::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use dirtydata_core::types::*;

/// Intent の実現方法。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "strategy", rename_all = "snake_case")]
pub enum IntentStrategy {
    /// 手動。ユーザーがパッチを適用して紐付ける。
    Manual,
    /// 自動。特定のノードを挿入する。
    InsertNode {
        kind: NodeKind,
        name: String,
        config: ConfigSnapshot,
    },
    /// 自動。既存のノードを接続する。
    Bridge { from_node: String, to_node: String },
    /// 自動。安全な Frozen Asset に置換する。
    Freeze { target_node: String },
}

impl Default for IntentStrategy {
    fn default() -> Self {
        Self::Manual
    }
}

/// Intent 本体。何を実現したいか。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentNode {
    pub id: IntentId,
    pub description: String,
    pub constraints: Vec<IntentConstraint>,
    pub status: IntentStatus,
    pub strategy: IntentStrategy,
    pub attached_patches: Vec<PatchId>,
}

/// IntentEngine の永続状態。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntentState {
    pub intents: HashMap<IntentId, IntentNode>,
}

impl IntentState {
    pub fn save(&self, project_root: &Path) -> Result<(), std::io::Error> {
        let path = project_root.join(".dirtydata").join("intents.json");
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)
    }

    pub fn load(project_root: &Path) -> Result<Self, std::io::Error> {
        let path = project_root.join(".dirtydata").join("intents.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(path)?;
        let state = serde_json::from_str(&data)?;
        Ok(state)
    }

    pub fn add(&mut self, description: String, constraints: Vec<IntentConstraint>) -> IntentId {
        let id = IntentId::new();
        self.intents.insert(
            id,
            IntentNode {
                id,
                description,
                constraints,
                status: IntentStatus::Proposal,
                strategy: IntentStrategy::Manual,
                attached_patches: Vec::new(),
            },
        );
        id
    }

    pub fn attach(&mut self, id: IntentId, patch_id: PatchId) -> Result<(), String> {
        let intent = self
            .intents
            .get_mut(&id)
            .ok_or_else(|| format!("Intent {} not found", id))?;
        if !intent.attached_patches.contains(&patch_id) {
            intent.attached_patches.push(patch_id);
        }
        if intent.status == IntentStatus::Proposal {
            intent.status = IntentStatus::Attached;
        }
        Ok(())
    }

    /// 制約を評価し、充足状況を返す
    pub fn evaluate_constraints(
        &self,
        id: IntentId,
        graph: &dirtydata_core::ir::Graph,
    ) -> Vec<String> {
        let intent = match self.intents.get(&id) {
            Some(i) => i,
            None => return vec![format!("Intent {} not found", id)],
        };
        let mut violations = Vec::new();

        for constraint in &intent.constraints {
            match constraint {
                IntentConstraint::Must(desc) if desc.to_lowercase().contains("clip") => {
                    // Check if all sinks have a ClipNode as input
                    for node in graph.nodes.values() {
                        if node.kind == NodeKind::Sink {
                            let mut has_clip = false;
                            for edge in graph.edges.values() {
                                if edge.target.node_id == node.id {
                                    if let Some(src_node) = graph.nodes.get(&edge.source.node_id) {
                                        let name = dirtydata_core::actions::node_name(src_node);
                                        if name.to_lowercase().contains("clip") {
                                            has_clip = true;
                                            break;
                                        }
                                    }
                                }
                            }
                            if !has_clip {
                                violations.push(format!("Constraint Violation [Must: {}]: Output '{}' must be protected by a Clip node.", desc, dirtydata_core::actions::node_name(node)));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        violations
    }
}
