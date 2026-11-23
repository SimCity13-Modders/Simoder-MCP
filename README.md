# Simoder-DevTools-MCP

Source owner of the optional game-owned native MCP DevTool mod (`crates/simoder-mcp` -> `simoder_mcp.dll`) for Simoder (`U4`/`U5`, `[Rust-6]`).

Built in Rust (`edition = "2024"`), `simoder_mcp.dll` interacts with `Simoder` (`d3d9.dll`) across the Native Module Host Boundary (`ABI v2`: `SimoderModule_QueryV2`, `SimoderModule_PrepareV2`, `SimoderModule_AdmitV2`, `SimoderModule_QuiesceV2`, `SimoderModule_DrainV2`) without embedding or duplicating `Simoder`'s internal runtime crates and without spawning legacy standalone `.NET` child executables (`Simoder.DevBridge.exe` / `Simoder.McpServer.exe`).

## Architecture & Capabilities

- **Game-Bound Lifecycle & Single Owner (`U4` / `ADR-002`)**: `PrepareV2` stages the module generation; `AdmitV2` activates MCP only after native trust and `base` readiness; `QuiesceV2` immediately closes MCP admission on mod disable or game stop; `DrainV2` returns `SD_BUSY` while any tool call is in flight. No MCP tools or offline bootstrap servers are callable when the game is not running.
- **Client Identity, Consent, Grants & Live Revocation (`U4` / `ADR-005`)**: Every `initialize`, `tools/list`, and `tools/call` invocation is verified by `McpAuthManager` for client identity, SHA-256 token digest, current runtime epoch, explicit user agent-access consent, and tool-specific `McpGrant` (`session.read`, `session.mutate`, `world.inspect`, `mods.control`, `memory.analysis`, `ui.input`, `openscp.diagnostics`, plus mod namespace grants).
- **32 Built-in Tools + `Simoder-OpenSCP` Extensions (`ADR-003`)**:
  - Preserves reference session, region, city, world (`slot,stamp`), window capture (`image/png`), mod control, and memory/REA export tool contracts (`4096`-byte public error truncation, internal error masking, `32 MiB` frame guard).
  - Adds native `Simoder-OpenSCP` diagnostic tools: `simoder_unlock_debug_tools` (`sc-properties::debug_tools::build_debug_tools_overlay`), `simoder_get_bridge_telemetry` (14-channel `EAWebKit` bridge observer & `BridgeTime`), `simoder_inspect_sldelta` (retail `SLDelta` v4 container inspector), and `simoder_eval_ui_bridge`.
- **Decoupled Mod Tool Extensions (`ADR-004`)**: Dynamically exposes tools registered by other mods in `Simoder`'s `HostToolRegistry` (`<mod_id>.<tool_name>`) without making `simoder-mcp` a hard dependency for those mods.
- **Transactional Native Mod Installer (`U5`)**: `NativeDevToolInstaller` (`src/packaging.rs`) performs clean-prefix staging, purges retired legacy `Broker/` and `MCP/` `.NET` executables, and rolls back atomically on upgrade failure.

## Build & Verification

```powershell
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target i686-pc-windows-msvc -- -D warnings
cargo test --workspace
cargo test --workspace --target i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc -p simoder-mcp
```

See [rust/DECISIONS.md](rust/DECISIONS.md), [rust/GLOSSARY.md](rust/GLOSSARY.md), [rust/MAP.md](rust/MAP.md), [docs/native-session.md](docs/native-session.md), [docs/runtime-analysis.md](docs/runtime-analysis.md), and [docs/ai/definition-of-done.md](docs/ai/definition-of-done.md).
