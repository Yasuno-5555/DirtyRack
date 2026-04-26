use crate::patch::{Operation, Patch, PatchSet};
use crate::types::{ConfigDelta, StableId};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("Conflict on node {node_id}: key '{key}' modified by both sides")]
    ConfigConflict { node_id: StableId, key: String },
    #[error("Conflict on edge {edge_id}: both sides modified this edge")]
    EdgeConflict { edge_id: StableId },
    #[error("Conflict on node {node_id}: one side removed, other side modified")]
    RemoveModifyConflict { node_id: StableId },
}

pub fn merge_three_way(
    _base: &PatchSet,
    left: &PatchSet,
    right: &PatchSet,
) -> Result<PatchSet, MergeError> {
    let mut merged_ops = Vec::new();

    // Track what each side is doing
    let left_ops = collect_op_targets(&left.patches);
    let right_ops = collect_op_targets(&right.patches);

    // Iterate through all modified nodes
    let all_nodes: HashSet<_> = left_ops
        .nodes
        .keys()
        .chain(right_ops.nodes.keys())
        .cloned()
        .collect();

    for node_id in all_nodes {
        let l_mod = left_ops.nodes.get(&node_id);
        let r_mod = right_ops.nodes.get(&node_id);

        match (l_mod, r_mod) {
            (Some(l), Some(r)) => {
                // Both modified the same node. Check for field conflicts.
                if l.removed || r.removed {
                    return Err(MergeError::RemoveModifyConflict { node_id });
                }

                let mut merged_delta = l.config_delta.clone();
                for (key, r_change) in &r.config_delta {
                    if let Some(l_change) = l.config_delta.get(key) {
                        if l_change != r_change {
                            return Err(MergeError::ConfigConflict {
                                node_id,
                                key: key.clone(),
                            });
                        }
                    } else {
                        merged_delta.insert(key.clone(), r_change.clone());
                    }
                }

                if !merged_delta.is_empty() {
                    merged_ops.push(Operation::ModifyConfig {
                        node_id,
                        delta: merged_delta,
                    });
                }
            }
            (Some(l), None) => {
                // Only left modified
                if l.removed {
                    merged_ops.push(Operation::RemoveNode(node_id));
                } else if !l.config_delta.is_empty() {
                    merged_ops.push(Operation::ModifyConfig {
                        node_id,
                        delta: l.config_delta.clone(),
                    });
                }
            }
            (None, Some(r)) => {
                // Only right modified
                if r.removed {
                    merged_ops.push(Operation::RemoveNode(node_id));
                } else if !r.config_delta.is_empty() {
                    merged_ops.push(Operation::ModifyConfig {
                        node_id,
                        delta: r.config_delta.clone(),
                    });
                }
            }
            _ => unreachable!(),
        }
    }

    Ok(PatchSet {
        patches: vec![Patch::from_operations_with_provenance(
            merged_ops,
            crate::types::PatchSource::System,
            crate::types::TrustLevel::Trusted,
        )],
    })
}

struct OpTargets {
    nodes: HashMap<StableId, NodeOpSummary>,
}

struct NodeOpSummary {
    removed: bool,
    config_delta: ConfigDelta,
}

fn collect_op_targets(patches: &[Patch]) -> OpTargets {
    let mut nodes = HashMap::new();

    for patch in patches {
        for op in &patch.operations {
            match op {
                Operation::RemoveNode(id) => {
                    nodes
                        .entry(*id)
                        .or_insert(NodeOpSummary {
                            removed: true,
                            config_delta: BTreeMap::new(),
                        })
                        .removed = true;
                }
                Operation::ModifyConfig { node_id, delta } => {
                    let summary = nodes.entry(*node_id).or_insert(NodeOpSummary {
                        removed: false,
                        config_delta: BTreeMap::new(),
                    });
                    for (k, v) in delta {
                        summary.config_delta.insert(k.clone(), v.clone());
                    }
                }
                _ => {}
            }
        }
    }

    OpTargets { nodes }
}
