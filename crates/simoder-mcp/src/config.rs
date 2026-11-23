#![forbid(unsafe_code)]

//! Protocol constants, ABI v2 contract fingerprints, payload size bounds, and `Simoder-OpenSCP`
//! retail debug-tool identifiers for `simoder-mcp`.

/// Canonical `Simoder-SDK/Native/native.h` normative contract bundle SHA-256 hex digest (`SD_CONTRACT_SHA256_HEX`).
pub const SD_CONTRACT_SHA256_HEX: &str =
    "00c98e36f669c6c4f16f7ffd9073130ae9798543b3d0759d7c1937f443e6e236";

/// Root host ABI version (`SD_U2A_HOST_VERSION = 2`).
pub const SD_U2A_HOST_VERSION: u32 = 2;

/// Base contract version required by `SdModuleDescriptorV2` (`SD_BASE_CONTRACT_VERSION = 1`).
pub const SD_BASE_CONTRACT_VERSION: u32 = 1;

/// Exact byte size of `SdModuleDescriptorV2` (`56` bytes).
pub const SD_MODULE_DESCRIPTOR_V2_SIZE: u32 = 56;

/// Maximum number of active/retained module owner generations tracked in the native mod DLL (`128`).
pub const MAX_MODULE_GENERATIONS: usize = 128;

/// MCP protocol revision negotiated with current clients (`2025-11-25`).
pub const MCP_PROTOCOL_VERSION_2025_11_25: &str = "2025-11-25";

/// MCP Streamable HTTP baseline protocol revision (`2025-06-18`, `ADR 0001`).
pub const MCP_PROTOCOL_VERSION_2025_06_18: &str = "2025-06-18";

/// Canonical native MCP DevTool mod identifier (`mods/simoder.devtools/`).
pub const MCP_NATIVE_MOD_ID: &str = "simoder.devtools.mcp";

/// Server name advertised in the MCP `initialize` response.
pub const MCP_SERVER_NAME: &str = "simoder-mcp";

/// Server semantic version advertised in the MCP `initialize` response.
pub const MCP_SERVER_VERSION: &str = "0.2.0";

/// Cross-tool safety and workflow instructions returned during MCP `initialize`.
pub const MCP_SERVER_INSTRUCTIONS: &str = "Simoder native MCP DevTool (.dll) operates inside the running SimCity process via Simoder's ABI v2 host boundary. No MCP tools are callable when the game is not running or when the mod is quiesced. Every tool invocation requires an authenticated client session, explicit user agent-access consent, and applicable permission grants. Mutations (region creation, city claim, city load) are never retried automatically; inspect session/regions state before retrying after a partial or unknown outcome.";

/// Maximum character/byte length (`4096`) preserved on public native/MCP tool error messages before truncation.
pub const MAX_MCP_ERROR_MESSAGE_BYTES: usize = 4096;

/// Maximum inbound/outbound JSON-RPC frame size (`32 MiB`, matching `UserPipe` and client-boundary tests).
pub const MAX_MCP_FRAME_BYTES: usize = 32 * 1024 * 1024;

/// Maximum valid `offset` parameter for `simcity_get_world_units` (`0..=999_999`).
pub const MAX_WORLD_UNIT_OFFSET: i64 = 999_999;

/// Minimum valid `limit` parameter for `simcity_get_world_units` (`1`).
pub const MIN_WORLD_UNIT_LIMIT: i64 = 1;

/// Maximum valid `limit` parameter for `simcity_get_world_units` (`64`).
pub const MAX_WORLD_UNIT_LIMIT: i64 = 64;

/// Minimum valid `timeoutSeconds` for `simcity_wait_for_stage` (`1`).
pub const MIN_WAIT_STAGE_TIMEOUT_SECS: i64 = 1;

/// Maximum valid `timeoutSeconds` for `simcity_wait_for_stage` (`300`).
pub const MAX_WAIT_STAGE_TIMEOUT_SECS: i64 = 300;

/// Minimum valid `length` for `simoder_read_game_memory` (`1` byte).
pub const MIN_MEMORY_READ_BYTES: i64 = 1;

/// Maximum valid `length` for `simoder_read_game_memory` (`65536` bytes / `64 KiB`).
pub const MAX_MEMORY_READ_BYTES: i64 = 65_536;

/// Maximum number of log lines returned by `simoder_get_logs` (`200`).
pub const MAX_LOG_TAIL_LINES: i64 = 200;

/// Default number of log lines returned by `simoder_get_logs` (`80`).
pub const DEFAULT_LOG_TAIL_LINES: i64 = 80;

/// Default loopback bind address for the game-hosted Streamable HTTP MCP server (`ADR 0001`).
pub const DEFAULT_MCP_HTTP_BIND: &str = "127.0.0.1:47391";

/// Streamable HTTP endpoint path for MCP JSON-RPC requests (`http://127.0.0.1:47391/mcp`).
pub const MCP_HTTP_ENDPOINT_PATH: &str = "/mcp";

/// Loopback health endpoint path (`http://127.0.0.1:47391/health`).
pub const MCP_HTTP_HEALTH_PATH: &str = "/health";

/// Environment variable overriding the loopback bind address (non-loopback requires opt-in).
pub const ENV_MCP_HTTP_BIND: &str = "SIMODER_MCP_BIND";

/// Environment variable configuring an explicit bearer token requirement.
pub const ENV_MCP_HTTP_TOKEN: &str = "SIMODER_MCP_TOKEN";

/// Relative file path under the game root storing the generated loopback bearer token.
pub const MCP_TOKEN_RELATIVE_PATH: &str = "simoder/mcp_token.txt";

/// Named Win32 mutex enforcing one active MCP service generation per Windows session (`ADR 0001`).
pub const MCP_SINGLETON_MUTEX_NAME: &str = r"Local\SimoderMcpDevToolSingletonV2";

/// Default local developer session identifier for loopback Streamable HTTP clients.
pub const DEFAULT_LOOPBACK_SESSION_ID: &str = "loopback-game-session";

/// Default local developer token provisioned for loopback clients when `SIMODER_MCP_TOKEN` is unset.
pub const DEFAULT_LOOPBACK_TOKEN: &str = "simoder-loopback-dev-token";

/// Retail SimCity DBPF tool-definition group (`0x09878A01`, `Simoder-OpenSCP` `sc_properties::debug_tools::TOOL_GROUP`).
pub const OPENSCP_TOOL_GROUP: u32 = 0x0987_8A01;

/// Retail parent instance shared by 310 visible building tools (`0xAF042E9A`, `LIVE_TOOL_PARENT`).
pub const OPENSCP_LIVE_TOOL_PARENT: u32 = 0xAF04_2E9A;

/// Default target menu category for unlocking retail debug tools (`0xC710B6E9`, `DEFAULT_CATEGORY`).
pub const OPENSCP_DEFAULT_CATEGORY: u32 = 0xC710_B6E9;

/// Default target UI subcategory for unlocking retail debug tools (`0x9D2EF585`, `DEFAULT_UI_CATEGORY`).
pub const OPENSCP_DEFAULT_UI_CATEGORY: u32 = 0x9D2E_F585;

/// Retail instances of the three property-gated SimCity development tools (`Edit Unit Prop`, `Edit Snap Points`, `Unit Connecter`).
pub const OPENSCP_DEBUG_TOOLS: [u32; 3] = [0x53B7_7423, 0xA2AA_4268, 0xB65E_1F2E];
