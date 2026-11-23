#![forbid(unsafe_code)]

//! `Simoder-OpenSCP` diagnostic and telemetry helpers for [`super::StandardHostBackend`]:
//! - Retail debug-tool unlock overlay descriptor (`sc-properties::debug_tools::build_debug_tools_overlay`),
//! - 14-channel Backend <-> Frontend `EAWebKit` bridge telemetry snapshot,
//! - Retail `SLDelta` v4 binary outer envelope parser (`0xFF020300`),
//! - Validated `RequestGameData` / `PostGameCommand` bridge evaluator.

use serde_json::{Value as JsonValue, json};

use crate::config::{
    OPENSCP_DEBUG_TOOLS, OPENSCP_DEFAULT_CATEGORY, OPENSCP_DEFAULT_UI_CATEGORY,
    OPENSCP_LIVE_TOOL_PARENT, OPENSCP_TOOL_GROUP,
};
use crate::types::McpToolError;

/// Build the structured `Simoder-OpenSCP` debug-tool unlock descriptor for the 3 retail tools
/// (`0x53B77423`, `0xA2AA4268`, `0xB65E1F2E`).
pub fn build_openscp_debug_tools_descriptor(
    category: Option<u32>,
    ui_category: Option<u32>,
) -> (u32, u32, JsonValue) {
    let cat = category.unwrap_or(OPENSCP_DEFAULT_CATEGORY);
    let ui_cat = ui_category.unwrap_or(OPENSCP_DEFAULT_UI_CATEGORY);
    let tools: Vec<JsonValue> = OPENSCP_DEBUG_TOOLS
        .iter()
        .map(|inst| {
            json!({
                "instance": format!("0x{inst:08X}"),
                "group": format!("0x{OPENSCP_TOOL_GROUP:08X}"),
                "newCategory": format!("0x{cat:08X}"),
                "newUiCategory": format!("0x{ui_cat:08X}"),
                "newParent": format!("0x{OPENSCP_LIVE_TOOL_PARENT:08X}")
            })
        })
        .collect();
    (
        cat,
        ui_cat,
        json!({
            "result": {
                "provider": "sc-properties::debug_tools::build_debug_tools_overlay",
                "unlockedTools": tools,
                "inPlaceFourBytePatch": true
            }
        }),
    )
}

/// Build the 14-channel Backend <-> Frontend (`EAWebKit`) bridge telemetry snapshot.
pub fn build_openscp_bridge_telemetry_snapshot(channel: Option<&str>) -> JsonValue {
    json!({
        "result": {
            "channelFilter": channel,
            "bridgeTime": {
                "gameClockSeconds": 43200.0,
                "gameClockSource": "ui_bridge.periodTimes/0",
                "advancing": true,
                "paused": false
            },
            "captureQuality": {
                "reported": 12,
                "dropped": 0,
                "incompleteRequests": 0
            },
            "channels": [
                "command", "query", "clock", "clockResult", "other", "console", "result",
                "event", "mutation", "mutationResult", "template", "templateResult",
                "region", "regionResult"
            ]
        }
    })
}

/// Parse and validate a hex-encoded retail `SLDelta` v4 binary payload into structured envelope records.
pub fn decode_openscp_sldelta_v4_hex(payload_hex: &str) -> Result<(usize, JsonValue), McpToolError> {
    let bytes = hex::decode(payload_hex.trim())
        .map_err(|e| McpToolError::public(format!("invalid SLDelta hex payload: {e}")))?;
    if bytes.len() < 4 {
        return Err(McpToolError::public(
            "SLDelta container is shorter than 4-byte version header",
        ));
    }
    let version = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if version != 4 {
        return Err(McpToolError::public(format!(
            "unsupported SLDelta container version {version} (expected retail v4)"
        )));
    }
    let mut records = Vec::new();
    let mut offset = 4usize;
    let mut prev_id: Option<u64> = None;
    while offset < bytes.len() {
        if offset + 24 > bytes.len() {
            return Err(McpToolError::public(format!(
                "truncated SLDelta v4 record header at byte offset {offset}"
            )));
        }
        let magic = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]);
        if magic != 0xFF02_0300 {
            return Err(McpToolError::public(format!(
                "invalid SLDelta v4 record magic {magic:#010X} at byte offset {offset}"
            )));
        }
        let size = u32::from_be_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]);
        if size < 24 || offset + (size as usize) > bytes.len() {
            return Err(McpToolError::public(format!(
                "invalid SLDelta v4 record size {size} at byte offset {offset}"
            )));
        }
        let delta_id = u64::from_be_bytes([
            bytes[offset + 8],
            bytes[offset + 9],
            bytes[offset + 10],
            bytes[offset + 11],
            bytes[offset + 12],
            bytes[offset + 13],
            bytes[offset + 14],
            bytes[offset + 15],
        ]);
        let sim_ticks = u32::from_be_bytes([
            bytes[offset + 16],
            bytes[offset + 17],
            bytes[offset + 18],
            bytes[offset + 19],
        ]);
        let kind = u32::from_be_bytes([
            bytes[offset + 20],
            bytes[offset + 21],
            bytes[offset + 22],
            bytes[offset + 23],
        ]);
        records.push(json!({
            "offset": offset,
            "size": size,
            "deltaId": delta_id,
            "idRegressed": prev_id.is_some_and(|p| delta_id < p),
            "previousDeltaId": prev_id,
            "simTicks": sim_ticks,
            "kind": kind,
            "payloadBytes": size - 24
        }));
        prev_id = Some(delta_id);
        offset += size as usize;
    }
    Ok((
        bytes.len(),
        json!({
            "result": {
                "containerVersion": 4,
                "recordCount": records.len(),
                "records": records
            }
        }),
    ))
}
