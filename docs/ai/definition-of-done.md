# Simoder-DevTools-MCP Rust Definition of Done (DoD)

Extends the base workspace standard ([definition-of-done.md](../../../.agents/plugins/simcity-ecosystem/skills/simoder-audit/references/definition-of-done.md)) for the target **Rust** stack of `Simoder-DevTools-MCP`.

---

## Rust Target `MUST` Checklist (Blocking)

1. **Protocol, IPC & Authorization Safety**
   - Enforce Rust `edition = "2024"` and `#![forbid(unsafe_code)]` for MCP server, broker, and JSON-RPC transport code (if any Win32 named-pipe/process FFI wrapper is required, isolate it behind a minimal safe boundary with `// SAFETY:` invariants).
   - Validate all sibling paths, executable identities, session tokens/leases, and IPC payloads strictly; never weaken verification to accept patched/untrusted executables or leak secrets/tokens in diagnostics.
   - Zero `.unwrap()` / `.expect()` on IPC messages, JSON payloads, filesystem I/O, or CLI inputs in non-test code.

2. **Crate-First & Minimal Dependency Surface**
   - Use established crates (`serde`, `serde_json`, official/audited MCP/transport/IPC crates, `thiserror`, `parking_lot`, `relative-path`, `anstyle`) instead of hand-rolled JSON-RPC framers, HTTP/pipe parsers, or path canonicalizers.
   - Keep protocol adapters thin and DRY: shared error/response mapping, no duplicated handlers or per-command boilerplate.

3. **Ownership Boundary & YAGNI**
   - Preserve exact DevTools/Broker/MCP sibling contracts without embedding native DLL injection (`Simoder`), author SDK code (`Simoder-SDK`), or game mod hooks inside `Simoder-DevTools-MCP`.
   - Implement only approved MCP tools/resources and session capabilities (no speculative transports, unused config switches, or premature multi-layer frameworks).
   - Keep module layout clean and cohesive (avoid both > ~600-line monoliths and unnecessary micro-files < ~150 lines).

4. **Rustdoc & Verification Gate**
   - Document Rust items with idiomatic Rustdoc (`//!`/`///`), zero `TODO`/`FIXME` placeholders.
   - Verify with offline builds/tests (`cargo test`, `cargo clippy --all-targets` if available, `cargo doc --no-deps`) plus contract/integration tests covering invalid IPC/auth rejection.

---

## `SHOULD` (Non-Blocking) & Stop-Line

- Non-blocking readability or micro-refactor notes are classified as `SHOULD`. Once all `MUST` criteria pass, stop rather than over-engineering or refactoring working code without user approval.
