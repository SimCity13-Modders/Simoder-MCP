#![forbid(unsafe_code)]

//! Built-in MCP tool catalog (34 simulation/session/builder tools + 4 `Simoder-OpenSCP` & telemetry tools),
//! JSON schemas, required permission grants, and strict argument validators (`Simoder-DevTools-MCP#2`).

pub mod definitions;

use serde::Serialize;
use serde_json::Value as JsonValue;
use serde_with::DeserializeFromStr;
use strum::{EnumString, IntoStaticStr, VariantArray};

use crate::auth::McpGrant;
use crate::config::{
    DEFAULT_LOG_TAIL_LINES, MAX_LOG_TAIL_LINES, MAX_MEMORY_READ_BYTES, MAX_WAIT_STAGE_TIMEOUT_SECS,
    MAX_WORLD_UNIT_LIMIT, MAX_WORLD_UNIT_OFFSET, MIN_MEMORY_READ_BYTES,
    MIN_WAIT_STAGE_TIMEOUT_SECS, MIN_WORLD_UNIT_LIMIT,
};
use crate::host::HostDispatchProvider;
use crate::types::{
    AllowedKey, CallToolResult, LaunchMode, McpToolError, NativeSessionOperation,
    NativeSessionRequest, RoadDrawType, StartupStage, ZoneType,
};

