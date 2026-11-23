#![forbid(unsafe_code)]

//! Domain DTOs, single-source `strum` enums, MCP content blocks, and structured tool error types for `simoder-mcp`.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use serde_with::DeserializeFromStr;
use strum::{EnumString, IntoStaticStr, VariantArray};

use crate::config::MAX_MCP_ERROR_MESSAGE_BYTES;

/// Top-level protocol, lifecycle, and transport errors in `simoder-mcp`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpError {
    /// Frame exceeded the `32 MiB` size bound ([`crate::config::MAX_MCP_FRAME_BYTES`]).
    #[error("Broker response exceeded its size bound")]
    FrameTooLarge(usize),
    /// Empty EOF or malformed JSON-RPC frame.
    #[error("The input does not contain any JSON tokens: {0}")]
    InvalidJson(String),
    /// Native MCP mod is not currently in the `Active` admitted state (for example game stopped or mod quiesced).
    #[error("MCP service is unavailable: native mod is not admitted in an active game session")]
    ServiceUnavailable,
    /// Client session is missing, expired, unconsented, or lacking required permission grants.
    #[error("MCP authorization denied: {0}")]
    AuthorizationDenied(String),
}

/// Classification of tool execution failures determining whether detailed diagnostics are disclosed on the wire.
///
/// Mirrors the client-boundary disclosure rules verified in `.analysis/u4u5/client-boundary/actual.json`:
/// - [`Self::PublicMcp`] (validation, native session, partial/unknown mutation, or auth denial) surfaces its message
///   truncated to [`MAX_MCP_ERROR_MESSAGE_BYTES`] (`4096` bytes): `"An error occurred invoking '<tool>': <msg>"`.
/// - [`Self::InternalMasked`] masks internal host/broker exceptions so sensitive details never leak:
///   `"An error occurred invoking '<tool>'."`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpToolError {
    /// Publicly reportable validation, native session, or permission failure (truncated to `4096` bytes).
    PublicMcp(String),
    /// Internal/generic backend failure whose raw message must not be disclosed to the client.
    InternalMasked(String),
}

impl McpToolError {
    /// Construct a [`Self::PublicMcp`] error with deterministic `4096`-byte UTF-8 boundary truncation.
    pub fn public(message: impl Into<String>) -> Self {
        let raw = message.into();
        let truncated = if raw.len() <= MAX_MCP_ERROR_MESSAGE_BYTES {
            raw
        } else {
            let mut end = MAX_MCP_ERROR_MESSAGE_BYTES;
            while end > 0 && !raw.is_char_boundary(end) {
                end -= 1;
            }
            raw[..end].to_string()
        };
        Self::PublicMcp(truncated)
    }

    /// Format the wire error text returned in `CallToolResult.content[0].text` for `tool_name`.
    pub fn format_wire_text(&self, tool_name: &str) -> String {
        match self {
            Self::PublicMcp(msg) => format!("An error occurred invoking '{tool_name}': {msg}"),
            Self::InternalMasked(_) => format!("An error occurred invoking '{tool_name}'."),
        }
    }
}

/// Requested path through the SimCity front-end.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    DeserializeFromStr,
    EnumString,
    IntoStaticStr,
    VariantArray,
)]
#[strum(ascii_case_insensitive)]
pub enum LaunchMode {
    /// Resume the most recent save.
    Continue,
    /// Enter play mode targeting a specific region and city.
    Play,
}

/// Externally meaningful SimCity startup and session stages.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    DeserializeFromStr,
    EnumString,
    IntoStaticStr,
    VariantArray,
)]
#[serde(rename_all = "camelCase")]
#[strum(ascii_case_insensitive)]
pub enum StartupStage {
    /// No active startup workflow.
    Idle,
    /// EA App is launching.
    EaStarting,
    /// EA App displayed an administrator notice.
    EaAdministratorNotice,
    /// Waiting for manual user approval of a Windows UAC prompt.
    AwaitingUac,
    /// Launcher play button stage.
    LauncherPlay,
    /// `SimCity.exe` process is starting.
    GameStarting,
    /// Intro movie is playing.
    Intro,
    /// Welcome overlay screen.
    Welcome,
    /// Main menu is ready.
    MainMenu,
    /// Region/city save selection screen.
    SaveSelection,
    /// City simulation is loading.
    CityLoading,
    /// City is loaded and finalized (`in_city`).
    InCity,
    /// Manual human action is required before proceeding.
    HumanActionRequired,
    /// Diagnostic indicator of a suspected infinite loading stall (never triggers force-kill).
    SuspectedInfiniteLoading,
    /// Startup workflow failed.
    Failed,
}

