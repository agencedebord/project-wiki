use std::collections::{HashMap, HashSet, VecDeque};

/// Perform transitive reduction on a directed graph.
///
/// For each edge (u, v), check if there exists a path from u to v of length ≥ 2.
/// If so, the direct edge is redundant and is removed.
///
/// Handles cycles correctly by tracking visited nodes during BFS.
/// Time complexity: O(V × E) — trivially fast for typical domain graphs (~20 nodes).
pub fn transitive_reduce(adj: &mut HashMap<String, HashSet<String>>) {
    let nodes: Vec<String> = adj.keys().cloned().collect();

    for u in &nodes {
        let neighbors: Vec<String> = adj
            .get(u)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();

        for v in &neighbors {
            // Check if v is reachable from u via a path of length ≥ 2
            // using the CURRENT state of the graph (not the original)
            if is_reachable_without_direct(adj, u, v) {
                if let Some(set) = adj.get_mut(u) {
                    set.remove(v);
                }
            }
        }
    }
}

/// Check if `target` is reachable from `source` without using the direct edge source→target.
/// Uses BFS starting from all neighbors of `source` except `target`.
fn is_reachable_without_direct(
    adj: &HashMap<String, HashSet<String>>,
    source: &str,
    target: &str,
) -> bool {
    let Some(neighbors) = adj.get(source) else {
        return false;
    };

    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();

    // Mark source as visited to prevent cycles from looping back through it
    visited.insert(source);

    // Start BFS from all neighbors of source except target
    for neighbor in neighbors {
        if neighbor != target && visited.insert(neighbor.as_str()) {
            queue.push_back(neighbor.as_str());
        }
    }

    while let Some(node) = queue.pop_front() {
        if node == target {
            return true;
        }
        if let Some(next_neighbors) = adj.get(node) {
            for next in next_neighbors {
                if visited.insert(next.as_str()) {
                    queue.push_back(next.as_str());
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_graph(edges: &[(&str, &str)]) -> HashMap<String, HashSet<String>> {
        let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
        for (u, v) in edges {
            adj.entry(u.to_string()).or_default().insert(v.to_string());
            // Ensure target node exists in the map
            adj.entry(v.to_string()).or_default();
        }
        adj
    }

    fn edge_count(adj: &HashMap<String, HashSet<String>>) -> usize {
        adj.values().map(|s| s.len()).sum()
    }

    #[test]
    fn diamond_graph_removes_transitive_edge() {
        // A→B, A→C, B→C → A→C is redundant (A→B→C exists)
        let mut g = make_graph(&[("A", "B"), ("A", "C"), ("B", "C")]);
        assert_eq!(edge_count(&g), 3);

        transitive_reduce(&mut g);

        assert!(g["A"].contains("B"), "A→B should remain");
        assert!(g["B"].contains("C"), "B→C should remain");
        assert!(
            !g["A"].contains("C"),
            "A→C should be removed (transitive via B)"
        );
        assert_eq!(edge_count(&g), 2);
    }

    #[test]
    fn cycle_with_shared_target_reduces() {
        // A→B, B→A (cycle), A→C, B→C
        // Both A→C and B→C are individually redundant (via the other + cycle).
        // The reduction removes exactly one of them (order-dependent).
        // The cycle edges A→B and B→A must always remain.
        let mut g = make_graph(&[("A", "B"), ("B", "A"), ("A", "C"), ("B", "C")]);
        transitive_reduce(&mut g);

        assert!(g["A"].contains("B"), "A→B should remain");
        assert!(g["B"].contains("A"), "B→A should remain (cycle)");
        // Exactly one of A→C or B→C should be removed
        let a_to_c = g.get("A").map(|s| s.contains("C")).unwrap_or(false);
        let b_to_c = g.get("B").map(|s| s.contains("C")).unwrap_or(false);
        assert!(a_to_c || b_to_c, "At least one path to C must remain");
        assert_eq!(edge_count(&g), 3, "One edge should be removed");
    }

    #[test]
    fn disconnected_components_unchanged() {
        // Two disconnected pairs: A→B, C→D
        let mut g = make_graph(&[("A", "B"), ("C", "D")]);
        transitive_reduce(&mut g);

        assert_eq!(edge_count(&g), 2);
        assert!(g["A"].contains("B"));
        assert!(g["C"].contains("D"));
    }

    #[test]
    fn empty_graph_noop() {
        let mut g: HashMap<String, HashSet<String>> = HashMap::new();
        transitive_reduce(&mut g);
        assert!(g.is_empty());
    }

    #[test]
    fn chain_no_reduction() {
        // A→B→C — no transitive edges to remove
        let mut g = make_graph(&[("A", "B"), ("B", "C")]);
        transitive_reduce(&mut g);
        assert_eq!(edge_count(&g), 2);
    }

    #[test]
    fn longer_transitive_path() {
        // A→B, B→C, C→D, A→D → A→D is redundant (A→B→C→D)
        let mut g = make_graph(&[("A", "B"), ("B", "C"), ("C", "D"), ("A", "D")]);
        transitive_reduce(&mut g);

        assert!(!g["A"].contains("D"), "A→D should be removed");
        assert_eq!(edge_count(&g), 3);
    }
}