/// Single-source enumeration of all built-in tools served by `simoder-mcp` (`ADR-003`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    DeserializeFromStr,
    EnumString,
    IntoStaticStr,
    VariantArray,
)]
pub enum BuiltinToolId {
    /// `simcity_assign_zone`
    #[strum(to_string = "simcity_assign_zone")]
    SimCityAssignZone,
    /// `simcity_build_road`
    #[strum(to_string = "simcity_build_road")]
    SimCityBuildRoad,
    /// `simcity_capture_window`
    #[strum(to_string = "simcity_capture_window")]
    SimCityCaptureWindow,
    /// `simcity_claim_city`
    #[strum(to_string = "simcity_claim_city")]
    SimCityClaimCity,
    /// `simcity_click`
    #[strum(to_string = "simcity_click")]
    SimCityClick,
    /// `simcity_close_game`
    #[strum(to_string = "simcity_close_game")]
    SimCityCloseGame,
    /// `simcity_confirm_stage`
    #[strum(to_string = "simcity_confirm_stage")]
    SimCityConfirmStage,
    /// `simcity_create_sandbox_region`
    #[strum(to_string = "simcity_create_sandbox_region")]
    SimCityCreateSandboxRegion,
    /// `simcity_enter_game`
    #[strum(to_string = "simcity_enter_game")]
    SimCityEnterGame,
    /// `simcity_get_build_tools`
    #[strum(to_string = "simcity_get_build_tools")]
    SimCityGetBuildTools,
    /// `simcity_get_diagnostics`
    #[strum(to_string = "simcity_get_diagnostics")]
    SimCityGetDiagnostics,
    /// `simcity_get_native_session`
    #[strum(to_string = "simcity_get_native_session")]
    SimCityGetNativeSession,
    /// `simcity_get_region_details`
    #[strum(to_string = "simcity_get_region_details")]
    SimCityGetRegionDetails,
    /// `simcity_get_region_templates`
    #[strum(to_string = "simcity_get_region_templates")]
    SimCityGetRegionTemplates,
    /// `simcity_get_regions`
    #[strum(to_string = "simcity_get_regions")]
    SimCityGetRegions,
    /// `simcity_get_startup_state`
    #[strum(to_string = "simcity_get_startup_state")]
    SimCityGetStartupState,
    /// `simcity_get_world_unit`
    #[strum(to_string = "simcity_get_world_unit")]
    SimCityGetWorldUnit,
    /// `simcity_get_world_units`
    #[strum(to_string = "simcity_get_world_units")]
    SimCityGetWorldUnits,
    /// `simcity_inspect_ui_tree`
    #[strum(to_string = "simcity_inspect_ui_tree")]
    SimCityInspectUiTree,
    /// `simcity_load_city`
    #[strum(to_string = "simcity_load_city")]
    SimCityLoadCity,
    /// `simcity_move_camera`
    #[strum(to_string = "simcity_move_camera")]
    SimCityMoveCamera,
    /// `simcity_place_building`
    #[strum(to_string = "simcity_place_building")]
    SimCityPlaceBuilding,
    /// `simcity_query_ui_dom`
    #[strum(to_string = "simcity_query_ui_dom")]
    SimCityQueryUiDom,
    /// `simcity_select_tool`
    #[strum(to_string = "simcity_select_tool")]
    SimCitySelectTool,
    /// `simcity_send_key`
    #[strum(to_string = "simcity_send_key")]
    SimCitySendKey,
    /// `simcity_skip_intro`
    #[strum(to_string = "simcity_skip_intro")]
    SimCitySkipIntro,
    /// `simcity_start_session`
    #[strum(to_string = "simcity_start_session")]
    SimCityStartSession,
    /// `simcity_wait_for_stage`
    #[strum(to_string = "simcity_wait_for_stage")]
    SimCityWaitForStage,
    /// `simoder_eval_ui_bridge` (`Simoder-OpenSCP` extension)
    #[strum(to_string = "simoder_eval_ui_bridge")]
    SimoderEvalUiBridge,
    /// `simoder_export_game_code`
    #[strum(to_string = "simoder_export_game_code")]
    SimoderExportGameCode,
    /// `simoder_get_bridge_telemetry` (`Simoder-OpenSCP` extension)
    #[strum(to_string = "simoder_get_bridge_telemetry")]
    SimoderGetBridgeTelemetry,
    /// `simoder_get_capabilities`
    #[strum(to_string = "simoder_get_capabilities")]
    SimoderGetCapabilities,
    /// `simoder_get_game_modules`
    #[strum(to_string = "simoder_get_game_modules")]
    SimoderGetGameModules,
    /// `simoder_get_logs`
    #[strum(to_string = "simoder_get_logs")]
    SimoderGetLogs,
    /// `simoder_get_runtime_state`
    #[strum(to_string = "simoder_get_runtime_state")]
    SimoderGetRuntimeState,
    /// `simoder_get_service_info`
    #[strum(to_string = "simoder_get_service_info")]
    SimoderGetServiceInfo,
    /// `simoder_get_ui_state`
    #[strum(to_string = "simoder_get_ui_state")]
    SimoderGetUiState,
    /// `simoder_inspect_sldelta` (`Simoder-OpenSCP` extension)
    #[strum(to_string = "simoder_inspect_sldelta")]
    SimoderInspectSlDelta,
    /// `simoder_read_game_memory`
    #[strum(to_string = "simoder_read_game_memory")]
    SimoderReadGameMemory,
    /// `simoder_set_mod_enabled`
    #[strum(to_string = "simoder_set_mod_enabled")]
    SimoderSetModEnabled,
    /// `simoder_unlock_debug_tools` (`Simoder-OpenSCP` extension)
    #[strum(to_string = "simoder_unlock_debug_tools")]
    SimoderUnlockDebugTools,
}

