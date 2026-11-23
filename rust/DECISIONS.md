# Decisions (`rust-anything` Architecture Decision Records for `simoder-mcp`)

## ADR-001: Native Mod `.dll` Boundary Without Bundling Simoder Crates
Status: accepted (2026-11-21)
Context: `Simoder` (`d3d9.dll`) supports native mods (`.dll`) via the Native Module Host Boundary (`ABI v2`: `SimoderModule_QueryV2`, `SimoderModule_PrepareV2`, `SimoderModule_AdmitV2`, `SimoderModule_QuiesceV2`, `SimoderModule_DrainV2`, `SdHostApiV2`). The legacy C# stack (`Simoder.DevBridge.exe` + `Simoder.McpServer.exe`, 52 units / 4,765 LOC) ran as separate child `.exe` processes.
Decision: Implement `simoder-mcp` in `Simoder-DevTools-MCP/crates/simoder-mcp` as an optional native DevTool mod `.dll` (`simoder_mcp.dll`, `cdylib` + `rlib`) interacting with `Simoder`'s DLL across the `ABI v2` host boundary (`SdHostApiV2`) and host dispatch bridge, without embedding or duplicating `Simoder`'s internal crates inside `simoder-mcp`.
Consequences: `Simoder` remains one clean `d3d9.dll` beside `SimCity.exe`; `simoder_mcp.dll` is loaded and lifecycle-managed by `Simoder` only when enabled, with zero standalone `.exe` services or offline bootstrap processes.

## ADR-002: Game-Owned Lifecycle & Single Owner Guard (`U4`)
Status: accepted (2026-11-21)
Context: In `U4` (`Simoder-DevTools-MCP#1`), the MCP service must have a single active owner bound to the game's lifecycle, with zero callable MCP tools when the game is stopped or the mod is quiesced/drained.
Decision: Tie `simoder-mcp` admission strictly to `SimoderModule_PrepareV2` (`Prepared`), `SimoderModule_AdmitV2` (`Active` only after native trust and base readiness), `SimoderModule_QuiesceV2` (`Stopping` — immediately closes MCP request admission and invalidates active sessions), and `SimoderModule_DrainV2` (`Drained` once in-flight requests settle; returns `SD_BUSY` while work is in flight).
Consequences: No MCP endpoint or offline bootstrap tool is callable when the game is not running or after `QuiesceV2`. Duplicate active owners for `simoder.devtools.mcp` are rejected with `SD_CONFLICT`.

## ADR-003: Reference Tool Catalog + Improvements (`U4`)
Status: accepted (2026-11-21)
Context: The legacy C# `Simoder.McpServer` defined 28 tools (`Fixtures/game.json`) and 31 client-boundary error/truncation/retention rules (`.analysis/u4u5/client-boundary/actual.json`). The user directed treating the legacy C# fixtures as a reference rather than a rigid ceiling so we can improve the catalog and remove retired offline-bootstrap artifacts.
Decision: Preserve the reference JSON-RPC 2.0 wire contracts (`initialize`, `notifications/initialized`, `tools/list`, `tools/call`, `structuredContent` + `content` text/PNG image blocks, `4096`-byte native/MCP error truncation, masking of sensitive internal errors, `partial`/`unknown` mutation reporting without automatic retry, and `slot,stamp` / `offset 0..999999` / `limit 1..64` validation) while upgrading the catalog with native `Simoder-OpenSCP` and `simoder-crash` diagnostic tools.
Consequences: Existing MCP clients interoperate seamlessly while gaining native `OpenSCP` debug-tool unlock, 14-channel `EAWebKit` bridge telemetry (`BridgeTime`), and `SLDelta` v4 / `cEcoGame` rule inspection.

## ADR-004: Decoupled Mod Tool Extensions Without Hard Dependency on MCP
Status: accepted (2026-11-21)
Context: Third-party and first-party mods (Luau, JS, or Native `.dll`) want to expose custom MCP tools when `simoder.devtools.mcp` is active, but must work 100% normally when `simoder.devtools.mcp` is not installed or disabled.
Decision: Provide the mod tool registration sink (`HostToolRegistry`) on the `Simoder` host side (`simoder-mods::native_host::HostToolRegistry`) and bridge it into `simoder-mcp`. Mods register their tool descriptors (`<mod_id>.<tool_name>`, JSON schemas, required permission grants, and handler) with `Simoder`. When `simoder-mcp` is absent, registrations remain inert in `Simoder`. When `simoder-mcp` is active, it lists and invokes active mod tools, and `Simoder` automatically revokes a mod's tools when its `(mod_id, generation)` is unloaded or reloaded.
Consequences: Mods never depend on `simoder-mcp` to load or run, and stale mod generations can never be invoked over MCP.

## ADR-005: Client Identity, Explicit Consent, Permission Grants & Live Revocation (`U4`)
Status: accepted (2026-11-21)
Context: `Simoder-DevTools-MCP#3` requires binding every MCP call to client identity, explicit agent consent, required/optional permission grants, token epoch freshness, and live mid-session revocation.
Decision: Enforce `McpSecurityGate` before every `tools/list` and `tools/call` execution: verify active game/module `Active` state, authenticated client session token, current runtime epoch, explicit user agent-access consent, and tool-specific `McpGrant` (`session.read`, `session.mutate`, `world.inspect`, `mods.control`, `memory.analysis`, `ui.input`, `openscp.diagnostics`, or mod-specific `<mod_id>.*` grants). Revoking consent or a grant mid-session immediately denies queued/subsequent calls with zero game side effects.
Consequences: Unauthorized, unconsented, revoked, stale-token, or stopped-game calls fail closed before any host dispatch occurs.
