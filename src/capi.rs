//! C ABI for embedding fig2json as a static library (no subprocess).
//!
//! Mirrors the CLI behavior: ZIP containers are extracted into `out_dir`
//! (which also writes the `images/` directory) and every contained .fig is
//! converted, writing `<name>.json` next to it; bare fig-kiwi files convert
//! directly with `out_dir` as the image base directory. The first converted
//! document is returned as a JSON string.

use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

fn find_fig_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                find_fig_files(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("fig") {
                out.push(path);
            }
        }
    }
}

fn convert_file_impl(fig_path: &Path, out_dir: &Path) -> anyhow::Result<String> {
    let bytes = fs::read(fig_path)?;
    fs::create_dir_all(out_dir)?;

    if crate::parser::is_zip_container(&bytes) {
        crate::parser::extract_zip_to_directory(&bytes, &out_dir.to_path_buf())?;
        let mut figs = Vec::new();
        find_fig_files(out_dir, &mut figs);
        if figs.is_empty() {
            anyhow::bail!("no .fig files found in ZIP archive");
        }
        let mut first: Option<String> = None;
        for fig in figs {
            let fig_bytes = fs::read(&fig)?;
            let json = crate::convert(&fig_bytes, fig.parent())?;
            let text = serde_json::to_string(&json)?;
            fs::write(fig.with_extension("json"), &text)?;
            if first.is_none() {
                first = Some(text);
            }
        }
        Ok(first.unwrap())
    } else {
        let json = crate::convert(&bytes, Some(out_dir))?;
        let text = serde_json::to_string(&json)?;
        fs::write(out_dir.join("canvas.json"), &text)?;
        Ok(text)
    }
}

unsafe fn to_path(p: *const c_char) -> Option<PathBuf> {
    if p.is_null() {
        return None;
    }
    CStr::from_ptr(p).to_str().ok().map(PathBuf::from)
}

fn to_c_string(s: String) -> *mut c_char {
    // Interior NULs would truncate — replace them defensively.
    CString::new(s.replace('\0', " ")).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

/// Converts `fig_path` into `out_dir` and returns the document JSON as a
/// NUL-terminated UTF-8 string (free with `fig2json_free`). On failure
/// returns NULL and, when `err` is non-NULL, stores a message there (also
/// freed with `fig2json_free`).
#[no_mangle]
pub unsafe extern "C" fn fig2json_convert_file(
    fig_path: *const c_char,
    out_dir: *const c_char,
    err: *mut *mut c_char,
) -> *mut c_char {
    if !err.is_null() {
        *err = std::ptr::null_mut();
    }
    let (Some(fig), Some(dir)) = (to_path(fig_path), to_path(out_dir)) else {
        if !err.is_null() {
            *err = to_c_string("fig2json: invalid path argument".to_string());
        }
        return std::ptr::null_mut();
    };
    match std::panic::catch_unwind(|| convert_file_impl(&fig, &dir)) {
        Ok(Ok(json)) => to_c_string(json),
        Ok(Err(e)) => {
            if !err.is_null() {
                *err = to_c_string(format!("fig2json: {e}"));
            }
            std::ptr::null_mut()
        }
        Err(_) => {
            if !err.is_null() {
                *err = to_c_string("fig2json: internal panic during conversion".to_string());
            }
            std::ptr::null_mut()
        }
    }
}

/// Frees a string returned by `fig2json_convert_file`.
#[no_mangle]
pub unsafe extern "C" fn fig2json_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}