impl BuiltinToolId {
    /// Return the canonical tool name string.
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// Return the required [`McpGrant`] permission for invoking this built-in tool.
    pub const fn required_grant(self) -> McpGrant {
        match self {
            Self::SimCityGetDiagnostics
            | Self::SimCityGetNativeSession
            | Self::SimCityGetRegionDetails
            | Self::SimCityGetRegionTemplates
            | Self::SimCityGetRegions
            | Self::SimCityGetStartupState
            | Self::SimCityInspectUiTree
            | Self::SimCityQueryUiDom
            | Self::SimCityConfirmStage
            | Self::SimCityWaitForStage
            | Self::SimoderGetCapabilities
            | Self::SimoderGetServiceInfo
            | Self::SimoderGetUiState => McpGrant::SessionRead,

            Self::SimCityClaimCity
            | Self::SimCityCloseGame
            | Self::SimCityCreateSandboxRegion
            | Self::SimCityEnterGame
            | Self::SimCityLoadCity
            | Self::SimCitySkipIntro
            | Self::SimCityStartSession
            | Self::SimCitySelectTool
            | Self::SimCityBuildRoad
            | Self::SimCityAssignZone
            | Self::SimCityPlaceBuilding => McpGrant::SessionMutate,

            Self::SimCityGetWorldUnit
            | Self::SimCityGetWorldUnits
            | Self::SimCityGetBuildTools => McpGrant::WorldInspect,

            Self::SimoderGetLogs | Self::SimoderGetRuntimeState | Self::SimoderSetModEnabled => {
                McpGrant::ModsControl
            }

            Self::SimoderExportGameCode
            | Self::SimoderGetGameModules
            | Self::SimoderReadGameMemory => McpGrant::MemoryAnalysis,

            Self::SimCityCaptureWindow
            | Self::SimCityClick
            | Self::SimCitySendKey
            | Self::SimCityMoveCamera => McpGrant::UiInput,

            Self::SimoderEvalUiBridge
            | Self::SimoderGetBridgeTelemetry
            | Self::SimoderInspectSlDelta
            | Self::SimoderUnlockDebugTools => McpGrant::OpenScpDiagnostics,
        }
    }

