#![deny(unsafe_code)]

//! Host dispatch provider trait, live Win32/filesystem backend ([`LiveHostBackend`]),
//! reference fixture backend ([`StandardHostBackend`]), and decoupled mod tool extension descriptors (`ADR-001`, `ADR-004`).

pub mod backend;
pub mod live;
mod live_builder;
mod live_session;
pub mod openscp;
#[cfg(windows)]
mod win32_live;

use std::sync::Arc;

pub use backend::*;
pub use live::*;
pub use openscp::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::types::{
    AllowedKey, LaunchMode, McpToolError, NativeSessionRequest, RoadDrawType, StartupStage,
    ToolDefinition, ZoneType,
};

/// Synchronous callback executing a strict-ES5 JS function on the game's verified owning UI thread.
pub type UiJsEvalFn = Arc<dyn Fn(&str, &str, u32) -> Result<String, String> + Send + Sync + 'static>;

/// Decoupled mod tool descriptor registered in `Simoder`'s host registry (`ADR-004`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalModTool {
    /// Host-verified owning mod ID (for example `"dawid.traffic-analytics"`).
    pub owner_mod_id: String,
    /// Host-verified owning mod generation (`>= 1`).
    pub owner_generation: u64,
    /// Tool definition (`name` is automatically namespaced as `"<owner_mod_id>.<tool_name>"`).
    pub definition: ToolDefinition,
    /// Required permission grant strings checked before invoking this tool.
    pub required_grants: Vec<String>,
}

/// Callback handler executing a mod-registered tool on the host.
pub type ExternalModToolHandler =
    Arc<dyn Fn(&JsonValue) -> Result<JsonValue, McpToolError> + Send + Sync + 'static>;

/// Record of a backend call captured for compatibility and provenance verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedBackendCall {
    /// Target host/broker route name.
    pub route: String,
    /// Projected typed arguments sent to the host route.
    pub arguments: JsonValue,
}

/// Fault-injection mode for verifying error projection, `4096`-byte truncation, and partial/unknown mutation outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendSimulationMode {
    /// Normal successful operation.
    #[default]
    Success,
    /// Publicly reportable MCP/native error (`McpException`).
    McpError,
    /// Generic internal host failure whose sensitive details must be masked on the wire.
    GenericError,
    /// `simcity_create_sandbox_region` partial outcome requiring user inspection before retry.
    PartialCreate,
    /// `simcity_claim_city` partial outcome requiring user inspection before retry.
    PartialClaim,
    /// `simcity_load_city` unacknowledged outcome requiring session inspection before retry.
    UnknownOutcome,
    /// `simcity_get_diagnostics` without a recent window screenshot (`LastCapture == null`).
    WithoutImage,
    /// Native error exceeding `4096` bytes to verify deterministic `4096`-byte truncation.
    OversizedNativeError4100,
}

/// Abstract host bridge connecting `simoder_mcp.dll` to `Simoder` (`d3d9.dll`) without bundling Simoder crates.
pub trait HostDispatchProvider: Send + Sync {
    /// Execute a native session or world inspection operation on the game's verified owning thread.
    fn native_session(&self, request: NativeSessionRequest) -> Result<JsonValue, McpToolError>;

    /// Read `1..=65536` bytes at a hexadecimal virtual address in the owning game's 32-bit address space.
    fn read_game_memory(&self, address: &str, length: i64) -> Result<JsonValue, McpToolError>;

    /// List loaded modules of the owning game process.
    fn get_game_modules(&self) -> Result<JsonValue, McpToolError>;

    /// Export live executable PE sections and provenance metadata for REA/Ghidra analysis.
    fn export_game_code(&self) -> Result<JsonValue, McpToolError>;

    /// Enable or disable one discovered mod ID through `Simoder`'s mod manager.
    fn set_mod_enabled(&self, mod_id: &str, enabled: bool) -> Result<JsonValue, McpToolError>;

    /// Request normal game exit (`WM_CLOSE`) on explicit user instruction.
    fn close_game(&self) -> Result<JsonValue, McpToolError>;

    /// Start or observe a game startup session (`Continue` or `Play`).
    fn start_session(
        &self,
        mode: LaunchMode,
        region_name: Option<&str>,
        city_name: Option<&str>,
    ) -> Result<JsonValue, McpToolError>;

    /// Read current startup state without launching or mutating anything.
    fn get_startup_state(&self) -> Result<JsonValue, McpToolError>;

    /// Capture an allowlisted window (`"auto"`, `"ea"`, or `"simcity"`), returning `(metadata, png_base64)`.
    fn capture_window(&self, target: &str) -> Result<(JsonValue, String), McpToolError>;

    /// Send a capture-bound normalized click (`0.0..=1.0`).
    fn click(
        &self,
        capture_id: &str,
        normalized_x: f64,
        normalized_y: f64,
    ) -> Result<JsonValue, McpToolError>;

    /// Send one allowlisted key (`Escape`, `Enter`, `Space`) to an allowlisted window.
    fn send_key(&self, target: &str, key: AllowedKey) -> Result<JsonValue, McpToolError>;

    /// Record an AI-confirmed startup stage backed by a fresh `capture_id`.
    fn confirm_stage(
        &self,
        capture_id: &str,
        stage: StartupStage,
        confidence: f64,
        note: Option<&str>,
    ) -> Result<JsonValue, McpToolError>;

