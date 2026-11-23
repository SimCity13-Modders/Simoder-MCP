# MCP Package Rules (`Simoder-DevTools-MCP`)

- Own the optional native MCP DevTool mod (`crates/simoder-mcp` -> `simoder_mcp.dll`), its `ABI v2` lifecycle boundary, client authentication/grant gate, tool catalog, `Simoder-OpenSCP` diagnostic extensions, and `U5` transactional installer/rollback logic.
- Communicate with `Simoder` (`d3d9.dll`) through the `ABI v2` host boundary (`SimoderModule_QueryV2`..`SimoderModule_DrainV2`) and `HostDispatchProvider` without duplicating or embedding `Simoder`'s internal runtime crates.
- Follow [docs/ai/definition-of-done.md](docs/ai/definition-of-done.md): target Rust `edition = "2024"`, enforce `#![forbid(unsafe_code)]` across internal modules (isolating the 5 `SimoderModule_*V2` C ABI exports in `src/abi.rs` with `// SAFETY:` comments), document all modules and public items with idiomatic Rustdoc (`//!` and `///`), and verify on both `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`.
- Keep raw investigation evidence in ignored `.analysis/` and architecture decision records in `rust/DECISIONS.md`.
