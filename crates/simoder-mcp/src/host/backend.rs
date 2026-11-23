#![forbid(unsafe_code)]

//! Deterministic host backend implementation ([`StandardHostBackend`]) supporting contract verification,
//! `Simoder-OpenSCP` diagnostics, and live decoupled mod tool registration (`ADR-004`).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value as JsonValue, json};

use super::{
    BackendSimulationMode, ExternalModTool, ExternalModToolHandler, HostDispatchProvider,
    RecordedBackendCall, build_openscp_bridge_telemetry_snapshot,
    build_openscp_debug_tools_descriptor, decode_openscp_sldelta_v4_hex,
};
use crate::types::{
    AllowedKey, LaunchMode, McpToolError, NativeSessionOperation, NativeSessionRequest,
    RoadDrawType, StartupStage, ZoneType,
};

struct RegisteredExternalTool {
    meta: ExternalModTool,
    handler: ExternalModToolHandler,
}

/// Host backend implementation supporting both deterministic reference fixture verification
/// (`Simoder-DevTools-MCP#2`, `#4`) and live decoupled mod tool registration (`ADR-004`).
pub struct StandardHostBackend {
    mode: parking_lot::Mutex<BackendSimulationMode>,
    calls: parking_lot::Mutex<Vec<RecordedBackendCall>>,
    external_tools: parking_lot::Mutex<BTreeMap<String, RegisteredExternalTool>>,
    active_mod_generations: parking_lot::Mutex<BTreeMap<String, u64>>,
    game_pid: u32,
}

impl Default for StandardHostBackend {
    fn default() -> Self {
        Self::new(4242)
    }
}

impl StandardHostBackend {
    /// Fixed synthetic timestamp matching `Fixtures/game.json` (`2026-01-02T03:04:05+00:00`).
    pub const FIXTURE_TIME_UTC: &'static str = "2026-01-02T03:04:05+00:00";

    /// 1x1 transparent PNG base64 fixture for window capture testing.
    pub const FIXTURE_PNG_BASE64: &'static str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

    /// Construct a [`StandardHostBackend`] bound to `game_pid`.
    pub fn new(game_pid: u32) -> Self {
        Self {
            mode: parking_lot::Mutex::new(BackendSimulationMode::Success),
            calls: parking_lot::Mutex::new(Vec::new()),
            external_tools: parking_lot::Mutex::new(BTreeMap::new()),
            active_mod_generations: parking_lot::Mutex::new(BTreeMap::new()),
            game_pid,
        }
    }

    /// Set the [`BackendSimulationMode`] for testing error boundaries and partial mutation outcomes.
    pub fn set_mode(&self, mode: BackendSimulationMode) {
        *self.mode.lock() = mode;
    }

    /// Clear and return all recorded backend calls since the last drain.
    pub fn take_recorded_calls(&self) -> Vec<RecordedBackendCall> {
        std::mem::take(&mut *self.calls.lock())
    }

    /// Register a decoupled mod tool in the host backend (`ADR-004`).
    pub fn register_mod_tool(
        &self,
        mut tool: ExternalModTool,
        handler: ExternalModToolHandler,
    ) -> Result<String, String> {
        if tool.owner_mod_id.is_empty() || tool.owner_generation == 0 {
            return Err("invalid owner_mod_id or generation 0".into());
        }
        let prefix = format!("{}.", tool.owner_mod_id);
        let canonical = if tool.definition.name.starts_with(&prefix) {
            tool.definition.name.clone()
        } else {
            format!("{prefix}{}", tool.definition.name)
        };
        let mut tools = self.external_tools.lock();
        if tools.contains_key(&canonical) {
            return Err(format!("duplicate mod tool '{canonical}'"));
        }
        tool.definition.name = canonical.clone();
        self.active_mod_generations
            .lock()
            .insert(tool.owner_mod_id.clone(), tool.owner_generation);
        tools.insert(
            canonical.clone(),
            RegisteredExternalTool {
                meta: tool,
                handler,
            },
        );
        Ok(canonical)
    }

