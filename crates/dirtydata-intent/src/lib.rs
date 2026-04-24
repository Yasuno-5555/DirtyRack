//! Intent Engine — 意味の構造化
//!
//! DirtyData において、パッチは単なる状態変更の羅列ではない。
//! Intent（意図）という上位概念があり、パッチは「それを実現するための Strategy の結果」である。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use dirtydata_core::types::*;

pub type IntentId = ulid::Ulid;

/// Intent の状態。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentStatus {
    /// 提案。まだ何も適用されていない。
    Proposal,
    /// 適用中。一部のパッチが紐付けられた。
    Attached,
    /// 完了・固定された。
    Resolved,
    /// 棄却された。
    Discarded,
}

/// Intent 本体。何を実現したいか。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentNode {
    pub id: IntentId,
    pub description: String,
    pub constraints: Vec<IntentConstraint>,
    pub status: IntentStatus,
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
        self.intents.insert(id, IntentNode {
            id,
            description,
            constraints,
            status: IntentStatus::Proposal,
            attached_patches: Vec::new(),
        });
        id
    }

    pub fn attach(&mut self, id: IntentId, patch_id: PatchId) -> Result<(), String> {
        let intent = self.intents.get_mut(&id).ok_or_else(|| format!("Intent {} not found", id))?;
        if !intent.attached_patches.contains(&patch_id) {
            intent.attached_patches.push(patch_id);
        }
        if intent.status == IntentStatus::Proposal {
            intent.status = IntentStatus::Attached;
        }
        Ok(())
    }
}