    /// Validate arguments and dispatch this built-in tool against `host`.
    pub fn execute(
        self,
        args: &JsonValue,
        host: &dyn HostDispatchProvider,
    ) -> Result<CallToolResult, McpToolError> {
        match self {
            Self::SimoderGetCapabilities => Ok(CallToolResult::success(host.get_capabilities())),
            Self::SimoderGetServiceInfo => Ok(CallToolResult::success(host.get_service_info())),
            Self::SimoderGetUiState => host.get_ui_state().map(CallToolResult::success),
            Self::SimCityInspectUiTree => {
                let control_id = args
                    .get("controlId")
                    .and_then(JsonValue::as_u64)
                    .and_then(|v| u32::try_from(v).ok());
                let max_depth = args
                    .get("maxDepth")
                    .and_then(JsonValue::as_i64)
                    .unwrap_or(3)
                    .clamp(0, 8) as u32;
                let visible_only = args
                    .get("visibleOnly")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(true);
                let max_nodes = args
                    .get("maxNodes")
                    .and_then(JsonValue::as_i64)
                    .unwrap_or(120)
                    .clamp(1, 300) as u32;
                host.inspect_ui_tree(control_id, max_depth, visible_only, max_nodes)
                    .map(CallToolResult::success)
            }
            Self::SimCityQueryUiDom => {
                let selector = require_str(args, "selector")?.trim();
                if selector.is_empty() || selector.len() > 256 {
                    return Err(McpToolError::public(
                        "selector must be 1..=256 characters",
                    ));
                }
                let limit = args
                    .get("limit")
                    .and_then(JsonValue::as_i64)
                    .unwrap_or(32)
                    .clamp(1, 100) as u32;
                let include_computed_style = args
                    .get("includeComputedStyle")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(true);
                host.query_ui_dom(selector, limit, include_computed_style)
                    .map(CallToolResult::success)
            }
            Self::SimCityGetStartupState => {
                host.get_startup_state().map(CallToolResult::success)
            }
            Self::SimCityCloseGame => host.close_game().map(CallToolResult::success),
            Self::SimoderGetRuntimeState => host.get_runtime_state().map(CallToolResult::success),
            Self::SimoderGetGameModules => host.get_game_modules().map(CallToolResult::success),
            Self::SimoderExportGameCode => host.export_game_code().map(CallToolResult::success),
            Self::SimCitySkipIntro => host
                .native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::SkipIntro,
                    region_id: None,
                    city_id: None,
                    template_id: None,
                    name: None,
                    argument: None,
                    timeout_seconds: 60,
                })
                .map(CallToolResult::success),
            Self::SimCityGetNativeSession => host
                .native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::Observe,
                    region_id: None,
                    city_id: None,
                    template_id: None,
                    name: None,
                    argument: None,
                    timeout_seconds: 60,
                })
                .map(CallToolResult::success),
            Self::SimCityEnterGame => {
                let timeout = args
                    .get("timeoutSeconds")
                    .and_then(JsonValue::as_i64)
                    .unwrap_or(60) as i32;
                host.native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::EnterMenu,
                    region_id: None,
                    city_id: None,
                    template_id: None,
                    name: None,
                    argument: None,
                    timeout_seconds: timeout,
                })
                .map(CallToolResult::success)
            }
            Self::SimCityGetRegionTemplates => host
                .native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::RegionTemplates,
                    region_id: None,
                    city_id: None,
                    template_id: None,
                    name: None,
                    argument: None,
                    timeout_seconds: 60,
                })
                .map(CallToolResult::success),
            Self::SimCityGetRegions => host
                .native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::Regions,
                    region_id: None,
                    city_id: None,
                    template_id: None,
                    name: None,
                    argument: None,
                    timeout_seconds: 60,
                })
                .map(CallToolResult::success),
            Self::SimCityGetRegionDetails => {
                let region_id = require_u32(args, "regionId")?;
                host.native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::RegionDetails,
                    region_id: Some(region_id),
                    city_id: None,
                    template_id: None,
                    name: None,
                    argument: None,
                    timeout_seconds: 60,
                })
                .map(CallToolResult::success)
            }
            Self::SimCityCreateSandboxRegion => {
                let template_id = require_u32(args, "templateId")?;
                let region_name = require_str(args, "regionName")?.to_string();
                host.native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::CreateSandboxRegion,
                    region_id: None,
                    city_id: None,
                    template_id: Some(template_id),
                    name: Some(region_name),
                    argument: None,
                    timeout_seconds: 60,
                })
                .map(CallToolResult::success)
            }
            Self::SimCityClaimCity => {
                let region_id = require_u32(args, "regionId")?;
                let city_id = require_u32(args, "cityId")?;
                let city_name = require_str(args, "cityName")?.to_string();
                host.native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::ClaimCity,
                    region_id: Some(region_id),
                    city_id: Some(city_id),
                    template_id: None,
                    name: Some(city_name),
                    argument: None,
                    timeout_seconds: 60,
                })
                .map(CallToolResult::success)
            }
            Self::SimCityLoadCity => {
                let region_id = require_u32(args, "regionId")?;
                let city_id = require_u32(args, "cityId")?;
                let timeout = args
                    .get("timeoutSeconds")
                    .and_then(JsonValue::as_i64)
                    .unwrap_or(90) as i32;
                host.native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::LoadCity,
                    region_id: Some(region_id),
                    city_id: Some(city_id),
                    template_id: None,
                    name: None,
                    argument: None,
                    timeout_seconds: timeout,
                })
                .map(CallToolResult::success)
            }
            Self::SimCityGetWorldUnits => {
                let region_id = require_u32(args, "regionId")?;
                let city_id = require_u32(args, "cityId")?;
                let offset = args.get("offset").and_then(JsonValue::as_i64).unwrap_or(0);
                let limit = args.get("limit").and_then(JsonValue::as_i64).unwrap_or(32);
                validate_world_pagination(offset, limit)?;
                host.native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::InspectWorldUnits,
                    region_id: Some(region_id),
                    city_id: Some(city_id),
                    template_id: None,
                    name: None,
                    argument: Some(format!("{offset},{limit}")),
                    timeout_seconds: 60,
                })
                .map(CallToolResult::success)
            }
            Self::SimCityGetWorldUnit => {
                let region_id = require_u32(args, "regionId")?;
                let city_id = require_u32(args, "cityId")?;
                let unit_ref = require_str(args, "unitRef")?;
                validate_canonical_unit_ref(unit_ref)?;
                host.native_session(NativeSessionRequest {
                    operation: NativeSessionOperation::InspectWorldUnit,
                    region_id: Some(region_id),
                    city_id: Some(city_id),
                    template_id: None,
                    name: None,
                    argument: Some(unit_ref.to_string()),
                    timeout_seconds: 60,
                })
                .map(CallToolResult::success)
            }
            Self::SimCityStartSession => {
                let raw_mode = require_str(args, "mode")?;
                let mode: LaunchMode = raw_mode
                    .parse()
                    .map_err(|_| McpToolError::public(format!("invalid LaunchMode '{raw_mode}'")))?;
                let region_name = args.get("regionName").and_then(JsonValue::as_str);
                let city_name = args.get("cityName").and_then(JsonValue::as_str);
                host.start_session(mode, region_name, city_name)
                    .map(CallToolResult::success)
            }
            Self::SimCityCaptureWindow => {
                let target = args
                    .get("target")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("auto");
                let (metadata, png_b64) = host.capture_window(target)?;
                Ok(CallToolResult::success_with_png(metadata, png_b64))
            }
            Self::SimCityClick => {
                let capture_id = require_str(args, "captureId")?;
                let nx = require_f64(args, "normalizedX")?;
                let ny = require_f64(args, "normalizedY")?;
                host.click(capture_id, nx, ny).map(CallToolResult::success)
            }
            Self::SimCitySendKey => {
                let target = require_str(args, "target")?;
                let raw_key = require_str(args, "key")?;
                let key: AllowedKey = raw_key
                    .parse()
                    .map_err(|_| McpToolError::public(format!("disallowed key '{raw_key}'")))?;
                host.send_key(target, key).map(CallToolResult::success)
            }
            Self::SimCityConfirmStage => {
                let capture_id = require_str(args, "captureId")?;
                let raw_stage = require_str(args, "stage")?;
                let stage: StartupStage = raw_stage
                    .parse()
                    .map_err(|_| McpToolError::public(format!("invalid stage '{raw_stage}'")))?;
                let confidence = require_f64(args, "confidence")?;
                let note = args.get("note").and_then(JsonValue::as_str);
                host.confirm_stage(capture_id, stage, confidence, note)
                    .map(CallToolResult::success)
            }
            Self::SimCityWaitForStage => {
                let raw_stage = require_str(args, "stage")?;
                let stage: StartupStage = raw_stage
                    .parse()
                    .map_err(|_| McpToolError::public(format!("invalid stage '{raw_stage}'")))?;
                let timeout = require_i64(args, "timeoutSeconds")?;
                if !(MIN_WAIT_STAGE_TIMEOUT_SECS..=MAX_WAIT_STAGE_TIMEOUT_SECS).contains(&timeout) {
                    return Err(McpToolError::public(
                        "Use timeoutSeconds between 1 and 300.",
                    ));
                }
                host.wait_for_stage(stage, timeout)
                    .map(CallToolResult::success)
            }
            Self::SimCityGetDiagnostics => {
                let (metadata, maybe_png) = host.get_diagnostics()?;
                match maybe_png {
                    Some(png_b64) => Ok(CallToolResult::success_with_png(metadata, png_b64)),
                    None => Ok(CallToolResult::success(metadata)),
                }
            }
            Self::SimoderSetModEnabled => {
                let mod_id = require_str(args, "modId")?;
                let enabled = require_bool(args, "enabled")?;
                host.set_mod_enabled(mod_id, enabled)
                    .map(CallToolResult::success)
            }
            Self::SimoderGetLogs => {
                let mod_id = args.get("modId").and_then(JsonValue::as_str);
                let limit = args
                    .get("limit")
                    .and_then(JsonValue::as_i64)
                    .unwrap_or(DEFAULT_LOG_TAIL_LINES)
                    .clamp(1, MAX_LOG_TAIL_LINES);
                host.get_logs(mod_id, limit).map(CallToolResult::success)
            }
            Self::SimoderReadGameMemory => {
                let address = require_str(args, "address")?;
                let length = require_i64(args, "length")?;
                if !(MIN_MEMORY_READ_BYTES..=MAX_MEMORY_READ_BYTES).contains(&length) {
                    return Err(McpToolError::public("Use length 1..65536."));
                }
                host.read_game_memory(address, length)
                    .map(CallToolResult::success)
            }
            Self::SimoderUnlockDebugTools => {
                let category = args
                    .get("category")
                    .and_then(JsonValue::as_u64)
                    .and_then(|v| u32::try_from(v).ok());
                let ui_category = args
                    .get("uiCategory")
                    .and_then(JsonValue::as_u64)
                    .and_then(|v| u32::try_from(v).ok());
                host.unlock_debug_tools(category, ui_category)
                    .map(CallToolResult::success)
            }
            Self::SimoderGetBridgeTelemetry => {
                let channel = args.get("channel").and_then(JsonValue::as_str);
                host.get_bridge_telemetry(channel)
                    .map(CallToolResult::success)
            }
            Self::SimoderInspectSlDelta => {
                let payload_hex = require_str(args, "payloadHex")?;
                host.inspect_sldelta(payload_hex)
                    .map(CallToolResult::success)
            }
            Self::SimoderEvalUiBridge => {
                let method = require_str(args, "method")?;
                let verb = require_str(args, "verb")?;
                host.eval_ui_bridge(method, verb)
                    .map(CallToolResult::success)
            }
            Self::SimCityGetBuildTools => {
                let category = args.get("category").and_then(JsonValue::as_str);
                host.get_build_tools(category).map(CallToolResult::success)
            }
            Self::SimCitySelectTool => {
                let tool_id = args
                    .get("toolId")
                    .and_then(JsonValue::as_u64)
                    .and_then(|v| u32::try_from(v).ok());
                let deselect = args
                    .get("deselect")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false);
                let road_draw_type = match args.get("roadDrawType").and_then(JsonValue::as_str) {
                    Some(raw) => Some(raw.parse::<RoadDrawType>().map_err(|_| {
                        McpToolError::public(format!("invalid roadDrawType '{raw}'"))
                    })?),
                    None => None,
                };
                if !deselect && tool_id.is_none() {
                    return Err(McpToolError::public(
                        "provide toolId or set deselect=true",
                    ));
                }
                host.select_tool(tool_id, deselect, road_draw_type)
                    .map(CallToolResult::success)
            }
            Self::SimCityMoveCamera => {
                let x = require_f64(args, "x")?;
                let y = require_f64(args, "y")?;
                let z = require_f64(args, "z")?;
                if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                    return Err(McpToolError::public("camera x, y, z must be finite"));
                }
                let rotate = match args.get("rotate").and_then(JsonValue::as_f64) {
                    Some(r) if r.is_finite() => Some(r),
                    Some(_) => return Err(McpToolError::public("camera rotate must be finite")),
                    None => None,
                };
                host.move_camera(x, y, z, rotate)
                    .map(CallToolResult::success)
            }
            Self::SimCityBuildRoad => {
                let tool_id = args
                    .get("toolId")
                    .and_then(JsonValue::as_u64)
                    .and_then(|v| u32::try_from(v).ok());
                let draw_type = match args.get("drawType").and_then(JsonValue::as_str) {
                    Some(raw) => raw.parse::<RoadDrawType>().map_err(|_| {
                        McpToolError::public(format!("invalid drawType '{raw}'"))
                    })?,
                    None => RoadDrawType::Straight,
                };
                let sx = require_norm_f64(args, "startX")?;
                let sy = require_norm_f64(args, "startY")?;
                let ex = require_norm_f64(args, "endX")?;
                let ey = require_norm_f64(args, "endY")?;
                let steps = args
                    .get("steps")
                    .and_then(JsonValue::as_i64)
                    .unwrap_or(12)
                    .clamp(4, 40) as u32;
                host.build_road(tool_id, draw_type, sx, sy, ex, ey, steps)
                    .map(CallToolResult::success)
            }
            Self::SimCityAssignZone => {
                let raw_zone = require_str(args, "zoneType")?;
                let zone_type: ZoneType = raw_zone
                    .parse()
                    .map_err(|_| McpToolError::public(format!("invalid zoneType '{raw_zone}'")))?;
                let tool_id = args
                    .get("toolId")
                    .and_then(JsonValue::as_u64)
                    .and_then(|v| u32::try_from(v).ok());
                let sx = require_norm_f64(args, "startX")?;
                let sy = require_norm_f64(args, "startY")?;
                let ex = require_norm_f64(args, "endX")?;
                let ey = require_norm_f64(args, "endY")?;
                let steps = args
                    .get("steps")
                    .and_then(JsonValue::as_i64)
                    .unwrap_or(10)
                    .clamp(4, 40) as u32;
                host.assign_zone(zone_type, tool_id, sx, sy, ex, ey, steps)
                    .map(CallToolResult::success)
            }
            Self::SimCityPlaceBuilding => {
                let tool_id = require_u32(args, "toolId")?;
                let nx = require_norm_f64(args, "normalizedX")?;
                let ny = require_norm_f64(args, "normalizedY")?;
                let rotate_steps = args
                    .get("rotateSteps")
                    .and_then(JsonValue::as_i64)
                    .unwrap_or(0)
                    .clamp(-4, 4) as i32;
                host.place_building(tool_id, nx, ny, rotate_steps)
                    .map(CallToolResult::success)
            }
        }
    }
}