    /// Revoke all tools registered by `(mod_id, generation)` when that mod is disabled or reloaded.
    pub fn revoke_mod_generation(&self, mod_id: &str, generation: u64) -> usize {
        let mut active = self.active_mod_generations.lock();
        if active.get(mod_id).copied() == Some(generation) {
            active.remove(mod_id);
        }
        let mut tools = self.external_tools.lock();
        let before = tools.len();
        tools.retain(|_, t| {
            !(t.meta.owner_mod_id == mod_id && t.meta.owner_generation == generation)
        });
        before - tools.len()
    }

    fn record_and_check<T>(
        &self,
        route: &str,
        arguments: JsonValue,
        value: T,
    ) -> Result<T, McpToolError> {
        self.calls.lock().push(RecordedBackendCall {
            route: route.to_string(),
            arguments,
        });
        match *self.mode.lock() {
            BackendSimulationMode::Success | BackendSimulationMode::WithoutImage => Ok(value),
            BackendSimulationMode::McpError => Err(McpToolError::public(
                "synthetic_backend_denied: no operation committed",
            )),
            BackendSimulationMode::GenericError => Err(McpToolError::InternalMasked(
                "synthetic_sensitive_detail".into(),
            )),
            BackendSimulationMode::PartialCreate => Err(McpToolError::public(
                "Native game operation failed: {\"ok\":false,\"partial\":{\"createdRegionId\":\"1\"},\"error\":\"The created region did not confirm Sandbox; inspect its identity before retrying.\"}",
            )),
            BackendSimulationMode::PartialClaim => Err(McpToolError::public(
                "Native game operation failed: {\"ok\":false,\"partial\":{\"regionId\":\"1\",\"cityId\":\"2\",\"claimMayHaveCompleted\":true},\"error\":\"Native claim did not confirm ownership; do not retry implicitly.\"}",
            )),
            BackendSimulationMode::UnknownOutcome => Err(McpToolError::public(
                "Native request synthetic-request has no completed acknowledgement. Its outcome is unknown; inspect session/regions before retrying a mutation.",
            )),
            BackendSimulationMode::OversizedNativeError4100 => {
                Err(McpToolError::public("x".repeat(4100)))
            }
        }
    }

    fn fixture_session_state() -> JsonValue {
        json!({
            "sessionId": "synthetic-session",
            "target": {
                "mode": "play",
                "regionName": "Fixture region",
                "cityName": "Fixture city"
            },
            "stage": "inCity",
            "source": "nativeObservation",
            "confidence": 1,
            "humanAction": null,
            "updatedAtUtc": Self::FIXTURE_TIME_UTC,
            "processes": [],
            "windows": [],
            "timeline": []
        })
    }

    fn fixture_capture_metadata() -> JsonValue {
        json!({
            "captureId": "synthetic-capture",
            "capturedAtUtc": Self::FIXTURE_TIME_UTC,
            "window": {
                "handle": 0,
                "processId": 0,
                "processName": "fixture",
                "title": "Synthetic window",
                "left": 0,
                "top": 0,
                "width": 1,
                "height": 1,
                "clientLeft": 0,
                "clientTop": 0,
                "clientWidth": 1,
                "clientHeight": 1,
                "isForeground": false,
                "isAllowed": false
            },
            "accessibleControls": []
        })
    }
}

