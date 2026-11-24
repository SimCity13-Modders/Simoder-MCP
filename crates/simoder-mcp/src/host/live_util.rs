#![forbid(unsafe_code)]

//! Utility helpers for [`super::LiveHostBackend`]: hexadecimal address parsing, mod directory
//! summary scanning, and UTC timestamp formatting.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value as JsonValue, json};

use crate::types::McpToolError;

pub(super) fn parse_hex_address(raw: &str) -> Result<usize, McpToolError> {
    let trimmed = raw.trim();
    let hex_digits = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    usize::from_str_radix(hex_digits, 16)
        .map_err(|e| McpToolError::public(format!("invalid hexadecimal address '{raw}': {e}")))
}

pub(super) fn scan_mod_summaries(mods_dir: &Path) -> Vec<JsonValue> {
    let Ok(entries) = fs::read_dir(mods_dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();
    let mut out = Vec::new();
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let dir_name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        if dir_name.eq_ignore_ascii_case("cache") {
            continue;
        }
        let manifest_path = dir.join("mod.toml");
        let manifest_present = manifest_path.is_file();
        let (mod_id, version, has_scripts, has_native) = if let Ok(raw) = fs::read_to_string(&manifest_path) {
            let mut id = None;
            let mut ver = None;
            let mut section = "";
            let mut scripts = false;
            let mut native = false;
            for line in raw.lines() {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') {
                    continue;
                }
                if t.starts_with('[') && t.ends_with(']') {
                    section = t;
                    if t == "[scripts]" {
                        scripts = true;
                    } else if t == "[native]" {
                        native = true;
                    }
                    continue;
                }
                if section == "[metadata]"
                    && let Some((k, v)) = t.split_once('=')
                {
                    let val = v.trim().trim_matches('"').trim_matches('\'');
                    match k.trim() {
                        "id" => id = Some(val.to_string()),
                        "version" => ver = Some(val.to_string()),
                        _ => {}
                    }
                }
            }
            (id.unwrap_or_else(|| dir_name.clone()), ver, scripts, native)
        } else {
            (dir_name.clone(), None, false, false)
        };
        out.push(json!({
            "directory": dir_name,
            "id": mod_id,
            "version": version,
            "hasManifest": manifest_present,
            "hasScripts": has_scripts,
            "hasNative": has_native,
        }));
    }
    out
}

/// Format the current UTC wall clock as an ISO-8601 timestamp (`YYYY-MM-DDTHH:MM:SS+00:00`) using Howard Hinnant's civil calendar conversion.
pub fn format_utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_i64, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}+00:00")
}
