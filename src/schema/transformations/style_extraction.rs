use crate::error::Result;
use serde_json::{Map, Value as JsonValue};

/// Extract published styles into a top-level `styles` map keyed by guid string.
///
/// Figma stores published color / text / effect styles on a hidden canvas
/// (`internalOnly: true`, usually named "Internal Only Canvas"). Each style
/// is a regular geometry node that carries a `styleType` field — FILL, TEXT,
/// EFFECT, GRID. Instances reference these styles by guid via fields like
/// `styleIdForFill`, `styleIdForText`, etc.
///
/// `remove_internal_only_nodes` deletes the whole hidden canvas, and
/// `remove_style_ids` strips the references — both run after this. So we
/// first walk the tree, find every node with `styleType` set, and copy the
/// rendering-relevant subset (`fillPaints`, `strokePaints`, `effects`, plus
/// `name` / `styleType` for debugging) into a top-level `styles` object.
/// Downstream renderers can then resolve `styleIdForFill: "<sessionID>:<localID>"`
/// by looking up `styles["<sessionID>:<localID>"]`.
///
/// Operates on the wrapper object built by `build_tree_with_components`,
/// which has the shape `{document, components?}` (in any order). The new
/// `styles` field is added as a sibling of `document` so it survives the
/// later split into the final output JSON.
pub fn extract_styles(tree: &mut JsonValue) -> Result<()> {
    let mut styles: Map<String, JsonValue> = Map::new();
    collect(tree, &mut styles);
    if let Some(obj) = tree.as_object_mut() {
        obj.insert("styles".to_string(), JsonValue::Object(styles));
    }
    Ok(())
}

/// Format a guid pair as `"<sessionID>:<localID>"`.
fn guid_key(guid: &JsonValue) -> Option<String> {
    let obj = guid.as_object()?;
    let local = obj.get("localID")?.as_u64()?;
    let session = obj.get("sessionID")?.as_u64()?;
    Some(format!("{}:{}", session, local))
}

/// Fields copied verbatim from a style node into its `styles[<key>]` entry.
/// Includes everything a downstream renderer might want to apply when a node
/// references the style — paint arrays, effects, and the full text-style
/// shape (fontName / fontSize / letterSpacing / lineHeight / etc). Without
/// the text fields, an "Italic" caption style resolves to a plain colour
/// fill and the referencing text renders upright.
const COPIED_STYLE_FIELDS: &[&str] = &[
    "name",
    "styleType",
    "fillPaints",
    "strokePaints",
    "effects",
    "fontName",
    "fontSize",
    "fontWeight",
    "letterSpacing",
    "lineHeight",
    "paragraphIndent",
    "paragraphSpacing",
    "textAlignHorizontal",
    "textAlignVertical",
    "textAutoResize",
    "textCase",
    "textDecoration",
    "textTruncation",
    "leadingTrim",
    "maxLines",
];

fn collect(value: &JsonValue, out: &mut Map<String, JsonValue>) {
    match value {
        JsonValue::Object(map) => {
            // Style nodes carry both a guid and a `styleType` field. Skip
            // generic geometry nodes that happen to have a guid.
            let has_style_type = map.contains_key("styleType");
            let guid = map.get("guid");
            if has_style_type {
                if let Some(g) = guid {
                    if let Some(key) = guid_key(g) {
                        let mut entry = Map::new();
                        for field in COPIED_STYLE_FIELDS {
                            if let Some(v) = map.get(*field) {
                                entry.insert((*field).to_string(), v.clone());
                            }
                        }
                        out.insert(key, JsonValue::Object(entry));
                    }
                }
            }
            for v in map.values() {
                collect(v, out);
            }
        }
        JsonValue::Array(arr) => {
            for v in arr {
                collect(v, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_fill_style_by_guid_key() {
        let mut tree = json!({
            "document": {
                "children": [
                    {
                        "name": "Internal Only Canvas",
                        "internalOnly": true,
                        "children": [
                            {
                                "name": "Primary/Primary",
                                "guid": { "localID": 28, "sessionID": 39 },
                                "styleType": "FILL",
                                "fillPaints": [
                                    { "color": "#04c88f", "type": "SOLID" }
                                ]
                            }
                        ]
                    }
                ]
            }
        });

        extract_styles(&mut tree).unwrap();

        let styles = tree.get("styles").unwrap();
        let entry = styles.get("39:28").unwrap();
        assert_eq!(entry.get("name").unwrap().as_str(), Some("Primary/Primary"));
        assert_eq!(entry.get("styleType").unwrap().as_str(), Some("FILL"));
        assert!(entry.get("fillPaints").is_some());
    }

    #[test]
    fn ignores_geometry_nodes_without_style_type() {
        let mut tree = json!({
            "document": {
                "children": [
                    {
                        "name": "Some Frame",
                        "guid": { "localID": 1, "sessionID": 2 },
                        "fillPaints": [{ "color": "#fff", "type": "SOLID" }]
                    }
                ]
            }
        });

        extract_styles(&mut tree).unwrap();

        let styles = tree.get("styles").unwrap().as_object().unwrap();
        assert!(styles.is_empty());
    }
}
