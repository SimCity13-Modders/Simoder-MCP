#![forbid(unsafe_code)]

//! JSON-RPC 2.0 MCP protocol server engine (`initialize`, `notifications/initialized`,
//! `tools/list`, `tools/call`) bound to the native module lifecycle, client authorization gate,
//! and host dispatch provider (`Simoder-DevTools-MCP#1..#4`).

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{Value as JsonValue, json};
use strum::VariantArray;

use crate::abi::{NativeMcpLifecycle, SdHandle};
use crate::auth::McpAuthManager;
use crate::catalog::BuiltinToolId;
use crate::config::{
    MAX_MCP_FRAME_BYTES, MCP_PROTOCOL_VERSION_2025_11_25, MCP_SERVER_INSTRUCTIONS,
    MCP_SERVER_NAME, MCP_SERVER_VERSION,
};
use crate::host::HostDispatchProvider;
use crate::types::{CallToolResult, McpError, McpToolError, ToolDefinition};

/// Call-scoped RAII guard ensuring [`NativeMcpLifecycle::leave_call`] runs when a tool call finishes.
struct ActiveCallGuard<'a> {
    lifecycle: &'a NativeMcpLifecycle,
    owner: SdHandle,
}

impl Drop for ActiveCallGuard<'_> {
    fn drop(&mut self) {
        self.lifecycle.leave_call(self.owner);
    }
}

/// Complete in-process MCP server engine hosted inside `simoder_mcp.dll`.
///
/// Enforces:
/// - **Lifecycle (`U4` / `#1`)**: Rejects all requests when no native generation is in `ModuleScopeState::Active`.
/// - **Security (`U4` / `#3`)**: Verifies `session_id`, bearer `token`, runtime epoch, explicit agent consent,
///   and required `McpGrant` permissions before every `tools/list` or `tools/call`.
/// - **Catalog & OpenSCP Extensions (`U4` / `#2`, `Simoder#12`)**: Merges the 38 built-in tools with any active
///   mod-registered tools from `Simoder`'s decoupled `HostToolRegistry`.
/// - **Client Interoperability (`U4` / `#4`)**: Preserves exact JSON-RPC 2.0 wire envelopes, `32 MiB` frame cap,
///   `4096`-byte public error truncation, and generic internal error masking.
pub struct McpServerEngine {
    lifecycle: Arc<NativeMcpLifecycle>,
    auth: Mutex<McpAuthManager>,
    host: Arc<dyn HostDispatchProvider>,
}

impl McpServerEngine {
    /// Construct an [`McpServerEngine`] binding `lifecycle`, `auth`, and `host`.
    pub fn new(
        lifecycle: Arc<NativeMcpLifecycle>,
        auth: McpAuthManager,
        host: Arc<dyn HostDispatchProvider>,
    ) -> Self {
        Self {
            lifecycle,
            auth: Mutex::new(auth),
            host,
        }
    }

    /// Borrow the [`NativeMcpLifecycle`] coordinator.
    pub fn lifecycle(&self) -> &NativeMcpLifecycle {
        &self.lifecycle
    }

    /// Mutably access the [`McpAuthManager`] under its lock.
    pub fn with_auth_mut<R>(&self, f: impl FnOnce(&mut McpAuthManager) -> R) -> R {
        let mut guard = self.auth.lock();
        f(&mut guard)
    }

