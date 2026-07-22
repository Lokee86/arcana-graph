use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Value, json};

use crate::repository::{RelationKind, edge_kind_to_relation};
use crate::synthetic::NodeId;

use super::request::QueryDirection;
use super::response::node_value;
use super::session::{ProtocolSnapshot, RequestFailure};

pub(crate) const DEFAULT_DEPTH: usize = 12;
pub(crate) const MAX_DEPTH: usize = 64;
pub(crate) const DEFAULT_RESULT_LIMIT: usize = 1_000;
pub(crate) const MAX_RESULT_LIMIT: usize = 10_000;
pub(crate) const DEFAULT_PATH_LIMIT: usize = 100;
pub(crate) const MAX_PATH_LIMIT: usize = 1_000;

pub(crate) type GraphPath = (Vec<NodeId>, Vec<RelationKind>);

pub(crate) fn bounded_depth(depth: Option<usize>) -> usize {
    depth.unwrap_or(DEFAULT_DEPTH).min(MAX_DEPTH)
}

pub(crate) fn bounded_result_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_RESULT_LIMIT).min(MAX_RESULT_LIMIT)
}

pub(crate) fn bounded_path_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_PATH_LIMIT).min(MAX_PATH_LIMIT)
}

pub(crate) fn require_node(
    snapshot: &ProtocolSnapshot,
    value: u32,
) -> Result<NodeId, RequestFailure> {
    let node = NodeId(value);
    snapshot
        .entry(node)
        .map(|_| node)
        .ok_or_else(|| RequestFailure::new("unknown_node", format!("node {value} does not exist")))
}

pub(crate) fn require_entries(
    snapshot: &ProtocolSnapshot,
    values: &[u32],
) -> Result<Vec<NodeId>, RequestFailure> {
    if values.is_empty() {
        return Err(RequestFailure::new(
            "missing_entry_points",
            "entry_node_ids must contain at least one node",
        ));
    }
    let mut entries = values
        .iter()
        .map(|value| require_node(snapshot, *value))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_unstable();
    entries.dedup();
    Ok(entries)
}

pub(crate) fn parse_relations(
    values: Option<&[String]>,
) -> Result<Option<BTreeSet<RelationKind>>, RequestFailure> {
    let Some(values) = values else {
        return Ok(None);
    };
    let mut relations = BTreeSet::new();
    for value in values {
        let relation = RelationKind::parse(value).ok_or_else(|| {
            RequestFailure::new("invalid_relation", format!("unknown relation '{value}'"))
        })?;
        relations.insert(relation);
    }
    Ok(Some(relations))
}

pub(crate) fn call_relations(include_possible: bool) -> BTreeSet<RelationKind> {
    let mut relations = BTreeSet::from([RelationKind::Calls]);
    if include_possible {
        relations.insert(RelationKind::PossibleCalls);
    }
    relations
}

pub(crate) fn impact_relations() -> BTreeSet<RelationKind> {
    BTreeSet::from([
        RelationKind::Calls,
        RelationKind::PossibleCalls,
        RelationKind::References,
    ])
}

pub(crate) fn graph_neighbors(
    snapshot: &ProtocolSnapshot,
    node: NodeId,
    direction: QueryDirection,
    allowed: Option<&BTreeSet<RelationKind>>,
) -> Result<Vec<(NodeId, RelationKind)>, RequestFailure> {
    let raw = match direction {
        QueryDirection::Outgoing => snapshot.graph.forward_neighbors(node),
        QueryDirection::Incoming => snapshot.graph.reverse_neighbors(node),
    }
    .map_err(|error| RequestFailure::new("query_failed", error.to_string()))?;

    let mut result = Vec::new();
    for neighbor in raw {
        let relation = edge_kind_to_relation(neighbor.kind).ok_or_else(|| {
            RequestFailure::new(
                "corrupt_graph",
                format!("unknown edge kind {}", neighbor.kind.0),
            )
        })?;
        if allowed.is_none_or(|allowed| allowed.contains(&relation)) {
            result.push((neighbor.node, relation));
        }
    }
    result.sort_unstable_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    Ok(result)
}