/// Validate `offset` (`0..=999_999`) and `limit` (`1..=64`) for `simcity_get_world_units`.
pub fn validate_world_pagination(offset: i64, limit: i64) -> Result<(), McpToolError> {
    if !(0..=MAX_WORLD_UNIT_OFFSET).contains(&offset)
        || !(MIN_WORLD_UNIT_LIMIT..=MAX_WORLD_UNIT_LIMIT).contains(&limit)
    {
        return Err(McpToolError::public(
            "Use offset 0..999999 and limit 1..64.",
        ));
    }
    Ok(())
}

/// Validate canonical `"slot,stamp"` world unit references (`\A(0|[1-9][0-9]{0,9}),(0|[1-9][0-9]{0,9})\z` within `i32`/`u32` bounds).
pub fn validate_canonical_unit_ref(unit_ref: &str) -> Result<(i32, u32), McpToolError> {
    let err = || {
        McpToolError::public(
            "Use a canonical native slot,stamp reference within int32/uint32 bounds.",
        )
    };
    let Some((slot_str, stamp_str)) = unit_ref.split_once(',') else {
        return Err(err());
    };
    if !is_canonical_decimal_segment(slot_str) || !is_canonical_decimal_segment(stamp_str) {
        return Err(err());
    }
    let slot = slot_str.parse::<i32>().map_err(|_| err())?;
    let stamp = stamp_str.parse::<u32>().map_err(|_| err())?;
    Ok((slot, stamp))
}