impl HostDispatchProvider for StandardHostBackend {
    fn native_session(&self, request: NativeSessionRequest) -> Result<JsonValue, McpToolError> {
        let observation = json!({
            "phase": "city",
            "loadingFinalized": true,
            "cityId": "2",
            "cityUid": "3",
            "regionId": "1"
        });
        let payload = match request.operation {
            NativeSessionOperation::Observe
            | NativeSessionOperation::EnterMenu
            | NativeSessionOperation::LoadCity => observation,
            NativeSessionOperation::SkipIntro => json!({
                "requested": true,
                "wasPlaying": false,
                "introPlaying": false
            }),
            NativeSessionOperation::RegionTemplates => json!({
                "regionTemplates": [{ "key": "1", "status": "ACTIVE" }]
            }),
            NativeSessionOperation::Regions => json!([]),
            NativeSessionOperation::RegionDetails | NativeSessionOperation::CreateSandboxRegion => {
                json!({
                    "id": "1",
                    "sandbox": true,
                    "boxes": [{ "id": "2", "uid": "3", "isClaimed": true }]
                })
            }
            NativeSessionOperation::ClaimCity => json!({
                "region": {
                    "id": "1",
                    "sandbox": true,
                    "boxes": [{ "id": "2", "uid": "3", "isClaimed": true }]
                },
                "city": { "id": "2", "uid": "3", "isClaimed": true }
            }),
            NativeSessionOperation::InspectWorldUnits => json!({
                "regionId": "1",
                "cityId": "2",
                "cityUid": "3",
                "total": 1,
                "offset": 0,
                "unitRefs": ["0,4294967295"],
                "nativeBindings": { "available": false }
            }),
            NativeSessionOperation::InspectWorldUnit => json!({
                "unitRef": "0,4294967295",
                "nativeUnitRef": "0,4294967295",
                "position": [0, 0, 0],
                "unitName": "synthetic-unit",
                "propKey": "synthetic-key"
            }),
        };
        let envelope = if matches!(
            request.operation,
            NativeSessionOperation::EnterMenu | NativeSessionOperation::LoadCity
        ) {
            json!({
                "completed": true,
                "processId": 0,
                "observedAtUtc": Self::FIXTURE_TIME_UTC,
                "result": payload
            })
        } else {
            json!({
                "processId": 0,
                "observedAtUtc": Self::FIXTURE_TIME_UTC,
                "result": payload
            })
        };
        let args = serde_json::to_value(&request).unwrap_or_else(|_| json!({}));
        self.record_and_check("NativeSession", args, json!({ "result": envelope }))
    }