/// Provenance source of a [`StartupStage`] observation.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    DeserializeFromStr,
    EnumString,
    IntoStaticStr,
    VariantArray,
)]
#[strum(ascii_case_insensitive)]
pub enum StageSource {
    /// Authoritative native engine/bridge observation.
    NativeObservation,
    /// Visually inferred by an AI agent backed by a fresh `capture_id`.
    AiConfirmed,
    /// Stage transition wait timed out.
    Timeout,
    /// Internally synthesized fallback state.
    Internal,
}

/// Allowlisted keyboard keys accepted by `simcity_send_key`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    DeserializeFromStr,
    EnumString,
    IntoStaticStr,
    VariantArray,
)]
#[strum(ascii_case_insensitive)]
pub enum AllowedKey {
    /// Escape key (`Escape`).
    Escape,
    /// Enter / Return key (`Enter`).
    Enter,
    /// Space bar (`Space`).
    Space,
}

/// Supported SimCity road drawing geometry modes (`RoadDrawType`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    Serialize,
    DeserializeFromStr,
    EnumString,
    IntoStaticStr,
    VariantArray,
)]
#[strum(ascii_case_insensitive)]
#[repr(u32)]
pub enum RoadDrawType {
    /// Freehand curved road (`kRoadTypeCurvy = 0`).
    #[strum(to_string = "curvy")]
    Curvy = 0,
    /// Straight road segment (`kRoadTypeStraight = 1`).
    #[default]
    #[strum(to_string = "straight")]
    Straight = 1,
    /// Rectangular grid roads (`kRoadTypeGrid = 2`).
    #[strum(to_string = "grid")]
    Grid = 2,
    /// Circular arc segment (`kRoadTypeArc = 3`).
    #[strum(to_string = "arc")]
    Arc = 3,
    /// Closed circle loop (`kRoadTypeCircle = 4`).
    #[strum(to_string = "circle")]
    Circle = 4,
}

/// Supported RCI zoning modes for `simcity_assign_zone`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    DeserializeFromStr,
    EnumString,
    IntoStaticStr,
    VariantArray,
)]
#[strum(ascii_case_insensitive)]
pub enum ZoneType {
    /// Residential zone (`LotZoneResidential`).
    #[strum(to_string = "residential")]
    Residential,
    /// Commercial zone (`LotZoneCommercial`).
    #[strum(to_string = "commercial")]
    Commercial,
    /// Industrial zone (`LotZoneIndustrial`).
    #[strum(to_string = "industrial")]
    Industrial,
    /// Remove existing zoning (`Dezone`).
    #[strum(to_string = "unzone")]
    Unzone,
}

/// Semantic native session operations executed on the game's verified owning thread.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    IntoStaticStr,
    VariantArray,
)]
pub enum NativeSessionOperation {
    /// Skip the intro movie via native `StopIntroMovie`.
    SkipIntro,
    /// Observe updater/menu/loading/city state and save identities.
    Observe,
    /// Complete updater (`UPDATE_COMPLETE`) and await main menu readiness.
    EnterMenu,
    /// Query active `RegionTemplates`.
    RegionTemplates,
    /// Query offline `PlayingRegions` and Sandbox flags.
    Regions,
    /// Query `GetRegionDetails` for a specific `region_id`.
    RegionDetails,
    /// Create a new offline Sandbox region via `CreateRegion`.
    CreateSandboxRegion,
    /// Claim an unclaimed city box in a Sandbox region via `CreateBox`.
    ClaimCity,
    /// Load a claimed Sandbox city and await `isLoadingFinalized`.
    LoadCity,
    /// List a bounded page (`offset`, `limit`) of `slot,stamp` world unit references.
    InspectWorldUnits,
    /// Inspect position, name, and property key of one `slot,stamp` world unit reference.
    InspectWorldUnit,
}

