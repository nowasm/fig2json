use crate::error::{FigError, Result};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};

/// Build a tree structure from flat nodeChanges array
///
/// Takes the flat array of nodes and builds a hierarchical tree structure
/// by creating parent-child relationships based on parentIndex fields.
///
/// # Arguments
/// * `node_changes` - Array of node objects from decoded Kiwi data
///
/// # Returns
/// * `Ok(JsonValue)` - Root node with children hierarchy
/// * `Err(FigError)` - If tree building fails
///
/// # Examples
/// ```no_run
/// use fig2json::schema::build_tree;
/// use serde_json::json;
///
/// let node_changes = vec![/* node objects */];
/// let root = build_tree(node_changes).unwrap();
/// ```
pub fn build_tree(node_changes: Vec<JsonValue>) -> Result<JsonValue> {
    let (nodes, parent_to_children) = index_node_changes(&node_changes)?;
    build_node_tree("0:0", &nodes, &parent_to_children)
}

/// Build the document tree (rooted at "0:0") plus the subtrees of all
/// COMPONENT master definitions (nodes with type = SYMBOL) that live outside
/// the document's visible tree. Returns `{ document, components }`.
///
/// Component masters are typically parented under hidden page-like containers
/// that the document doesn't walk into, which is why
/// `build_tree` alone drops them. Renderers that want to materialise an
/// INSTANCE need the master's children, so emit them as a separate list
/// keyed by guid.
pub fn build_tree_with_components(node_changes: Vec<JsonValue>) -> Result<JsonValue> {
    let (nodes, parent_to_children) = index_node_changes(&node_changes)?;
    let document = build_node_tree("0:0", &nodes, &parent_to_children)?;

    // Collect every SYMBOL node's guid. Figma parks component masters on a
    // hidden "Internal Only Canvas" that `remove_internal_only_nodes` later
    // strips from the document tree — so we pick them up NOW, before that
    // filter runs, regardless of whether they're reachable from "0:0".
    let symbol_guids: HashSet<String> = nodes
        .iter()
        .filter_map(|(g, n)| if node_is_symbol(n) { Some(g.clone()) } else { None })
        .collect();

    // Emit only the top-most SYMBOL of each tree (a SYMBOL nested inside
    // another SYMBOL is already reachable via the outer master's children).
    let mut components: Vec<JsonValue> = Vec::new();
    for guid in &symbol_guids {
        if has_symbol_ancestor(guid, &nodes, &symbol_guids) {
            continue;
        }
        let subtree = build_node_tree(guid, &nodes, &parent_to_children)?;
        components.push(subtree);
    }

    let mut out = serde_json::Map::new();
    out.insert("document".to_string(), document);
    if !components.is_empty() {
        out.insert("components".to_string(), JsonValue::Array(components));
    }
    Ok(JsonValue::Object(out))
}

fn has_symbol_ancestor(
    guid: &str,
    nodes: &HashMap<String, JsonValue>,
    symbol_guids: &HashSet<String>,
) -> bool {
    let mut cur = match nodes.get(guid).and_then(|n| n.get("parentIndex")) {
        Some(p) => match format_parent_guid(p) {
            Ok(g) => g,
            Err(_) => return false,
        },
        None => return false,
    };
    let mut visited: HashSet<String> = HashSet::new();
    while visited.insert(cur.clone()) {
        if symbol_guids.contains(&cur) {
            return true;
        }
        let parent = match nodes.get(&cur).and_then(|n| n.get("parentIndex")) {
            Some(p) => match format_parent_guid(p) {
                Ok(g) => g,
                Err(_) => return false,
            },
            None => return false,
        };
        cur = parent;
    }
    false
}

