use std::collections::{HashMap, HashSet, VecDeque};

/// Perform transitive reduction on a directed graph.
///
/// For each edge (u, v), check if there exists a path from u to v of length ≥ 2.
/// If so, the direct edge is redundant and is removed.
///
/// Handles cycles correctly by tracking visited nodes during BFS.
/// Deterministic: nodes and neighbors are sorted before iteration, so the same
/// logical graph always produces the same reduction regardless of HashMap ordering.
/// Time complexity: O(V × E) — trivially fast for typical domain graphs (~20 nodes).
pub fn transitive_reduce(adj: &mut HashMap<String, HashSet<String>>) {
    let mut nodes: Vec<String> = adj.keys().cloned().collect();
    nodes.sort();

    for u in &nodes {
        let mut neighbors: Vec<String> = adj
            .get(u)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        neighbors.sort();

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
    fn cycle_with_shared_target_reduces_deterministically() {
        // A→B, B→A (cycle), A→C, B→C
        // With sorted iteration (A before B), A→C is processed first and
        // found redundant via A→B→C. Then B→C is no longer redundant
        // (A→C was already removed). Result is deterministic: A→C removed.
        let mut g = make_graph(&[("A", "B"), ("B", "A"), ("A", "C"), ("B", "C")]);
        transitive_reduce(&mut g);

        assert!(g["A"].contains("B"), "A→B should remain");
        assert!(g["B"].contains("A"), "B→A should remain (cycle)");
        assert!(
            !g["A"].contains("C"),
            "A→C should be removed (redundant via A→B→C)"
        );
        assert!(g["B"].contains("C"), "B→C should remain");
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
    fn deterministic_reduction_across_insertion_orders() {
        // Same logical graph built with different insertion orders must always
        // produce the same reduced result.
        let edge_sets: Vec<Vec<(&str, &str)>> = vec![
            vec![("A", "B"), ("B", "A"), ("A", "C"), ("B", "C")],
            vec![("B", "C"), ("A", "C"), ("B", "A"), ("A", "B")],
            vec![("A", "C"), ("B", "A"), ("A", "B"), ("B", "C")],
            vec![("B", "A"), ("B", "C"), ("A", "B"), ("A", "C")],
        ];

        let mut results: Vec<Vec<(String, Vec<String>)>> = Vec::new();

        for edges in &edge_sets {
            let mut g = make_graph(edges);
            transitive_reduce(&mut g);

            // Normalize to sorted vec for comparison
            let mut sorted: Vec<(String, Vec<String>)> = g
                .into_iter()
                .map(|(k, v)| {
                    let mut vs: Vec<String> = v.into_iter().collect();
                    vs.sort();
                    (k, vs)
                })
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            results.push(sorted);
        }

        for (i, result) in results.iter().enumerate().skip(1) {
            assert_eq!(
                &results[0], result,
                "Insertion order {} produced different result than order 0",
                i
            );
        }
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
