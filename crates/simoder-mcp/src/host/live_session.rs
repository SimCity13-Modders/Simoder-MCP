#![forbid(unsafe_code)]

//! Live `EAWebKit` ES5 router client (`session_router.js`), bounded postcondition polling,
//! and unit/zone diff utilities backing [`super::LiveHostBackend`].

use std::collections::BTreeSet;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value as JsonValue, json};

use super::UiJsEvalFn;
use crate::types::{McpToolError, NativeSessionOperation, NativeSessionRequest, StartupStage};

/// Verified strict-ES5 router script executed inside SimCity's active `EAWebKit` view.
pub(super) const SESSION_ROUTER_JS: &str = include_str!("session_router.js");

const POLL_INTERVAL: Duration = Duration::from_millis(200);
const DEFAULT_ROUTER_TIMEOUT_MS: u32 = 8_000;

/// Execute one JSON packet operation against [`SESSION_ROUTER_JS`] through `js_bridge`.
pub(super) fn eval_router_op(
    js_bridge: Option<&UiJsEvalFn>,
    packet: JsonValue,
    timeout_ms: u32,
) -> Result<JsonValue, McpToolError> {
    let bridge = js_bridge.ok_or_else(|| {
        McpToolError::public("EAWebKit semantic bridge is unavailable in this host session")
    })?;
    let packet_json = serde_json::to_string(&packet)
        .map_err(|e| McpToolError::public(format!("failed serializing router packet: {e}")))?;
    let raw_reply =
        (bridge)(SESSION_ROUTER_JS, &packet_json, timeout_ms).map_err(McpToolError::public)?;
    let parsed: JsonValue = serde_json::from_str(&raw_reply).map_err(|e| {
        McpToolError::public(format!(
            "EAWebKit session router returned malformed JSON ({e})"
        ))
    })?;
    if parsed.get("ok").and_then(JsonValue::as_bool) != Some(true) {
        return Err(McpToolError::public(format!(
            "Native game operation failed: {raw_reply}"
        )));
    }
    Ok(parsed.get("result").cloned().unwrap_or(JsonValue::Null))
}

/// Map native `observe` phase strings onto typed [`StartupStage`] camelCase names.
pub(super) fn map_native_phase_to_stage(obs: &JsonValue) -> Option<StartupStage> {
    match obs.get("phase").and_then(JsonValue::as_str) {
        Some("updater") => Some(StartupStage::LauncherPlay),
        Some("intro") => Some(StartupStage::Intro),
        Some("main_menu") => Some(StartupStage::MainMenu),
        Some("loading") => Some(StartupStage::CityLoading),
        Some("city") => Some(StartupStage::InCity),
        _ => None,
    }
}

