use crate::error::Result;
use serde_json::Value as JsonValue;

/// Remove guid fields from all objects in the JSON tree
///
/// Recursively traverses the JSON tree and removes "guid" fields, with one
/// scoped exception: nodes anywhere under `components[*]` keep their guid.
/// Renderers walking an instance's `symbolOverrides[*].guidPath` need to
/// follow that path through the cloned master tree, which only works if the
/// master descendants still carry their guids.
///
/// # Arguments
/// * `tree` - The JSON tree to modify (usually the document root)
///
/// # Returns
/// * `Ok(())` - Successfully removed all guid fields
pub fn remove_guid_fields(tree: &mut JsonValue) -> Result<()> {
    transform_inner(tree, false)
}

/// `inside_master` is true when this value lives anywhere under a
/// `components[*]` master subtree.
fn transform_inner(value: &mut JsonValue, inside_master: bool) -> Result<()> {
    match value {
        JsonValue::Object(map) => {
            if !inside_master {
                map.remove("guid");
            }
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(val) = map.get_mut(&key) {
                    transform_inner(val, inside_master)?;
                }
            }
        }
        JsonValue::Array(arr) => {
            for val in arr.iter_mut() {
                transform_inner(val, inside_master)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Like `remove_guid_fields` but seeds `inside_master = true` for the
/// `components` subtree of the wrapper object built by
/// `build_tree_with_components`. Use this on the wrapper rather than the
/// plain `remove_guid_fields` so master descendants keep their guids.
pub fn remove_guid_fields_outside_masters(tree: &mut JsonValue) -> Result<()> {
    let Some(obj) = tree.as_object_mut() else {
        return transform_inner(tree, false);
    };
    let keys: Vec<String> = obj.keys().cloned().collect();
    for key in keys {
        if let Some(val) = obj.get_mut(&key) {
            let inside = key == "components";
            transform_inner(val, inside)?;
        }
    }
    obj.remove("guid");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_remove_guid_simple() {
        let mut tree = json!({
            "name": "Rectangle",
            "guid": {
                "localID": 3,
                "sessionID": 1
            },
            "visible": true
        });

        remove_guid_fields(&mut tree).unwrap();

        assert!(tree.get("guid").is_none());
        assert_eq!(tree.get("name").unwrap().as_str(), Some("Rectangle"));
        assert_eq!(tree.get("visible").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn test_remove_guid_nested() {
        let mut tree = json!({
            "name": "Frame",
            "guid": {"localID": 1, "sessionID": 2},
            "children": [
                {
                    "name": "Child1",
                    "guid": {"localID": 3, "sessionID": 4}
                },
                {
                    "name": "Child2",
                    "guid": {"localID": 5, "sessionID": 6}
                }
            ]
        });

        remove_guid_fields(&mut tree).unwrap();

        assert!(tree.get("guid").is_none());
        assert!(tree["children"][0].get("guid").is_none());
        assert!(tree["children"][1].get("guid").is_none());
    }

    #[test]
    fn preserves_guids_inside_components_subtrees() {
        // The components array carries master subtrees; renderers walk into
        // them when hydrating instances and need every descendant's guid
        // intact so symbolOverrides[*].guidPath references resolve.
        let mut tree = json!({
            "document": {
                "guid": { "localID": 0, "sessionID": 0 },
                "name": "Document"
            },
            "components": [
                {
                    "name": "MasterA",
                    "guid": { "localID": 1, "sessionID": 2 },
                    "children": [
                        {
                            "name": "InnerVector",
                            "guid": { "localID": 1594, "sessionID": 33 },
                            "fillPaints": [{"color": "#ffffff", "type": "SOLID"}]
                        }
                    ]
                }
            ]
        });

        remove_guid_fields_outside_masters(&mut tree).unwrap();

        // Document's guid stripped (outside any master subtree).
        assert!(tree["document"].get("guid").is_none());
        // Master root and descendant both keep their guids.
        assert!(tree["components"][0].get("guid").is_some());
        assert!(tree["components"][0]["children"][0].get("guid").is_some());
    }
}