fn index_node_changes(
    node_changes: &[JsonValue],
) -> Result<(HashMap<String, JsonValue>, HashMap<String, Vec<(String, String)>>)> {
    let mut nodes: HashMap<String, JsonValue> = HashMap::new();
    let mut parent_to_children: HashMap<String, Vec<(String, String)>> = HashMap::new();

    for node in node_changes {
        let guid = format_guid(node)?;
        nodes.insert(guid, node.clone());
    }
    for node in node_changes {
        if let Some(parent_index) = node.get("parentIndex") {
            let parent_guid = format_parent_guid(parent_index)?;
            let child_guid = format_guid(node)?;
            let position = parent_index
                .get("position")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            parent_to_children
                .entry(parent_guid)
                .or_default()
                .push((position, child_guid));
        }
    }
    for children in parent_to_children.values_mut() {
        children.sort_by(|a, b| a.0.cmp(&b.0));
    }
    Ok((nodes, parent_to_children))
}


fn node_is_symbol(node: &JsonValue) -> bool {
    // After enum simplification the type field is a plain string, but the
    // pre-transform data has {__enum__: "NodeType", value: "SYMBOL"}. Accept
    // either form so this can run before or after schema simplification.
    let t = node.get("type");
    if let Some(JsonValue::String(s)) = t {
        return s == "SYMBOL";
    }
    if let Some(JsonValue::Object(m)) = t {
        if let Some(JsonValue::String(v)) = m.get("value") {
            return v == "SYMBOL";
        }
    }
    false
}

/// Recursively build a node with its children
fn build_node_tree(
    guid: &str,
    nodes: &HashMap<String, JsonValue>,
    parent_to_children: &HashMap<String, Vec<(String, String)>>,
) -> Result<JsonValue> {
    // Get the node
    let mut node = nodes
        .get(guid)
        .ok_or_else(|| FigError::ZipError(format!("Node {} not found", guid)))?
        .clone();

    // Remove parentIndex
    if let Some(obj) = node.as_object_mut() {
        obj.remove("parentIndex");

        // Add children recursively
        if let Some(child_entries) = parent_to_children.get(guid) {
            let mut children = Vec::new();
            for (_position, child_guid) in child_entries {
                let child_node = build_node_tree(child_guid, nodes, parent_to_children)?;
                children.push(child_node);
            }

            if !children.is_empty() {
                obj.insert("children".to_string(), JsonValue::Array(children));
            }
        }
    }

    Ok(node)
}

/// Format a GUID from a node's guid field
///
/// Converts `{sessionID: X, localID: Y}` to string "X:Y"
fn format_guid(node: &JsonValue) -> Result<String> {
    let guid_obj = node
        .get("guid")
        .and_then(|v| v.as_object())
        .ok_or_else(|| FigError::ZipError("Node missing guid field".to_string()))?;

    let session_id = guid_obj
        .get("sessionID")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| FigError::ZipError("Invalid sessionID in guid".to_string()))?;

    let local_id = guid_obj
        .get("localID")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| FigError::ZipError("Invalid localID in guid".to_string()))?;

    Ok(format!("{}:{}", session_id, local_id))
}

