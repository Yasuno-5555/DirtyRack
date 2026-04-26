//! Canonical IR — Layer 1: Machine Truth.
//!
//! The single Source of Truth.
//! Git manages it. The compiler interprets it. The runtime depends on it.
//!
//! GUI や DSL による直接上書きを禁止。
//! すべての変更は PatchSet を経由して適用される。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::types::*;

// ──────────────────────────────────────────────
// §5.1 — Node
// ──────────────────────────────────────────────

/// A node in the Canonical IR graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: StableId,
    pub kind: NodeKind,
    pub ports: Vec<TypedPort>,
    pub config: ConfigSnapshot,
    pub metadata: MetadataRef,
    pub confidence: ConfidenceScore,
}

impl Node {
    /// Create a minimal node with standard audio I/O ports.
    pub fn new_processor(name: &str) -> Self {
        Self {
            id: StableId::new(),
            kind: NodeKind::Processor,
            ports: vec![
                TypedPort {
                    name: "in".into(),
                    direction: PortDirection::Input,
                    domain: ExecutionDomain::Sample,
                    data_type: DataType::Audio { channels: 2 },
                },
                TypedPort {
                    name: "out".into(),
                    direction: PortDirection::Output,
                    domain: ExecutionDomain::Sample,
                    data_type: DataType::Audio { channels: 2 },
                },
            ],
            config: {
                let mut c = BTreeMap::new();
                c.insert("name".into(), ConfigValue::String(name.into()));
                c
            },
            metadata: MetadataRef(None),
            confidence: ConfidenceScore::Verified,
        }
    }

    /// Create a source node (audio file, input device).
    pub fn new_source(name: &str) -> Self {
        Self {
            id: StableId::new(),
            kind: NodeKind::Source,
            ports: vec![TypedPort {
                name: "out".into(),
                direction: PortDirection::Output,
                domain: ExecutionDomain::Sample,
                data_type: DataType::Audio { channels: 2 },
            }],
            config: {
                let mut c = BTreeMap::new();
                c.insert("name".into(), ConfigValue::String(name.into()));
                c
            },
            metadata: MetadataRef(None),
            confidence: ConfidenceScore::Verified,
        }
    }

    /// Create a sink node (output, export target).
    pub fn new_sink(name: &str) -> Self {
        Self {
            id: StableId::new(),
            kind: NodeKind::Sink,
            ports: vec![TypedPort {
                name: "in".into(),
                direction: PortDirection::Input,
                domain: ExecutionDomain::Sample,
                data_type: DataType::Audio { channels: 2 },
            }],
            config: {
                let mut c = BTreeMap::new();
                c.insert("name".into(), ConfigValue::String(name.into()));
                c
            },
            metadata: MetadataRef(None),
            confidence: ConfidenceScore::Verified,
        }
    }

    pub fn new_subgraph(name: &str) -> Self {
        Self {
            id: StableId::new(),
            kind: NodeKind::SubGraph,
            ports: vec![
                TypedPort {
                    name: "in".into(),
                    direction: PortDirection::Input,
                    domain: ExecutionDomain::Sample,
                    data_type: DataType::Audio { channels: 2 },
                },
                TypedPort {
                    name: "out".into(),
                    direction: PortDirection::Output,
                    domain: ExecutionDomain::Sample,
                    data_type: DataType::Audio { channels: 2 },
                },
            ],
            config: {
                let mut c = BTreeMap::new();
                c.insert("name".into(), ConfigValue::String(name.into()));
                c.insert("graph_json".into(), ConfigValue::String("{}".into()));
                c
            },
            metadata: MetadataRef(None),
            confidence: ConfidenceScore::Verified,
        }
    }

    pub fn new_input_proxy(name: &str) -> Self {
        Self {
            id: StableId::new(),
            kind: NodeKind::InputProxy,
            ports: vec![TypedPort {
                name: "out".into(),
                direction: PortDirection::Output,
                domain: ExecutionDomain::Sample,
                data_type: DataType::Audio { channels: 2 },
            }],
            config: {
                let mut c = BTreeMap::new();
                c.insert("name".into(), ConfigValue::String(name.into()));
                c
            },
            metadata: MetadataRef(None),
            confidence: ConfidenceScore::Verified,
        }
    }

    pub fn new_output_proxy(name: &str) -> Self {
        Self {
            id: StableId::new(),
            kind: NodeKind::OutputProxy,
            ports: vec![TypedPort {
                name: "in".into(),
                direction: PortDirection::Input,
                domain: ExecutionDomain::Sample,
                data_type: DataType::Audio { channels: 2 },
            }],
            config: {
                let mut c = BTreeMap::new();
                c.insert("name".into(), ConfigValue::String(name.into()));
                c
            },
            metadata: MetadataRef(None),
            confidence: ConfidenceScore::Verified,
        }
    }
}

// ──────────────────────────────────────────────
// §5.2 — Edge
// ──────────────────────────────────────────────

/// The type of connection between ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    /// Normal feed-forward connection (causal dependency).
    Normal,
    /// Feedback connection (1-sample delay, breaks DAG constraint).
    Feedback,
}

/// An edge connecting two ports in the Canonical IR graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: StableId,
    pub source: PortRef,
    pub target: PortRef,
    pub kind: EdgeKind,
}

impl Edge {
    /// Create a causal edge between two ports.
    pub fn new(source: PortRef, target: PortRef) -> Self {
        Self {
            id: StableId::new(),
            source,
            target,
            kind: EdgeKind::Normal,
        }
    }

    /// Create a feedback edge (1-sample delay).
    pub fn new_feedback(source: PortRef, target: PortRef) -> Self {
        Self {
            id: StableId::new(),
            source,
            target,
            kind: EdgeKind::Feedback,
        }
    }
}

// ──────────────────────────────────────────────
// §5.3 — Modulation
// ──────────────────────────────────────────────

/// A modulation assignment between a source and a parameter.
/// This is "cable-less" modulation — Bitwig style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Modulation {
    pub id: StableId,
    pub source: PortRef,
    pub target_node: StableId,
    pub target_param: String,
    pub amount: f32,
}

impl Modulation {
    pub fn new(source: PortRef, target_node: StableId, target_param: String, amount: f32) -> Self {
        Self {
            id: StableId::new(),
            source,
            target_node,
            target_param,
            amount,
        }
    }
}

// ──────────────────────────────────────────────
// Canonical IR Graph
// ──────────────────────────────────────────────

/// The Canonical IR Graph — the single Source of Truth.
///
/// State alone is not enough. History is required.
/// "stateだけ持つな。historyを持て。"
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: BTreeMap<StableId, Node>,
    pub edges: BTreeMap<StableId, Edge>,
    pub modulations: BTreeMap<StableId, Modulation>,
    pub revision: Revision,
    /// Ordered history of applied patches — explainability.
    /// "今この状態は何からできたか" が常に答えられること。
    pub applied_patches: Vec<PatchId>,
}

impl Graph {
    /// Create an empty graph at revision zero.
    pub fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            modulations: BTreeMap::new(),
            revision: Revision::zero(),
            applied_patches: Vec::new(),
        }
    }

    /// Get a node by ID.
    pub fn node(&self, id: &StableId) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Get an edge by ID.
    pub fn edge(&self, id: &StableId) -> Option<&Edge> {
        self.edges.get(id)
    }

    /// Check if a port reference is valid.
    pub fn validate_port_ref(&self, port_ref: &PortRef) -> bool {
        self.nodes
            .get(&port_ref.node_id)
            .map(|n| n.ports.iter().any(|p| p.name == port_ref.port_name))
            .unwrap_or(false)
    }
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}
