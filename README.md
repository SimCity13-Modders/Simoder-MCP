# Simoder-MCP

[![Download Latest (Nightly)](https://img.shields.io/github/actions/workflow/status/SimCity13-Modders/Simoder-MCP/ci.yml?branch=main&label=Download%20latest%20(nightly)&logo=githubactions&logoColor=white)](https://nightly.link/SimCity13-Modders/Simoder-MCP/workflows/ci/main/simoder-mcp-windows-x86.zip)
[![Download Latest Release](https://img.shields.io/github/v/release/SimCity13-Modders/Simoder-MCP?label=Download%20latest%20release&logo=github)](https://github.com/SimCity13-Modders/Simoder-MCP/releases/latest)
[![Star](https://img.shields.io/github/stars/SimCity13-Modders/Simoder-MCP?style=social)](https://github.com/SimCity13-Modders/Simoder-MCP/stargazers)

**Simoder-MCP** (`simoder.devtools` -> `simoder_mcp.dll`) is the optional, game-owned **Model Context Protocol (MCP)** DevTool native mod for **Simoder**. Built in Rust (`edition = "2024"`), it runs inside the game process over Simoder's `ABI v2` Native Module Host Boundary (`SimoderModule_QueryV2`, `PrepareV2`, `AdmitV2`, `QuiesceV2`, `DrainV2`) and lets MCP clients (such as Antigravity, Claude, or Codex) inspect live game state, capture screenshots, drive UI, and invoke mod-registered tools with explicit user consent and grant enforcement.

## Dependencies & Requirements

- **Runtime**: [Simoder](https://github.com/SimCity13-Modders/Simoder) (`d3d9.dll`) + [Simoder-Base](https://github.com/SimCity13-Modders/Simoder-Base) (`mods/simoder.base`)
- **Trust Approval**: Native `.dll` mods require explicit one-time SHA-256 approval in Simoder's in-game **Mods → Native DLLs** tab (`simoder/native_trust.toon`).
- **Building from source (optional)**: Rust toolchain with `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc` targets.

## Installation

1. Install [Simoder](https://github.com/SimCity13-Modders/Simoder) and [Simoder-Base](https://github.com/SimCity13-Modders/Simoder-Base).
2. Download `simoder_mcp.dll` + `mod.toml` from the **Release** or **Nightly** badge above (or build `target/i686-pc-windows-msvc/release/simoder_mcp.dll`).
3. Place them in `<SimCity Install>\mods\simoder.devtools\`:

```text
<SimCity Install>/
  mods/
    simoder.devtools/
      mod.toml
      simoder_mcp.dll
```

4. Launch SimCity, open **Mods → Native DLLs**, and click **Approve** on **Simoder Developer Tools (Native MCP)**.
5. Point your MCP client at the local Simoder MCP endpoint using the generated bearer token in `<SimCity Install>\simoder\mcp_token.txt`.

---

## Architecture & Security Model

- **Game-Bound Lifecycle**: `PrepareV2` stages the module; `AdmitV2` starts the MCP host only after native trust and `Simoder-Base` readiness; `QuiesceV2` and `DrainV2` cleanly close admission on disable or exit.
- **Consent & Grants (`McpAuthManager`)**: Every `initialize`, `tools/list`, and `tools/call` request validates client identity, SHA-256 token digest, runtime epoch, explicit user consent, and fine-grained `McpGrant` scopes (`session.read`, `session.mutate`, `world.inspect`, `mods.control`, `memory.analysis`, `ui.input`, `openscp.diagnostics`, plus mod namespace grants).
- **Decoupled Mod Tool Extensions**: Automatically exposes optional tools registered by other mods in `Simoder`'s `HostToolRegistry` (e.g. `simoder.ui_inspector.*`) without hard dependencies.

## Built-in MCP Tools (38 Tools)

| Category | Tools |
| --- | --- |
| **Session & Lifecycle** | `simcity_get_startup_state`, `simcity_start_session`, `simcity_enter_game`, `simcity_confirm_stage`, `simcity_get_native_session`, `simcity_close_game`, `simcity_get_diagnostics` |
| **Window & Input** | `simcity_capture_window`, `simcity_click`, `simcity_drag`, `simcity_scroll_view`, `simcity_press_key`, `simcity_type_text`, `simcity_eval_ui_bridge` |
| **Regions, Cities & World** | `simcity_get_region_templates`, `simcity_create_sandbox_region`, `simcity_get_region_details`, `simcity_claim_city`, `simcity_load_city`, `simcity_get_build_tools`, `simcity_assign_zone`, `simcity_build_road`, `simcity_place_building`, `simcity_inspect_world` |
| **Mods & Scripts** | `simcity_list_mods`, `simcity_set_mod_enabled`, `simcity_reload_mod`, `simcity_list_patches`, `simcity_run_script` |
| **Diagnostics & OpenSCP** | `simoder_unlock_debug_tools`, `simoder_get_bridge_telemetry`, `simoder_inspect_sldelta`, `simcity_inspect_memory`, `simcity_read_memory`, `simcity_scan_signature`, `simcity_export_rea_bundle`, `simcity_list_logs`, `simcity_tail_logs` |

*(Plus dynamic `<mod_id>.<tool_name>` tools contributed by loaded mods such as [Simoder-UI-Inspector](https://github.com/SimCity13-Modders/Simoder-UI-Inspector).)*

## Build & Verification

```powershell
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target i686-pc-windows-msvc -- -D warnings
cargo test --workspace
cargo test --workspace --target i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc -p simoder-mcp
```

See [rust/DECISIONS.md](rust/DECISIONS.md), [rust/MAP.md](rust/MAP.md), [docs/native-session.md](docs/native-session.md), and [docs/ai/definition-of-done.md](docs/ai/definition-of-done.md).