fn is_canonical_decimal_segment(s: &str) -> bool {
    if s.is_empty() || s.len() > 10 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    s == "0" || !s.starts_with('0')
}

fn require_str<'a>(args: &'a JsonValue, field: &str) -> Result<&'a str, McpToolError> {
    args.get(field)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| McpToolError::public(format!("missing required string argument '{field}'")))
}

fn require_u32(args: &JsonValue, field: &str) -> Result<u32, McpToolError> {
    args.get(field)
        .and_then(JsonValue::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| McpToolError::public(format!("missing required uint32 argument '{field}'")))
}

fn require_i64(args: &JsonValue, field: &str) -> Result<i64, McpToolError> {
    args.get(field)
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| McpToolError::public(format!("missing required integer argument '{field}'")))
}

fn require_f64(args: &JsonValue, field: &str) -> Result<f64, McpToolError> {
    args.get(field)
        .and_then(JsonValue::as_f64)
        .ok_or_else(|| McpToolError::public(format!("missing required number argument '{field}'")))
}

fn require_norm_f64(args: &JsonValue, field: &str) -> Result<f64, McpToolError> {
    let val = require_f64(args, field)?;
    if !val.is_finite() || !(0.0..=1.0).contains(&val) {
        return Err(McpToolError::public(format!(
            "argument '{field}' must be within 0.0..=1.0"
        )));
    }
    Ok(val)
}

fn require_bool(args: &JsonValue, field: &str) -> Result<bool, McpToolError> {
    args.get(field)
        .and_then(JsonValue::as_bool)
        .ok_or_else(|| McpToolError::public(format!("missing required boolean argument '{field}'")))
}