/// Format a GUID from a parentIndex's guid field
fn format_parent_guid(parent_index: &JsonValue) -> Result<String> {
    let guid_obj = parent_index
        .get("guid")
        .and_then(|v| v.as_object())
        .ok_or_else(|| FigError::ZipError("parentIndex missing guid field".to_string()))?;

    let session_id = guid_obj
        .get("sessionID")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| FigError::ZipError("Invalid sessionID in parentIndex".to_string()))?;

    let local_id = guid_obj
        .get("localID")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| FigError::ZipError("Invalid localID in parentIndex".to_string()))?;

    Ok(format!("{}:{}", session_id, local_id))
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_format_guid() {
        let node = json!({
            "guid": {
                "sessionID": 1,
                "localID": 42
            }
        });

        assert_eq!(format_guid(&node).unwrap(), "1:42");
    }

    #[test]
    fn test_format_parent_guid() {
        let parent_index = json!({
            "guid": {
                "sessionID": 0,
                "localID": 1
            },
            "position": "!"
        });

        assert_eq!(format_parent_guid(&parent_index).unwrap(), "0:1");
    }

    #[test]
    fn test_build_tree_simple() {
        let node_changes = vec![
            json!({
                "guid": {"sessionID": 0, "localID": 0},
                "name": "Root",
                "type": "DOCUMENT"
            }),
            json!({
                "guid": {"sessionID": 0, "localID": 1},
                "parentIndex": {
                    "guid": {"sessionID": 0, "localID": 0},
                    "position": "a"
                },
                "name": "Child1"
            }),
            json!({
                "guid": {"sessionID": 0, "localID": 2},
                "parentIndex": {
                    "guid": {"sessionID": 0, "localID": 0},
                    "position": "b"
                },
                "name": "Child2"
            }),
        ];

        let root = build_tree(node_changes).unwrap();

        // Check root
        assert_eq!(root.get("name").and_then(|v| v.as_str()), Some("Root"));

        // Check children
        let children = root.get("children").and_then(|v| v.as_array()).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].get("name").and_then(|v| v.as_str()), Some("Child1"));
        assert_eq!(children[1].get("name").and_then(|v| v.as_str()), Some("Child2"));

        // Check parentIndex is removed
        assert!(children[0].get("parentIndex").is_none());
    }

    #[test]
    fn test_build_tree_with_components_emits_orphan_symbols() {
        let node_changes = vec![
            json!({
                "guid": {"sessionID": 0, "localID": 0},
                "name": "Doc"
            }),
            // Orphan SYMBOL master (no parentIndex)
            json!({
                "guid": {"sessionID": 1, "localID": 1},
                "name": "Master",
                "type": { "__enum__": "NodeType", "value": "SYMBOL" }
            }),
            // Child of the master
            json!({
                "guid": {"sessionID": 1, "localID": 2},
                "name": "MasterChild",
                "parentIndex": {
                    "guid": {"sessionID": 1, "localID": 1},
                    "position": "a"
                }
            }),
        ];
        let out = build_tree_with_components(node_changes).unwrap();
        let document = out.get("document").unwrap();
        assert_eq!(document.get("name").and_then(|v| v.as_str()), Some("Doc"));

        let components = out
            .get("components")
            .expect("components should be present")
            .as_array()
            .unwrap();
        assert_eq!(components.len(), 1, "expected one master");
        let master = &components[0];
        assert_eq!(master.get("name").and_then(|v| v.as_str()), Some("Master"));
        let kids = master.get("children").and_then(|v| v.as_array()).unwrap();
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].get("name").and_then(|v| v.as_str()), Some("MasterChild"));
    }

    #[test]
    fn test_sort_children_by_position() {
        let node_changes = vec![
            json!({
                "guid": {"sessionID": 0, "localID": 0},
                "name": "Root"
            }),
            json!({
                "guid": {"sessionID": 0, "localID": 1},
                "parentIndex": {
                    "guid": {"sessionID": 0, "localID": 0},
                    "position": "z"  // Should be last
                },
                "name": "Third"
            }),
            json!({
                "guid": {"sessionID": 0, "localID": 2},
                "parentIndex": {
                    "guid": {"sessionID": 0, "localID": 0},
                    "position": "a"  // Should be first
                },
                "name": "First"
            }),
            json!({
                "guid": {"sessionID": 0, "localID": 3},
                "parentIndex": {
                    "guid": {"sessionID": 0, "localID": 0},
                    "position": "m"  // Should be second
                },
                "name": "Second"
            }),
        ];

        let root = build_tree(node_changes).unwrap();
        let children = root.get("children").and_then(|v| v.as_array()).unwrap();

        // Check sorted order
        assert_eq!(children[0].get("name").and_then(|v| v.as_str()), Some("First"));
        assert_eq!(children[1].get("name").and_then(|v| v.as_str()), Some("Second"));
        assert_eq!(children[2].get("name").and_then(|v| v.as_str()), Some("Third"));
    }
}
