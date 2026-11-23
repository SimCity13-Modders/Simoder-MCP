# Migration Map (`Simoder-DevTools-MCP` -> Native Rust Mod `simoder-mcp`)

- **Goal**: Replace the legacy `.NET 8` child-executable stack (`Simoder.DevBridge.exe` + `Simoder.McpServer.exe`, 52 C# units / 4,765 LOC) and reject the 95k-LOC C++ `Root5` branch in favor of a pure-Rust native DevTool mod `.dll` (`crates/simoder-mcp` -> `simoder_mcp.dll`) loaded by `Simoder` (`d3d9.dll`) via `ABI v2` (`ADR-001`..`ADR-005`).
- **Decisions**: [DECISIONS.md](DECISIONS.md) (`ADR-001`..`ADR-005`)
- **Glossary**: [GLOSSARY.md](GLOSSARY.md)

## Wave Plan & Unit Status

| Wave | Legacy C# Package / Scope | Target Rust Module (`crates/simoder-mcp/src/`) | State | Verification Gate |
| --- | --- | --- | --- | --- |
| Wave 0 | `Simoder-SDK/Native` (`abi_v1.h`, `native.h`) & `GameProcessLifetime.cs` | `abi.rs` & `config.rs` (`SimoderModule_QueryV2`..`DrainV2`, `SdModuleDescriptorV2`, single-owner lifecycle guard) | `done` | `x86_64` + `i686-pc-windows-msvc` ABI layout & lifecycle tests |
| Wave 1 | `Simoder.DevBridge.Core` (`Contracts.cs`, `MemoryContracts.cs`, `NativeSessionContracts.cs`, `ClientAuthorization.cs`) | `types.rs` & `auth.rs` (DTOs, `McpGrant`, client identity, explicit consent, token epoch, live revocation) | `done` | `Simoder-DevTools-MCP#3` security & revocation tests |
| Wave 2 | `Simoder.McpServer` (`BootstrapTools.cs`, `DevExperienceTools.cs`, `SimCityTools.cs`, `NativeSessionTools.cs`, `NativeWorldTools.cs`, `RuntimeAnalysisTools.cs`, `RuntimeCapabilities.cs`) + `Simoder-OpenSCP` extensions | `catalog.rs` & `host.rs` (reference tools + `OpenSCP` debug tools, 14-channel `EAWebKit` bridge telemetry, `SLDelta` v4 inspector, decoupled mod tool registry) | `done` | `Simoder-DevTools-MCP#2` catalog, schema & extension tests |
| Wave 3 | `GameMcpHost.cs`, `DevBridgeClient.cs`, `Simoder.McpServer.Compatibility.Tests`, `Simoder.McpServer.ClientBoundary.Tests` | `protocol.rs` (JSON-RPC 2.0 MCP engine, `initialize`, `tools/list`, `tools/call`, 4096-char truncation, generic error masking, 32 MiB frame guard) | `done` | `Simoder-DevTools-MCP#4` client boundary & wire interoperability tests |