    /// Wait `1..=300` seconds for a target startup stage.
    fn wait_for_stage(
        &self,
        stage: StartupStage,
        timeout_seconds: i64,
    ) -> Result<JsonValue, McpToolError>;

    /// Collect bounded startup diagnostics and optional last capture PNG (`(metadata, Option<png_base64>)`).
    fn get_diagnostics(&self) -> Result<(JsonValue, Option<String>), McpToolError>;

    /// Read `Simoder`'s atomic runtime state snapshot (mods, overrides, pending changes, conflicts).
    fn get_runtime_state(&self) -> Result<JsonValue, McpToolError>;

    /// Read up to `200` recent lines from the `Simoder` diagnostic log pipeline.
    fn get_logs(&self, mod_id: Option<&str>, limit: i64) -> Result<JsonValue, McpToolError>;

    /// Read `simoder_get_capabilities` descriptor from the host runtime.
    fn get_capabilities(&self) -> JsonValue;

    /// Read `simoder_get_service_info` (`serverPid`, `gamePid`, `transport`).
    fn get_service_info(&self) -> JsonValue;

    /// `Simoder-OpenSCP` extension: generate/inspect the retail debug-tools unlock DBPF overlay
    /// (`sc_properties::debug_tools::build_debug_tools_overlay`).
    fn unlock_debug_tools(
        &self,
        category: Option<u32>,
        ui_category: Option<u32>,
    ) -> Result<JsonValue, McpToolError>;

    /// `Simoder-OpenSCP` extension: query 14-channel Backend <-> Frontend (`EAWebKit`) bridge telemetry,
    /// `BridgeTime` simulation clock/pause state, and `CaptureQualityReport`.
    fn get_bridge_telemetry(&self, channel: Option<&str>) -> Result<JsonValue, McpToolError>;

    /// `Simoder-OpenSCP` extension: inspect retail `SLDelta` v4 container records and `cEcoGame` rule telemetry.
    fn inspect_sldelta(&self, payload_hex: &str) -> Result<JsonValue, McpToolError>;

    /// `Simoder-OpenSCP` extension: dispatch a validated `RequestGameData` or `PostGameCommand` bridge call.
    fn eval_ui_bridge(&self, method: &str, verb: &str) -> Result<JsonValue, McpToolError>;

    /// Query city-building tool palette categories and tool IDs in an active city session.
    fn get_build_tools(&self, category: Option<&str>) -> Result<JsonValue, McpToolError>;

    /// Select (or deselect) a city-building tool ID and optional road draw mode.
    fn select_tool(
        &self,
        tool_id: Option<u32>,
        deselect: bool,
        road_draw_type: Option<RoadDrawType>,
    ) -> Result<JsonValue, McpToolError>;

    /// Reposition and optionally rotate the active city simulation camera immediately.
    fn move_camera(
        &self,
        x: f64,
        y: f64,
        z: f64,
        rotate: Option<f64>,
    ) -> Result<JsonValue, McpToolError>;

    /// Build a road between two normalized window coordinates using the selected road tool and draw mode.
    #[allow(clippy::too_many_arguments)]
    fn build_road(
        &self,
        tool_id: Option<u32>,
        draw_type: RoadDrawType,
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        steps: u32,
    ) -> Result<JsonValue, McpToolError>;

    /// Assign (or remove) RCI zoning along a normalized drag stroke in the active city.
    #[allow(clippy::too_many_arguments)]
    fn assign_zone(
        &self,
        zone_type: ZoneType,
        tool_id: Option<u32>,
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        steps: u32,
    ) -> Result<JsonValue, McpToolError>;

    /// Place a building (`tool_id`) at normalized viewport coordinates with optional rotation steps (`','`/`'.'`).
    fn place_building(
        &self,
        tool_id: u32,
        normalized_x: f64,
        normalized_y: f64,
        rotate_steps: i32,
    ) -> Result<JsonValue, McpToolError>;

    /// Inspect the live `scrui` UI control hierarchy inside EAWebKit.
    fn inspect_ui_tree(
        &self,
        control_id: Option<u32>,
        max_depth: u32,
        visible_only: bool,
        max_nodes: u32,
    ) -> Result<JsonValue, McpToolError>;

    /// Query live EAWebKit DOM elements by CSS selector and inspect rects + computed styles.
    fn query_ui_dom(
        &self,
        selector: &str,
        limit: u32,
        include_computed_style: bool,
    ) -> Result<JsonValue, McpToolError>;

    /// Inspect Simoder's live declarative UI host state, slot containers, and mounted patches.
    fn get_ui_state(&self) -> Result<JsonValue, McpToolError>;

    /// List currently active decoupled mod extension tools registered in `Simoder`'s `HostToolRegistry`.
    fn list_external_mod_tools(&self) -> Vec<ExternalModTool>;

    /// Invoke a decoupled mod extension tool by its canonical namespaced name (`"<mod_id>.<tool_name>"`).
    fn invoke_external_mod_tool(
        &self,
        tool_name: &str,
        arguments: &JsonValue,
    ) -> Result<JsonValue, McpToolError>;
}
