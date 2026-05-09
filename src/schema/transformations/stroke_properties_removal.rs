use crate::error::Result;
use serde_json::Value as JsonValue;

/// Remove stroke property fields from all objects in the JSON tree.
///
/// Currently a no-op. The fields this pass used to strip
/// (`strokeAlign`, `strokeJoin`) are needed by pixel renderers like
/// fig2psd to draw the stroke at the correct position relative to the
/// path: INSIDE shrinks the visible region into the path interior,
/// OUTSIDE pushes it past the path edge, CENTER straddles. Without the
/// alignment, fig2psd defaulted to CENTER everywhere — visible on the
/// wallet's "Ethereum" card icon, which has `strokeAlign: INSIDE` and
/// `strokeWeight: 8` and rendered with half its 8 px stroke bleeding
/// past the rounded corner instead of sitting flush inside.
///
/// Kept as a function (no-op now) to avoid churning the lib.rs pipeline
/// ordering and the docs that reference it.
///
/// # Arguments
/// * `tree` - The JSON tree to modify (usually the document root)
///
/// # Returns
/// * `Ok(())` - Successfully removed all stroke property fields
///
/// # Examples
/// ```no_run
/// use fig2json::schema::remove_stroke_properties;
/// use serde_json::json;
///
/// let mut tree = json!({
///     "name": "Rectangle",
///     "strokeAlign": {
///         "__enum__": "StrokeAlign",
///         "value": "INSIDE"
///     },
///     "strokeJoin": {
///         "__enum__": "StrokeJoin",
///         "value": "MITER"
///     },
///     "strokeWeight": 1.0,
///     "visible": true
/// });
/// remove_stroke_properties(&mut tree).unwrap();
/// // tree now has only "name" and "visible" fields
/// ```
pub fn remove_stroke_properties(tree: &mut JsonValue) -> Result<()> {
    transform_recursive(tree)
}

/// Recursively walk the JSON tree. Currently no-op — see the doc on
/// `remove_stroke_properties` for why we now keep `strokeAlign` and
/// `strokeJoin`.
fn transform_recursive(value: &mut JsonValue) -> Result<()> {
    match value {
        JsonValue::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(val) = map.get_mut(&key) {
                    transform_recursive(val)?;
                }
            }
        }
        JsonValue::Array(arr) => {
            for val in arr.iter_mut() {
                transform_recursive(val)?;
            }
        }
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_preserves_stroke_align() {
        let mut tree = json!({
            "name": "Rectangle",
            "strokeAlign": "INSIDE",
            "visible": true
        });

        remove_stroke_properties(&mut tree).unwrap();

        assert_eq!(tree.get("strokeAlign").and_then(|v| v.as_str()), Some("INSIDE"));
    }

    #[test]
    fn test_preserves_stroke_join() {
        let mut tree = json!({
            "name": "Line",
            "strokeJoin": "MITER",
            "opacity": 1.0
        });

        remove_stroke_properties(&mut tree).unwrap();

        assert_eq!(tree.get("strokeJoin").and_then(|v| v.as_str()), Some("MITER"));
    }

    #[test]
    fn test_preserve_stroke_weight() {
        let mut tree = json!({
            "name": "Shape",
            "strokeWeight": 1.0,
            "visible": true
        });

        remove_stroke_properties(&mut tree).unwrap();

        // strokeWeight is kept for downstream rasterisers.
        assert_eq!(tree.get("strokeWeight").and_then(|v| v.as_f64()), Some(1.0));
        assert_eq!(tree.get("name").unwrap().as_str(), Some("Shape"));
        assert_eq!(tree.get("visible").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn test_preserves_all_stroke_fields() {
        let mut tree = json!({
            "name": "Rectangle",
            "strokeAlign": "CENTER",
            "strokeJoin": "BEVEL",
            "strokeWeight": 2.5,
        });

        remove_stroke_properties(&mut tree).unwrap();

        assert_eq!(tree.get("strokeAlign").and_then(|v| v.as_str()), Some("CENTER"));
        assert_eq!(tree.get("strokeJoin").and_then(|v| v.as_str()), Some("BEVEL"));
        assert_eq!(tree.get("strokeWeight").and_then(|v| v.as_f64()), Some(2.5));
    }

    #[test]
    fn test_no_stroke_properties() {
        let mut tree = json!({
            "name": "Rectangle",
            "width": 100,
            "height": 200,
            "visible": true
        });

        remove_stroke_properties(&mut tree).unwrap();

        // Tree without stroke properties should be unchanged
        assert_eq!(tree.get("name").unwrap().as_str(), Some("Rectangle"));
        assert_eq!(tree.get("width").unwrap().as_i64(), Some(100));
        assert_eq!(tree.get("height").unwrap().as_i64(), Some(200));
        assert_eq!(tree.get("visible").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn test_preserves_stroke_paints() {
        let mut tree = json!({
            "name": "Line",
            "strokeWeight": 1.0,
            "strokePaints": [{ "color": "#000000" }]
        });

        remove_stroke_properties(&mut tree).unwrap();

        assert_eq!(tree.get("strokeWeight").and_then(|v| v.as_f64()), Some(1.0));
        assert!(tree.get("strokePaints").is_some());
        assert_eq!(tree["strokePaints"][0]["color"].as_str(), Some("#000000"));
    }
}