    /// Return the combined, alphabetically sorted list of built-in tools and active mod-registered tools.
    pub fn list_all_tools(&self) -> Vec<ToolDefinition> {
        let mut tools: Vec<ToolDefinition> = BuiltinToolId::VARIANTS
            .iter()
            .map(|id| id.definition())
            .collect();
        for ext in self.host.list_external_mod_tools() {
            tools.push(ext.definition);
        }
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    /// Process a raw UTF-8 JSON-RPC 2.0 request frame for an authenticated client session.
    ///
    /// Returns `Ok(None)` for JSON-RPC notifications (such as `notifications/initialized`),
    /// `Ok(Some(response_json))` for requests with an `id`, or `Err(McpError)` on transport/frame/lifecycle failure.
    pub fn handle_raw_frame(
        &self,
        raw_frame: &str,
        session_id: &str,
        raw_token: &str,
    ) -> Result<Option<JsonValue>, McpError> {
        if raw_frame.len() > MAX_MCP_FRAME_BYTES {
            return Err(McpError::FrameTooLarge(raw_frame.len()));
        }
        let trimmed = raw_frame.trim();
        if trimmed.is_empty() {
            return Err(McpError::InvalidJson("empty EOF frame".into()));
        }
        let req: JsonValue = serde_json::from_str(trimmed)
            .map_err(|e| McpError::InvalidJson(e.to_string()))?;
        self.handle_jsonrpc(&req, session_id, raw_token)
    }

    /// Process a parsed JSON-RPC 2.0 message object.
    pub fn handle_jsonrpc(
        &self,
        request: &JsonValue,
        session_id: &str,
        raw_token: &str,
    ) -> Result<Option<JsonValue>, McpError> {
        // 1. Lifecycle gate: no running game / active admitted mod means no callable MCP!
        let Ok(owner) = self.lifecycle.try_enter_call() else {
            return Err(McpError::ServiceUnavailable);
        };
        let _call_guard = ActiveCallGuard {
            lifecycle: &self.lifecycle,
            owner,
        };

        let method = request
            .get("method")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| McpError::InvalidJson("missing JSON-RPC 'method' field".into()))?;
        let id = request.get("id").cloned();

        // JSON-RPC notifications carry no `id` and produce no response frame.
        if id.is_none() || method.starts_with("notifications/") {
            return Ok(None);
        }
        let req_id = id.unwrap_or(JsonValue::Null);
        let params = request.get("params").unwrap_or(&JsonValue::Null);

        match method {
            "initialize" => {
                // Verify client identity & basic session consent before completing handshake.
                self.auth.lock().authorize_call(session_id, raw_token, &[])?;
                let requested_version = params
                    .get("protocolVersion")
                    .and_then(JsonValue::as_str)
                    .unwrap_or(MCP_PROTOCOL_VERSION_2025_11_25);
                Ok(Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "protocolVersion": requested_version,
                        "capabilities": {
                            "logging": {},
                            "tools": {
                                "listChanged": true
                            }
                        },
                        "serverInfo": {
                            "name": MCP_SERVER_NAME,
                            "version": MCP_SERVER_VERSION
                        },
                        "instructions": MCP_SERVER_INSTRUCTIONS
                    }
                })))
            }
            "ping" => {
                self.auth.lock().authorize_call(session_id, raw_token, &[])?;
                Ok(Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {}
                })))
            }
            "resources/list" => {
                self.auth.lock().authorize_call(session_id, raw_token, &[])?;
                Ok(Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "resources": []
                    }
                })))
            }
            "resources/templates/list" => {
                self.auth.lock().authorize_call(session_id, raw_token, &[])?;
                Ok(Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "resourceTemplates": []
                    }
                })))
            }
            "prompts/list" => {
                self.auth.lock().authorize_call(session_id, raw_token, &[])?;
                Ok(Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "prompts": []
                    }
                })))
            }
            "tools/list" => {
                // Anonymous catalog listing is forbidden (`ADR 0001`).
                self.auth.lock().authorize_call(session_id, raw_token, &[])?;
                let tools = self.list_all_tools();
                Ok(Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "tools": tools
                    }
                })))
            }
            "tools/call" => {
                let Some(tool_name) = params.get("name").and_then(JsonValue::as_str) else {
                    return Ok(Some(json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": {
                            "code": -32602,
                            "message": "Missing required 'name' parameter in tools/call"
                        }
                    })));
                };
                let empty_args = json!({});
                let arguments = params.get("arguments").unwrap_or(&empty_args);

                let call_result = self.dispatch_tool_call(tool_name, arguments, session_id, raw_token);
                Ok(Some(json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": call_result
                })))
            }
            unknown => Ok(Some(json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {unknown}")
                }
            }))),
        }
    }

    fn dispatch_tool_call(
        &self,
        tool_name: &str,
        arguments: &JsonValue,
        session_id: &str,
        raw_token: &str,
    ) -> CallToolResult {
        // 1. Check if it is a built-in tool.
        if let Ok(builtin) = tool_name.parse::<BuiltinToolId>() {
            let req_grant = vec![builtin.required_grant().as_str().to_string()];
            if let Err(auth_err) = self
                .auth
                .lock()
                .authorize_call(session_id, raw_token, &req_grant)
            {
                self.host.log_event(
                    "warn",
                    "simoder-mcp",
                    &format!("tools/call '{tool_name}' denied: {auth_err}"),
                );
                return CallToolResult::error(tool_name, &McpToolError::public(auth_err.to_string()));
            }
            return match builtin.execute(arguments, self.host.as_ref()) {
                Ok(ok_res) => {
                    self.host.log_event(
                        "info",
                        "simoder-mcp",
                        &format!("tools/call '{tool_name}' -> ok"),
                    );
                    ok_res
                }
                Err(tool_err) => {
                    self.host.log_event(
                        "warn",
                        "simoder-mcp",
                        &format!("tools/call '{tool_name}' -> error: {tool_err:?}"),
                    );
                    CallToolResult::error(tool_name, &tool_err)
                }
            };
        }

        // 2. Otherwise check decoupled external mod tools registered in the host (`ADR-004`).
        let ext_tools = self.host.list_external_mod_tools();
        let Some(ext) = ext_tools.iter().find(|t| t.definition.name == tool_name) else {
            self.host.log_event(
                "warn",
                "simoder-mcp",
                &format!("tools/call '{tool_name}' -> unknown tool"),
            );
            return CallToolResult::error(
                tool_name,
                &McpToolError::public(format!("unknown or unregistered tool '{tool_name}'")),
            );
        };
        if let Err(auth_err) = self
            .auth
            .lock()
            .authorize_call(session_id, raw_token, &ext.required_grants)
        {
            self.host.log_event(
                "warn",
                "simoder-mcp",
                &format!("tools/call '{tool_name}' denied: {auth_err}"),
            );
            return CallToolResult::error(tool_name, &McpToolError::public(auth_err.to_string()));
        }
        match self
            .host
            .invoke_external_mod_tool(tool_name, arguments, &ext.required_grants)
        {
            Ok(payload) => {
                self.host.log_event(
                    "info",
                    "simoder-mcp",
                    &format!("tools/call '{tool_name}' -> ok"),
                );
                CallToolResult::success(payload)
            }
            Err(tool_err) => {
                self.host.log_event(
                    "warn",
                    "simoder-mcp",
                    &format!("tools/call '{tool_name}' -> error: {tool_err:?}"),
                );
                CallToolResult::error(tool_name, &tool_err)
            }
        }
    }
}
