use crate::error::Result;
use serde_json::Value as JsonValue;

/// Normalize Figma shared-style references to compact `"<sessionID>:<localID>"`
/// strings keyed against the top-level `styles` map produced by
/// `extract_styles`.
///
/// Three reference shapes appear in the raw output:
/// - `styleIdForFill` - reference to fill paint style
/// - `styleIdForText` - reference to text style
/// - `styleIdForStrokeFill` - reference to stroke paint style
///
/// Each is normally an object `{ guid: { localID, sessionID } }` (or
/// `{ assetRef: ... }` for cross-file library refs). For the local-guid
/// shape, we replace it with the string key so renderers can look the
/// resolved style up directly. The cross-file `assetRef` form is kept as
/// is — the actual style data isn't in this file, so there's nothing for
/// downstream code to resolve to anyway.
///
/// Important: do NOT touch the `symbolOverrides` array inside `symbolData`.
/// Each override entry there is keyed by a `guid` field that identifies
/// which descendant of the master gets overridden — it has nothing to do
/// with shared-style references.
///
/// # Arguments
/// * `tree` - The JSON tree to modify (usually the document root)
///
/// # Returns
/// * `Ok(())` - Successfully normalized all style ID fields
pub fn remove_style_ids(tree: &mut JsonValue) -> Result<()> {
    transform_recursive(tree)
}

const STYLE_REF_KEYS: &[&str] = &["styleIdForFill", "styleIdForText", "styleIdForStrokeFill"];

/// Format a guid pair as `"<sessionID>:<localID>"`, matching `extract_styles`.
fn guid_key(guid: &JsonValue) -> Option<String> {
    let obj = guid.as_object()?;
    let local = obj.get("localID")?.as_u64()?;
    let session = obj.get("sessionID")?.as_u64()?;
    Some(format!("{}:{}", session, local))
}

/// Convert one style-id reference object in place. Returns true when the
/// reference was rewritten to a string; false when we left it alone.
fn rewrite_style_ref(value: &mut JsonValue) -> bool {
    let Some(obj) = value.as_object() else { return false; };
    if let Some(guid) = obj.get("guid") {
        if let Some(key) = guid_key(guid) {
            *value = JsonValue::String(key);
            return true;
        }
    }
    false
}

/// Recursively normalize style ID fields in a JSON value
fn transform_recursive(value: &mut JsonValue) -> Result<()> {
    match value {
        JsonValue::Object(map) => {
            for key in STYLE_REF_KEYS {
                if let Some(v) = map.get_mut(*key) {
                    rewrite_style_ref(v);
                }
            }

            // Recurse into all remaining values
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(val) = map.get_mut(&key) {
                    transform_recursive(val)?;
                }
            }
        }
        JsonValue::Array(arr) => {
            // Recurse into array elements
            for val in arr.iter_mut() {
                transform_recursive(val)?;
            }
        }
        _ => {
            // Primitives - nothing to do
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rewrites_local_guid_fill_ref_to_string() {
        let mut tree = json!({
            "name": "Rectangle",
            "fillPaints": [{"color": "#ff0000", "type": "SOLID"}],
            "styleIdForFill": {
                "guid": { "localID": 28, "sessionID": 39 }
            },
            "visible": true
        });

        remove_style_ids(&mut tree).unwrap();

        assert_eq!(
            tree.get("styleIdForFill").unwrap().as_str(),
            Some("39:28")
        );
        assert_eq!(tree.get("name").unwrap().as_str(), Some("Rectangle"));
        assert!(tree.get("fillPaints").is_some());
    }

    #[test]
    fn rewrites_text_and_stroke_refs() {
        let mut tree = json!({
            "styleIdForText": { "guid": { "localID": 1, "sessionID": 2 } },
            "styleIdForStrokeFill": { "guid": { "localID": 3, "sessionID": 4 } }
        });

        remove_style_ids(&mut tree).unwrap();

        assert_eq!(tree.get("styleIdForText").unwrap().as_str(), Some("2:1"));
        assert_eq!(
            tree.get("styleIdForStrokeFill").unwrap().as_str(),
            Some("4:3")
        );
    }

    #[test]
    fn leaves_cross_file_assetref_alone() {
        // Cross-file library refs use `assetRef` instead of `guid`. The
        // referenced style data isn't in this file at all, so we leave the
        // record as-is rather than fabricate a key that resolves to nothing.
        let mut tree = json!({
            "styleIdForFill": {
                "assetRef": { "key": "abc123", "version": "1:77" }
            }
        });

        remove_style_ids(&mut tree).unwrap();

        let v = tree.get("styleIdForFill").unwrap();
        assert!(v.is_object());
        assert!(v.get("assetRef").is_some());
    }

    #[test]
    fn rewrites_in_symbol_overrides() {
        let mut tree = json!({
            "symbolData": {
                "symbolOverrides": [
                    {
                        "styleIdForFill": { "guid": { "localID": 28, "sessionID": 39 } }
                    },
                    {
                        "styleIdForText": { "guid": { "localID": 5, "sessionID": 6 } },
                        "fontSize": 12.0
                    }
                ]
            }
        });

        remove_style_ids(&mut tree).unwrap();

        assert_eq!(
            tree["symbolData"]["symbolOverrides"][0]["styleIdForFill"].as_str(),
            Some("39:28")
        );
        assert_eq!(
            tree["symbolData"]["symbolOverrides"][1]["styleIdForText"].as_str(),
            Some("6:5")
        );
        assert_eq!(
            tree["symbolData"]["symbolOverrides"][1]["fontSize"].as_f64(),
            Some(12.0)
        );
    }

    #[test]
    fn nodes_without_style_refs_unchanged() {
        let mut tree = json!({
            "name": "Frame",
            "type": "FRAME",
            "visible": true
        });

        remove_style_ids(&mut tree).unwrap();

        assert!(tree.get("styleIdForFill").is_none());
        assert_eq!(tree.get("name").unwrap().as_str(), Some("Frame"));
    }
}