    fn read_game_memory(&self, address: &str, length: i64) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "ReadGameMemory",
            json!({ "address": address, "length": length }),
            json!({
                "processId": 0,
                "address": address,
                "length": length,
                "hex": "00".repeat(length as usize),
                "sha256": "0".repeat(64),
                "readAtUtc": Self::FIXTURE_TIME_UTC
            }),
        )
    }

    fn get_game_modules(&self) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "GetGameModules",
            json!({}),
            json!({
                "result": {
                    "processId": 0,
                    "observedAtUtc": Self::FIXTURE_TIME_UTC,
                    "modules": [{
                        "name": "synthetic.dll",
                        "path": "synthetic.dll",
                        "baseAddress": "0x400000",
                        "imageSize": 4096
                    }]
                }
            }),
        )
    }

    fn export_game_code(&self) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "ExportGameCode",
            json!({}),
            json!({
                "result": {
                    "schemaVersion": 1,
                    "processId": 0,
                    "capturedAtUtc": Self::FIXTURE_TIME_UTC,
                    "sourcePath": "synthetic.exe",
                    "sourceSha256": "synthetic-digest",
                    "moduleBase": "0x400000",
                    "artifactPath": "synthetic.bin",
                    "artifactSha256": "synthetic-digest",
                    "provenancePath": "synthetic.json",
                    "format": "pe",
                    "architecture": "x86",
                    "sections": [{
                        "name": ".text",
                        "rva": "0x1000",
                        "fileOffset": 512,
                        "length": 1,
                        "sha256": "synthetic-digest",
                        "differsFromDisk": false
                    }],
                    "limitations": [
                        "Analysis PE only; not a repaired runnable executable.",
                        "Only executable sections contain observed runtime bytes; other sections retain disk bytes.",
                        "Includes existing hooks; imports, object lifetimes and calling conventions remain unverified.",
                        "No writable runtime data or heap exported; section stability is not a whole-process atomic snapshot."
                    ]
                }
            }),
        )
    }

    fn set_mod_enabled(&self, mod_id: &str, enabled: bool) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "SetModEnabled",
            json!({ "modId": mod_id, "enabled": enabled }),
            json!({
                "result": {
                    "requestId": "synthetic-request",
                    "pid": 0,
                    "success": true,
                    "modId": mod_id,
                    "enabled": enabled,
                    "generation": 1,
                    "error": ""
                }
            }),
        )
    }

    fn close_game(&self) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "CloseGame",
            json!({}),
            json!({
                "result": {
                    "requested": true,
                    "status": "close_queued",
                    "processId": 0,
                    "note": "The game may require save or exit confirmation. A queued request does not prove exit; no force termination is used."
                }
            }),
        )
    }

    fn start_session(
        &self,
        mode: LaunchMode,
        region_name: Option<&str>,
        city_name: Option<&str>,
    ) -> Result<JsonValue, McpToolError> {
        let mode_str: &'static str = mode.into();
        self.record_and_check(
            "StartSession",
            json!({
                "mode": mode_str,
                "regionName": region_name,
                "cityName": city_name
            }),
            Self::fixture_session_state(),
        )
    }

    fn get_startup_state(&self) -> Result<JsonValue, McpToolError> {
        self.record_and_check("GetState", json!({}), Self::fixture_session_state())
    }

    fn capture_window(&self, target: &str) -> Result<(JsonValue, String), McpToolError> {
        self.record_and_check(
            "CaptureWindow",
            json!({ "target": target }),
            (
                Self::fixture_capture_metadata(),
                Self::FIXTURE_PNG_BASE64.to_string(),
            ),
        )
    }

    fn click(
        &self,
        capture_id: &str,
        normalized_x: f64,
        normalized_y: f64,
    ) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "Click",
            json!({
                "captureId": capture_id,
                "normalizedX": normalized_x,
                "normalizedY": normalized_y
            }),
            json!({
                "result": {
                    "accepted": true,
                    "mechanism": "synthetic-input-path"
                }
            }),
        )
    }

    fn send_key(&self, target: &str, key: AllowedKey) -> Result<JsonValue, McpToolError> {
        let key_str: &'static str = key.into();
        self.record_and_check(
            "SendKey",
            json!({ "target": target, "key": key_str }),
            json!({
                "result": {
                    "accepted": true,
                    "key": key_str.to_ascii_lowercase(),
                    "target": target
                }
            }),
        )
    }

    fn confirm_stage(
        &self,
        capture_id: &str,
        stage: StartupStage,
        confidence: f64,
        note: Option<&str>,
    ) -> Result<JsonValue, McpToolError> {
        let stage_str: &'static str = stage.into();
        self.record_and_check(
            "ConfirmStage",
            json!({
                "captureId": capture_id,
                "stage": stage_str,
                "confidence": confidence,
                "note": note
            }),
            Self::fixture_session_state(),
        )
    }

    fn wait_for_stage(
        &self,
        stage: StartupStage,
        timeout_seconds: i64,
    ) -> Result<JsonValue, McpToolError> {
        let stage_str: &'static str = stage.into();
        self.record_and_check(
            "WaitForStage",
            json!({
                "stage": stage_str,
                "timeoutSeconds": timeout_seconds
            }),
            Self::fixture_session_state(),
        )
    }

    fn get_diagnostics(&self) -> Result<(JsonValue, Option<String>), McpToolError> {
        let without_image = *self.mode.lock() == BackendSimulationMode::WithoutImage;
        let last_capture = if without_image {
            JsonValue::Null
        } else {
            Self::fixture_capture_metadata()
        };
        let png = if without_image {
            None
        } else {
            Some(Self::FIXTURE_PNG_BASE64.to_string())
        };
        self.record_and_check(
            "GetDiagnostics",
            json!({}),
            (
                json!({
                    "state": Self::fixture_session_state(),
                    "overlayModules": ["synthetic-module"],
                    "recentLogLines": ["synthetic-log"],
                    "lastCapture": last_capture,
                    "generatedAtUtc": Self::FIXTURE_TIME_UTC
                }),
                png,
            ),
        )
    }

    fn get_runtime_state(&self) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "GetRuntimeEvidence",
            json!({}),
            json!({
                "result": {
                    "available": false,
                    "fresh": false,
                    "message": "synthetic unavailable evidence",
                    "snapshot": null,
                    "readAtUtc": Self::FIXTURE_TIME_UTC
                }
            }),
        )
    }

    fn get_logs(&self, mod_id: Option<&str>, limit: i64) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "GetModLogs",
            json!({ "modId": mod_id, "limit": limit }),
            json!({
                "result": {
                    "lines": ["synthetic-log"],
                    "modId": mod_id,
                    "readAtUtc": Self::FIXTURE_TIME_UTC
                }
            }),
        )
    }

    fn get_capabilities(&self) -> JsonValue {
        json!({
            "result": {
                "schemaVersion": 2,
                "nativeModDll": true,
                "hostAbiVersion": 2,
                "gamePid": self.game_pid,
                "loaderControlReady": true,
                "openScpExtensions": [
                    "simoder_unlock_debug_tools",
                    "simoder_get_bridge_telemetry",
                    "simoder_inspect_sldelta",
                    "simoder_eval_ui_bridge"
                ],
                "decoupledModToolsRegistered": self.list_external_mod_tools().len()
            }
        })
    }

    fn get_service_info(&self) -> JsonValue {
        json!({
            "result": {
                "serverPid": self.game_pid,
                "gamePid": self.game_pid,
                "transport": "native-mod-dll-abi-v2"
            }
        })
    }

    fn unlock_debug_tools(
        &self,
        category: Option<u32>,
        ui_category: Option<u32>,
    ) -> Result<JsonValue, McpToolError> {
        let (cat, ui_cat, payload) = build_openscp_debug_tools_descriptor(category, ui_category);
        self.record_and_check(
            "OpenScpUnlockDebugTools",
            json!({ "category": cat, "uiCategory": ui_cat }),
            payload,
        )
    }

    fn get_bridge_telemetry(&self, channel: Option<&str>) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "OpenScpGetBridgeTelemetry",
            json!({ "channel": channel }),
            build_openscp_bridge_telemetry_snapshot(channel),
        )
    }

    fn inspect_sldelta(&self, payload_hex: &str) -> Result<JsonValue, McpToolError> {
        let (byte_len, payload) = decode_openscp_sldelta_v4_hex(payload_hex)?;
        self.record_and_check(
            "OpenScpInspectSlDelta",
            json!({ "byteLength": byte_len }),
            payload,
        )
    }

    fn eval_ui_bridge(&self, method: &str, verb: &str) -> Result<JsonValue, McpToolError> {
        if !method.eq_ignore_ascii_case("RequestGameData")
            && !method.eq_ignore_ascii_case("PostGameCommand")
        {
            return Err(McpToolError::public(
                "method must be 'RequestGameData' or 'PostGameCommand'",
            ));
        }
        if verb.trim().is_empty() {
            return Err(McpToolError::public("bridge verb cannot be empty"));
        }
        self.record_and_check(
            "OpenScpEvalUiBridge",
            json!({ "method": method, "verb": verb }),
            json!({
                "result": {
                    "dispatched": true,
                    "method": method,
                    "verb": verb
                }
            }),
        )
    }

    fn get_build_tools(&self, category: Option<&str>) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "GetBuildTools",
            json!({ "category": category }),
            json!({
                "result": {
                    "selectedToolId": null,
                    "categories": [
                        { "label": "roads", "categoryId": 1577763374_u32 },
                        { "label": "zones", "categoryId": 831766718_u32 }
                    ],
                    "roadDrawTypes": { "curvy": 0, "straight": 1, "grid": 2, "arc": 3, "circle": 4 }
                }
            }),
        )
    }

    fn select_tool(
        &self,
        tool_id: Option<u32>,
        deselect: bool,
        road_draw_type: Option<RoadDrawType>,
    ) -> Result<JsonValue, McpToolError> {
        let mode_str: Option<&'static str> = road_draw_type.map(Into::into);
        self.record_and_check(
            "SelectTool",
            json!({
                "toolId": tool_id,
                "deselect": deselect,
                "roadDrawType": mode_str
            }),
            json!({
                "result": {
                    "requestedToolId": tool_id,
                    "deselected": deselect && tool_id.is_none(),
                    "selectedToolId": if deselect && tool_id.is_none() { JsonValue::Null } else { json!(tool_id) },
                    "roadDrawType": mode_str
                }
            }),
        )
    }

    fn move_camera(
        &self,
        x: f64,
        y: f64,
        z: f64,
        rotate: Option<f64>,
    ) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "MoveCamera",
            json!({ "x": x, "y": y, "z": z, "rotate": rotate }),
            json!({
                "result": {
                    "moved": true,
                    "position": [x, y, z],
                    "rotate": rotate
                }
            }),
        )
    }

    fn build_road(
        &self,
        tool_id: Option<u32>,
        draw_type: RoadDrawType,
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        steps: u32,
    ) -> Result<JsonValue, McpToolError> {
        let draw_str: &'static str = draw_type.into();
        self.record_and_check(
            "BuildRoad",
            json!({
                "toolId": tool_id,
                "drawType": draw_str,
                "startX": start_x,
                "startY": start_y,
                "endX": end_x,
                "endY": end_y,
                "steps": steps
            }),
            json!({
                "result": {
                    "completed": true,
                    "drawType": draw_str,
                    "toolId": tool_id,
                    "newUnits": []
                }
            }),
        )
    }

    fn assign_zone(
        &self,
        zone_type: ZoneType,
        tool_id: Option<u32>,
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        steps: u32,
    ) -> Result<JsonValue, McpToolError> {
        let zone_str: &'static str = zone_type.into();
        self.record_and_check(
            "AssignZone",
            json!({
                "zoneType": zone_str,
                "toolId": tool_id,
                "startX": start_x,
                "startY": start_y,
                "endX": end_x,
                "endY": end_y,
                "steps": steps
            }),
            json!({
                "result": {
                    "completed": true,
                    "zoneType": zone_str,
                    "toolId": tool_id
                }
            }),
        )
    }

    fn place_building(
        &self,
        tool_id: u32,
        normalized_x: f64,
        normalized_y: f64,
        rotate_steps: i32,
    ) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "PlaceBuilding",
            json!({
                "toolId": tool_id,
                "normalizedX": normalized_x,
                "normalizedY": normalized_y,
                "rotateSteps": rotate_steps
            }),
            json!({
                "result": {
                    "completed": true,
                    "toolId": tool_id,
                    "rotateSteps": rotate_steps,
                    "newUnits": []
                }
            }),
        )
    }

    fn inspect_ui_tree(
        &self,
        control_id: Option<u32>,
        max_depth: u32,
        visible_only: bool,
        max_nodes: u32,
    ) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "InspectUiTree",
            json!({
                "controlId": control_id,
                "maxDepth": max_depth,
                "visibleOnly": visible_only,
                "maxNodes": max_nodes
            }),
            json!({
                "result": {
                    "viewport": { "width": 1280, "height": 720 },
                    "rootControlId": control_id.unwrap_or(241896001),
                    "nodeCount": 1,
                    "truncated": false,
                    "nodes": [
                        {
                            "depth": 0,
                            "controlId": control_id.unwrap_or(241896001),
                            "visible": true,
                            "width": 300,
                            "height": 201
                        }
                    ]
                }
            }),
        )
    }

    fn query_ui_dom(
        &self,
        selector: &str,
        limit: u32,
        include_computed_style: bool,
    ) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "QueryUiDom",
            json!({
                "selector": selector,
                "limit": limit,
                "includeComputedStyle": include_computed_style
            }),
            json!({
                "result": {
                    "viewport": { "width": 1280, "height": 720 },
                    "selector": selector,
                    "matchedCount": 0,
                    "elements": []
                }
            }),
        )
    }

    fn get_ui_state(&self) -> Result<JsonValue, McpToolError> {
        self.record_and_check(
            "GetSimoderUiState",
            json!({}),
            json!({
                "result": {
                    "viewport": { "width": 1280, "height": 720 },
                    "observation": { "phase": "main_menu" },
                    "stylesInjected": true,
                    "slots": [],
                    "patches": []
                }
            }),
        )
    }

    fn list_external_mod_tools(&self) -> Vec<ExternalModTool> {
        let active = self.active_mod_generations.lock();
        self.external_tools
            .lock()
            .values()
            .filter(|t| active.get(&t.meta.owner_mod_id).copied() == Some(t.meta.owner_generation))
            .map(|t| t.meta.clone())
            .collect()
    }

    fn invoke_external_mod_tool(
        &self,
        tool_name: &str,
        arguments: &JsonValue,
    ) -> Result<JsonValue, McpToolError> {
        let active = self.active_mod_generations.lock();
        let tools = self.external_tools.lock();
        let Some(entry) = tools.get(tool_name) else {
            return Err(McpToolError::public(format!(
                "unknown or unregistered tool '{tool_name}'"
            )));
        };
        if active.get(&entry.meta.owner_mod_id).copied() != Some(entry.meta.owner_generation) {
            return Err(McpToolError::public(format!(
                "tool '{tool_name}' belongs to a stale unloaded mod generation"
            )));
        }
        let handler = Arc::clone(&entry.handler);
        drop(tools);
        drop(active);
        let payload = handler(arguments)?;
        Ok(json!({ "result": payload }))
    }
}