/// Typed request envelope passed to the host native session adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSessionRequest {
    /// Operation kind.
    pub operation: NativeSessionOperation,
    /// Optional target region ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_id: Option<u32>,
    /// Optional target city box ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city_id: Option<u32>,
    /// Optional region template ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_id: Option<u32>,
    /// Optional explicit region or city name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional string argument (such as `"offset,limit"` or `"slot,stamp"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument: Option<String>,
    /// Bounded operation timeout in seconds.
    pub timeout_seconds: i32,
}

/// MCP tool behavioral annotations (`annotations` in `tools/list`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    /// Whether the tool performs destructive state changes.
    pub destructive_hint: bool,
    /// Whether repeated identical calls have no additional effect.
    pub idempotent_hint: bool,
    /// Whether the tool interacts with external open-world systems.
    pub open_world_hint: bool,
    /// Whether the tool is strictly read-only.
    pub read_only_hint: bool,
}

impl ToolAnnotations {
    /// Construct read-only tool annotations.
    pub const fn read_only(idempotent: bool) -> Self {
        Self {
            destructive_hint: false,
            idempotent_hint: idempotent,
            open_world_hint: false,
            read_only_hint: true,
        }
    }

    /// Construct state-mutating tool annotations.
    pub const fn mutating(idempotent: bool, destructive: bool) -> Self {
        Self {
            destructive_hint: destructive,
            idempotent_hint: idempotent,
            open_world_hint: false,
            read_only_hint: false,
        }
    }
}

/// Complete MCP tool definition advertised by `tools/list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    /// Unique tool identifier.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for tool input arguments.
    pub input_schema: JsonValue,
    /// Optional JSON Schema for structured tool output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<JsonValue>,
    /// Behavioral hints for MCP clients.
    pub annotations: ToolAnnotations,
}

/// Content block inside an MCP `CallToolResult` (`text` or `image`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpContentBlock {
    /// UTF-8 text content block.
    Text {
        /// Serialized JSON or diagnostic error text.
        text: String,
    },
    /// Base64-encoded image content block (used by `simcity_capture_window` and `simcity_get_diagnostics`).
    #[serde(rename_all = "camelCase")]
    Image {
        /// Base64-encoded image payload.
        data: String,
        /// MIME type (always `"image/png"` for window captures).
        mime_type: String,
    },
}

/// Standardized MCP `tools/call` result envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    /// Content blocks (`text` and optional `image/png`).
    pub content: Vec<McpContentBlock>,
    /// Structured JSON payload omitted on tool error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    /// `true` when the tool call failed validation, authorization, or execution.
    pub is_error: bool,
}

impl CallToolResult {
    /// Construct a successful `CallToolResult` with matching `structuredContent` and JSON `text` block.
    pub fn success(structured: JsonValue) -> Self {
        let text = serde_json::to_string(&structured).unwrap_or_else(|_| "{}".into());
        Self {
            content: vec![McpContentBlock::Text { text }],
            structured_content: Some(structured),
            is_error: false,
        }
    }

    /// Construct a successful `CallToolResult` carrying both `structuredContent` metadata and a native `image/png` block.
    pub fn success_with_png(structured: JsonValue, png_base64: String) -> Self {
        let text = serde_json::to_string(&structured).unwrap_or_else(|_| "{}".into());
        Self {
            content: vec![
                McpContentBlock::Text { text },
                McpContentBlock::Image {
                    data: png_base64,
                    mime_type: "image/png".into(),
                },
            ],
            structured_content: Some(structured),
            is_error: false,
        }
    }

    /// Construct an error `CallToolResult` (`isError: true`) formatted according to [`McpToolError`].
    pub fn error(tool_name: &str, err: &McpToolError) -> Self {
        Self {
            content: vec![McpContentBlock::Text {
                text: err.format_wire_text(tool_name),
            }],
            structured_content: None,
            is_error: true,
        }
    }
}