pub(crate) fn bfs_distances(
    snapshot: &ProtocolSnapshot,
    starts: &[NodeId],
    direction: QueryDirection,
    allowed: Option<&BTreeSet<RelationKind>>,
    max_depth: usize,
) -> Result<BTreeMap<NodeId, usize>, RequestFailure> {
    let mut distances = BTreeMap::new();
    let mut queue = VecDeque::new();
    for start in starts {
        distances.insert(*start, 0);
        queue.push_back(*start);
    }
    while let Some(node) = queue.pop_front() {
        let depth = distances[&node];
        if depth >= max_depth {
            continue;
        }
        for (neighbor, _) in graph_neighbors(snapshot, node, direction, allowed)? {
            if let std::collections::btree_map::Entry::Vacant(entry) = distances.entry(neighbor) {
                entry.insert(depth + 1);
                queue.push_back(neighbor);
            }
        }
    }
    Ok(distances)
}

pub(crate) fn shortest_path(
    snapshot: &ProtocolSnapshot,
    start: NodeId,
    target: NodeId,
    allowed: Option<&BTreeSet<RelationKind>>,
    max_depth: usize,
) -> Result<Option<GraphPath>, RequestFailure> {
    if start == target {
        return Ok(Some((vec![start], Vec::new())));
    }
    let mut depth = BTreeMap::from([(start, 0_usize)]);
    let mut parent = BTreeMap::<NodeId, (NodeId, RelationKind)>::new();
    let mut queue = VecDeque::from([start]);
    while let Some(node) = queue.pop_front() {
        let current_depth = depth[&node];
        if current_depth >= max_depth {
            continue;
        }
        for (neighbor, relation) in
            graph_neighbors(snapshot, node, QueryDirection::Outgoing, allowed)?
        {
            if depth.contains_key(&neighbor) {
                continue;
            }
            depth.insert(neighbor, current_depth + 1);
            parent.insert(neighbor, (node, relation));
            if neighbor == target {
                return Ok(Some(reconstruct_path(start, target, &parent)));
            }
            queue.push_back(neighbor);
        }
    }
    Ok(None)
}

fn reconstruct_path(
    start: NodeId,
    target: NodeId,
    parent: &BTreeMap<NodeId, (NodeId, RelationKind)>,
) -> (Vec<NodeId>, Vec<RelationKind>) {
    let mut nodes = vec![target];
    let mut relations = Vec::new();
    let mut current = target;
    while current != start {
        let (previous, relation) = parent[&current].clone();
        nodes.push(previous);
        relations.push(relation);
        current = previous;
    }
    nodes.reverse();
    relations.reverse();
    (nodes, relations)
}

pub(crate) fn path_value(
    snapshot: &ProtocolSnapshot,
    nodes: &[NodeId],
    relations: &[RelationKind],
) -> Result<Value, RequestFailure> {
    let node_values = nodes
        .iter()
        .map(|node| {
            snapshot.entry(*node).map(node_value).ok_or_else(|| {
                RequestFailure::new(
                    "invalid_snapshot",
                    format!("missing catalogue node {}", node.0),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "depth": relations.len(),
        "nodes": node_values,
        "relations": relations.iter().map(RelationKind::as_str).collect::<Vec<_>>(),
    }))
}

pub(crate) fn related_values(
    snapshot: &ProtocolSnapshot,
    values: &[(NodeId, RelationKind)],
) -> Result<Vec<Value>, RequestFailure> {
    values
        .iter()
        .map(|(node, relation)| {
            let entry = snapshot.entry(*node).ok_or_else(|| {
                RequestFailure::new(
                    "invalid_snapshot",
                    format!("missing catalogue node {}", node.0),
                )
            })?;
            Ok(json!({"relation": relation.as_str(), "node": node_value(entry)}))
        })
        .collect()
}