/// Execute a typed [`NativeSessionRequest`] against live `SimCity.exe` with postcondition polling.
pub(super) fn execute_native_session(
    js_bridge: Option<&UiJsEvalFn>,
    game_pid: u32,
    request: NativeSessionRequest,
    observed_at_utc: &dyn Fn() -> String,
) -> Result<JsonValue, McpToolError> {
    let timeout_secs = u64::try_from(request.timeout_seconds.max(1)).unwrap_or(60);
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    if request.operation != NativeSessionOperation::Observe {
        let _ = eval_router_op(
            js_bridge,
            json!({ "op": "observe" }),
            DEFAULT_ROUTER_TIMEOUT_MS,
        )?;
    }

    if request.operation == NativeSessionOperation::LoadCity {
        await_frontend_ready(js_bridge, deadline)?;
    }

    let op_name = match request.operation {
        NativeSessionOperation::SkipIntro => "skip-intro",
        NativeSessionOperation::Observe => "observe",
        NativeSessionOperation::EnterMenu => "enter-menu",
        NativeSessionOperation::RegionTemplates => "region-templates",
        NativeSessionOperation::Regions => "regions",
        NativeSessionOperation::RegionDetails => "region-details",
        NativeSessionOperation::CreateSandboxRegion => "create-sandbox-region",
        NativeSessionOperation::ClaimCity => "claim-city",
        NativeSessionOperation::LoadCity => "load-city",
        NativeSessionOperation::InspectWorldUnits => "world-units",
        NativeSessionOperation::InspectWorldUnit => "world-unit",
    };

    let packet = json!({
        "op": op_name,
        "regionId": request.region_id,
        "cityId": request.city_id,
        "templateId": request.template_id,
        "regionName": request.name,
        "cityName": request.name,
        "argument": request.argument,
    });
    let initial_result = eval_router_op(js_bridge, packet, DEFAULT_ROUTER_TIMEOUT_MS)?;

    if !matches!(
        request.operation,
        NativeSessionOperation::EnterMenu | NativeSessionOperation::LoadCity
    ) {
        return Ok(json!({
            "result": {
                "processId": game_pid,
                "observedAtUtc": observed_at_utc(),
                "result": initial_result
            }
        }));
    }

    while Instant::now() < deadline {
        let Ok(observation) = eval_router_op(
            js_bridge,
            json!({ "op": "observe" }),
            DEFAULT_ROUTER_TIMEOUT_MS,
        ) else {
            thread::sleep(POLL_INTERVAL);
            continue;
        };
        let phase = observation
            .get("phase")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        if phase == "intro" {
            let _ = eval_router_op(
                js_bridge,
                json!({ "op": "skip-intro" }),
                DEFAULT_ROUTER_TIMEOUT_MS,
            );
            thread::sleep(POLL_INTERVAL);
            continue;
        }
        let completed = match request.operation {
            NativeSessionOperation::EnterMenu => matches!(phase, "main_menu" | "city"),
            NativeSessionOperation::LoadCity => is_loaded_city(&observation, &initial_result),
            _ => false,
        };
        if completed {
            return Ok(json!({
                "result": {
                    "completed": true,
                    "processId": game_pid,
                    "observedAtUtc": observed_at_utc(),
                    "result": observation
                }
            }));
        }
        thread::sleep(POLL_INTERVAL);
    }

    Err(McpToolError::public(
        "Native command was accepted but its requested menu/city postcondition was not observed before timeout. Inspect the current session before retrying.",
    ))
}

fn await_frontend_ready(
    js_bridge: Option<&UiJsEvalFn>,
    deadline: Instant,
) -> Result<(), McpToolError> {
    while Instant::now() < deadline {
        let Ok(state) = eval_router_op(
            js_bridge,
            json!({ "op": "observe" }),
            DEFAULT_ROUTER_TIMEOUT_MS,
        ) else {
            thread::sleep(POLL_INTERVAL);
            continue;
        };
        match state.get("phase").and_then(JsonValue::as_str) {
            Some("main_menu" | "city") => return Ok(()),
            Some("updater") => {
                return Err(McpToolError::public(
                    "Enter the game before requesting a city load.",
                ));
            }
            Some("intro") => {
                let _ = eval_router_op(
                    js_bridge,
                    json!({ "op": "skip-intro" }),
                    DEFAULT_ROUTER_TIMEOUT_MS,
                );
            }
            _ => {}
        }
        thread::sleep(POLL_INTERVAL);
    }
    Err(McpToolError::public(
        "Intro/preload did not complete; no city load command was published.",
    ))
}

fn is_loaded_city(observation: &JsonValue, expected: &JsonValue) -> bool {
    observation
        .get("loadingFinalized")
        .and_then(JsonValue::as_bool)
        == Some(true)
        && observation.get("cityId") == expected.get("cityId")
        && observation.get("cityUid") == expected.get("cityUid")
        && observation.get("regionId") == expected.get("regionId")
}

/// Extract sorted `unitRefs` array from a `unit-snapshot` reply.
pub(super) fn extract_unit_ref_set(snapshot: &JsonValue) -> BTreeSet<String> {
    snapshot
        .get("unitRefs")
        .and_then(JsonValue::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(JsonValue::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}
