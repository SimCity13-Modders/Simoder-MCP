# Glossary (`Simoder.McpServer` / `Simoder.DevBridge` C# -> `simoder-mcp` Rust)

| Legacy C# / Native SDK Term | Meaning | Rust Equivalent (`simoder-mcp`) |
| --- | --- | --- |
| `SimoderModule_QueryV2` .. `DrainV2` | Native module ABI v2 lifecycle exports (`Simoder-SDK/Native/native.h`) | `abi::SimoderModule_QueryV2` .. `abi::SimoderModule_DrainV2` (`SdResult`) |
| `SdHostApiV2` / `SdModuleDescriptorV2` | 32-byte host ABI v2 root table & 56-byte contract descriptor (`SD_CONTRACT_SHA256_HEX`) | `abi::SdHostApiV2`, `abi::SdModuleDescriptorV2` |
| `GameMcpHost` | Lifecycle-bound MCP server host owned by the running game | `host::NativeMcpModule` (`ModuleScopeState::{Prepared, Active, Stopping, Drained}`) |
| `DevBridgeClient` / `BrokerService` | Internal dispatch bridge between MCP tool handlers and game runtime services | `host::HostDispatchProvider` trait & `host::SimulatedHostProvider` |
| `ClientAuthorization` / `GameMcpEndpoint` | Client identity, explicit agent consent, permission grants, and live revocation | `auth::McpAuthManager`, `auth::ClientSession`, `auth::McpGrant` |
| `BootstrapTools` / `SimCityTools` / `NativeSessionTools` / `NativeWorldTools` / `RuntimeAnalysisTools` / `DevExperienceTools` | Legacy C# `[McpServerToolType]` classes | `catalog::BuiltinToolId` (`strum` enum) & `catalog::ToolCatalog` |
| `HostToolRegistry` (new, `ADR-004`) | Decoupled mod-provided MCP tool extension sink in `Simoder` | `catalog::ExternalModTool` & `simoder_mods::HostToolRegistry` |
| `sc_properties::debug_tools::build_debug_tools_overlay` (new, `ADR-003`) | `Simoder-OpenSCP` retail debug-tool unlock overlay (`0x53B77423`, `0xA2AA4268`, `0xB65E1F2E`) | `simoder_unlock_debug_tools` tool in `catalog.rs` |
| `BridgeCorrelationTracker` / `BridgeTime` / `parse_sldelta_v4_envelopes` (new, `ADR-003`) | `Simoder-OpenSCP` 14-channel `EAWebKit` bridge observer & `SLDelta` v4 telemetry | `simoder_get_bridge_telemetry`, `simoder_inspect_sldelta`, `simoder_eval_ui_bridge` tools |
| `CaptureResultFactory` / `DiagnosticsResultFactory` | Produces `structuredContent` + native `image/png` content block without duplicate base64 | `protocol::McpContentBlock::Image` & `protocol::ToolCallEnvelope` |
| `McpException` (4096-byte truncation) vs `BrokerException` (generic masking) | Native/validation errors surface up to 4096 chars; internal exceptions mask sensitive details | `types::McpToolError::{PublicMcp, InternalMasked, AuthDenied}` |
