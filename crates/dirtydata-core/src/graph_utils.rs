use crate::ir::Graph;
use crate::types::StableId;
use std::collections::{HashMap, HashSet, VecDeque};

/// Sorts the graph nodes topologically.
/// If cycles are detected, they are returned as well.
pub fn topological_sort(graph: &Graph) -> (Vec<StableId>, Vec<Vec<StableId>>) {
    let mut in_degree = HashMap::new();
    let mut adj = HashMap::new();
    let mut all_nodes = HashSet::new();

    for id in graph.nodes.keys() {
        all_nodes.insert(*id);
        in_degree.insert(*id, 0);
        adj.insert(*id, Vec::new());
    }

    for edge in graph.edges.values() {
        adj.get_mut(&edge.source.node_id).unwrap().push(edge.target.node_id);
        *in_degree.get_mut(&edge.target.node_id).unwrap() += 1;
    }

    let mut queue = VecDeque::new();
    for (id, degree) in &in_degree {
        if *degree == 0 {
            queue.push_back(*id);
        }
    }

    let mut sorted = Vec::new();
    while let Some(u) = queue.pop_front() {
        sorted.push(u);
        if let Some(neighbors) = adj.get(&u) {
            for &v in neighbors {
                let degree = in_degree.get_mut(&v).unwrap();
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(v);
                }
            }
        }
    }

    // Detect cycles
    let mut cycles = Vec::new();
    if sorted.len() < all_nodes.len() {
        // Simple cycle detection: nodes with remaining in-degree are part of cycles
        let remaining: HashSet<_> = all_nodes.into_iter().filter(|id| !sorted.contains(id)).collect();
        // For MVP, we just return them as a single group of "cyclic nodes"
        // In a real system, we'd use Tarjan's or similar to find SCCs.
        cycles.push(remaining.into_iter().collect());
    }

    (sorted, cycles)
}
