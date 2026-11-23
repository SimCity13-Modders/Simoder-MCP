#![forbid(unsafe_code)]

//! In-process Streamable HTTP (`127.0.0.1:47391/mcp`) transport server hosted inside `simoder_mcp.dll`
//! when `SimCity.exe` is running and `simoder.devtools.mcp` is admitted (`ADR 0001`).
//!
//! Enforces loopback-only default binding, bounded request framing ([`MAX_MCP_FRAME_BYTES`]),
//! client authorization (`McpAuthManager`), and immediate shutdown on `QuiesceV2` / process exit.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_CONCURRENT_CONNECTIONS: usize = 16;

struct ConnectionGuard(Arc<AtomicUsize>);

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

enum ChunkedReadError {
    TooLarge,
    Malformed,
}

use parking_lot::Mutex;
use serde_json::{Value as JsonValue, json};

use crate::config::{
    MAX_MCP_FRAME_BYTES, MCP_HTTP_ENDPOINT_PATH, MCP_HTTP_HEALTH_PATH, MCP_SERVER_NAME,
    MCP_SERVER_VERSION,
};
use crate::protocol::McpServerEngine;
use crate::types::McpError;

/// Active handle to the background Streamable HTTP MCP listener thread.
pub struct McpHttpServerHandle {
    local_addr: SocketAddr,
    stop_flag: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl McpHttpServerHandle {
    /// Validate `bind_addr_str`, bind a loopback TCP listener, and start the HTTP worker thread.
    ///
    /// Unconditionally rejects non-loopback bind addresses because plaintext HTTP without TLS and
    /// certificate validation violates `ADR 0001`.
    pub fn start(
        bind_addr_str: &str,
        engine: Arc<McpServerEngine>,
        default_session_id: String,
        _default_token: String,
        _require_explicit_token: bool,
    ) -> Result<Self, String> {
        let addr: SocketAddr = bind_addr_str
            .parse()
            .map_err(|e| format!("invalid MCP HTTP bind address '{bind_addr_str}': {e}"))?;

        if !addr.ip().is_loopback() {
            return Err(format!(
                "refusing non-loopback MCP bind address '{addr}': plaintext HTTP is forbidden outside loopback; TLS and certificate validation are required (ADR 0001)"
            ));
        }

        let listener =
            TcpListener::bind(addr).map_err(|e| format!("failed to bind MCP HTTP {addr}: {e}"))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| format!("failed to inspect local MCP HTTP address: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("failed to configure nonblocking MCP HTTP listener: {e}"))?;

        let stop_flag = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop_flag);

        let handle = thread::Builder::new()
            .name("simoder-mcp-http".to_string())
            .spawn(move || {
                accept_loop(
                    listener,
                    worker_stop,
                    engine,
                    default_session_id,
                );
            })
            .map_err(|e| format!("failed to spawn MCP HTTP thread: {e}"))?;

        Ok(Self {
            local_addr,
            stop_flag,
            worker: Mutex::new(Some(handle)),
        })
    }

    /// Return the actual bound loopback [`SocketAddr`].
    pub const fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Signal the HTTP listener thread to terminate and wait briefly for it to exit.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(handle) = self.worker.lock().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for McpHttpServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_loop(
    listener: TcpListener,
    stop_flag: Arc<AtomicBool>,
    engine: Arc<McpServerEngine>,
    default_session_id: String,
) {
    let active_connections = Arc::new(AtomicUsize::new(0));
    while !stop_flag.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _peer)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                let acquired = active_connections.try_update(
                    Ordering::AcqRel,
                    Ordering::Acquire,
                    |n| (n < MAX_CONCURRENT_CONNECTIONS).then_some(n + 1),
                );
                if acquired.is_err() {
                    write_jsonrpc_http_error(
                        &mut stream,
                        "503 Service Unavailable",
                        -32000,
                        "Too many concurrent MCP HTTP connections",
                    );
                    continue;
                }
                let guard = ConnectionGuard(Arc::clone(&active_connections));
                let conn_engine = Arc::clone(&engine);
                let conn_session = default_session_id.clone();
                let _ = thread::Builder::new()
                    .name("simoder-mcp-conn".to_string())
                    .spawn(move || {
                        let _guard = guard;
                        handle_connection(stream, &conn_engine, &conn_session);
                    });
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(15));
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    engine: &Arc<McpServerEngine>,
    default_session_id: &str,
) {
    let Ok(cloned) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(cloned);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        write_http_response(&mut stream, "400 Bad Request", "text/plain", b"Malformed HTTP request line", None);
        return;
    }
    let method = parts[0];
    let raw_target = parts[1];
    let path = raw_target.split('?').next().unwrap_or(raw_target);

    let mut content_length: usize = 0;
    let mut is_chunked = false;
    let mut auth_bearer: Option<String> = None;
    let mut session_header: Option<String> = None;
    let mut host_header: Option<String> = None;
    let mut origin_header: Option<String> = None;

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).unwrap_or(0);
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, val)) = line.trim_end().split_once(':') {
            let lower = name.trim().to_ascii_lowercase();
            let trimmed_val = val.trim();
            if lower == "content-length" {
                content_length = trimmed_val.parse::<usize>().unwrap_or(0);
            } else if lower == "transfer-encoding" {
                if trimmed_val
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
                {
                    is_chunked = true;
                }
            } else if lower == "authorization" {
                if let Some(tok) = trimmed_val
                    .strip_prefix("Bearer ")
                    .or_else(|| trimmed_val.strip_prefix("bearer "))
                {
                    auth_bearer = Some(tok.trim().to_string());
                }
            } else if lower == "mcp-session-id" && !trimmed_val.is_empty() {
                session_header = Some(trimmed_val.to_string());
            } else if lower == "host" && !trimmed_val.is_empty() {
                host_header = Some(trimmed_val.to_string());
            } else if lower == "origin" && !trimmed_val.is_empty() {
                origin_header = Some(trimmed_val.to_string());
            }
        }
    }

    let body = if is_chunked {
        match read_chunked_body(&mut reader, MAX_MCP_FRAME_BYTES) {
            Ok(bytes) => bytes,
            Err(ChunkedReadError::TooLarge) => {
                write_http_response(
                    &mut stream,
                    "413 Payload Too Large",
                    "application/json",
                    br#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Frame exceeds 32 MiB limit"}}"#,
                    None,
                );
                return;
            }
            Err(ChunkedReadError::Malformed) => {
                write_http_response(
                    &mut stream,
                    "400 Bad Request",
                    "text/plain",
                    b"Malformed chunked HTTP body",
                    None,
                );
                return;
            }
        }
    } else {
        if content_length > MAX_MCP_FRAME_BYTES {
            write_http_response(
                &mut stream,
                "413 Payload Too Large",
                "application/json",
                br#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Frame exceeds 32 MiB limit"}}"#,
                None,
            );
            return;
        }
        let mut buf = vec![0u8; content_length];
        if content_length > 0 && reader.read_exact(&mut buf).is_err() {
            write_http_response(
                &mut stream,
                "400 Bad Request",
                "text/plain",
                b"Incomplete HTTP body",
                None,
            );
            return;
        }
        buf
    };

    match (method, path) {
        ("GET", MCP_HTTP_HEALTH_PATH) => {
            let payload = json!({
                "status": "ok",
                "server": MCP_SERVER_NAME,
                "version": MCP_SERVER_VERSION,
                "transport": "streamable-http",
                "gamePid": std::process::id(),
                "active": engine.lifecycle().active_owner().is_some(),
            });
            let serialized = serde_json::to_vec(&payload).unwrap_or_else(|_| b"{}".to_vec());
            write_http_response(&mut stream, "200 OK", "application/json; charset=utf-8", &serialized, None);
        }
        ("GET", MCP_HTTP_ENDPOINT_PATH) => {
            write_http_response(
                &mut stream,
                "405 Method Not Allowed",
                "text/plain",
                b"Use POST /mcp for Streamable HTTP JSON-RPC requests",
                Some(("Allow", "POST, DELETE, OPTIONS")),
            );
        }
        ("OPTIONS", MCP_HTTP_ENDPOINT_PATH) => {
            write_http_response(
                &mut stream,
                "204 No Content",
                "text/plain",
                b"",
                Some(("Allow", "POST, DELETE, OPTIONS")),
            );
        }
        ("DELETE", MCP_HTTP_ENDPOINT_PATH) => {
            write_http_response(&mut stream, "204 No Content", "text/plain", b"", None);
        }
        ("POST", MCP_HTTP_ENDPOINT_PATH) => {
            if !is_valid_loopback_host(host_header.as_deref()) {
                write_jsonrpc_http_error(
                    &mut stream,
                    "403 Forbidden",
                    -32001,
                    "Rejected non-loopback or missing Host header (DNS rebinding protection)",
                );
                return;
            }
            if let Some(origin) = origin_header.as_deref()
                && !is_valid_loopback_origin(origin)
            {
                write_jsonrpc_http_error(
                    &mut stream,
                    "403 Forbidden",
                    -32001,
                    "Rejected non-loopback Origin header",
                );
                return;
            }

            let Ok(body_str) = std::str::from_utf8(&body) else {
                write_http_response(
                    &mut stream,
                    "400 Bad Request",
                    "application/json",
                    br#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Request body must be valid UTF-8"}}"#,
                    None,
                );
                return;
            };

            let session_id = session_header.as_deref().unwrap_or(default_session_id);
            let Some(token) = auth_bearer.as_deref().filter(|t| !t.is_empty()) else {
                write_jsonrpc_http_error(
                    &mut stream,
                    "401 Unauthorized",
                    -32001,
                    "Missing Authorization Bearer token (anonymous loopback access is forbidden per ADR 0001)",
                );
                return;
            };

            match engine.handle_raw_frame(body_str, session_id, token) {
                Ok(Some(response_json)) => {
                    let bytes = serde_json::to_vec(&response_json).unwrap_or_else(|_| b"{}".to_vec());
                    write_http_response(
                        &mut stream,
                        "200 OK",
                        "application/json; charset=utf-8",
                        &bytes,
                        Some(("Mcp-Session-Id", session_id)),
                    );
                }
                Ok(None) => {
                    write_http_response(
                        &mut stream,
                        "202 Accepted",
                        "application/json; charset=utf-8",
                        b"",
                        Some(("Mcp-Session-Id", session_id)),
                    );
                }
                Err(McpError::ServiceUnavailable) => {
                    write_jsonrpc_http_error(&mut stream, "503 Service Unavailable", -32000, "MCP service is not active (game stopped or module quiesced)");
                }
                Err(McpError::AuthorizationDenied(msg)) => {
                    write_jsonrpc_http_error(&mut stream, "401 Unauthorized", -32001, &msg);
                }
                Err(err) => {
                    write_jsonrpc_http_error(&mut stream, "400 Bad Request", -32600, &err.to_string());
                }
            }
        }
        _ => {
            write_http_response(&mut stream, "404 Not Found", "text/plain", b"Not Found", None);
        }
    }
}

fn read_chunked_body<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Vec<u8>, ChunkedReadError> {
    let mut out = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return Err(ChunkedReadError::Malformed);
        }
        let size_hex = line
            .trim_end_matches(['\r', '\n'])
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        let Ok(chunk_len) = usize::from_str_radix(size_hex, 16) else {
            return Err(ChunkedReadError::Malformed);
        };
        if chunk_len == 0 {
            loop {
                line.clear();
                let n = reader.read_line(&mut line).unwrap_or(0);
                if n == 0 || line == "\r\n" || line == "\n" {
                    break;
                }
            }
            return Ok(out);
        }
        if chunk_len > max_bytes || out.len().saturating_add(chunk_len) > max_bytes {
            return Err(ChunkedReadError::TooLarge);
        }
        let start = out.len();
        out.resize(start + chunk_len, 0);
        if reader.read_exact(&mut out[start..]).is_err() {
            return Err(ChunkedReadError::Malformed);
        }
        line.clear();
        let crlf_n = reader.read_line(&mut line).unwrap_or(0);
        if crlf_n == 0 || (line != "\r\n" && line != "\n") {
            return Err(ChunkedReadError::Malformed);
        }
    }
}

fn write_jsonrpc_http_error(stream: &mut TcpStream, status: &str, code: i64, message: &str) {
    let val: JsonValue = json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": code,
            "message": message,
        }
    });
    let bytes = serde_json::to_vec(&val).unwrap_or_else(|_| b"{}".to_vec());
    write_http_response(stream, status, "application/json; charset=utf-8", &bytes, None);
}

fn is_valid_loopback_host(host: Option<&str>) -> bool {
    let Some(raw) = host.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    is_loopback_authority(raw)
}

fn is_valid_loopback_origin(origin: &str) -> bool {
    let trimmed = origin.trim();
    let rest = if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("http://") {
        &trimmed[7..]
    } else if trimmed.len() >= 8 && trimmed[..8].eq_ignore_ascii_case("https://") {
        &trimmed[8..]
    } else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    !authority.is_empty() && is_loopback_authority(authority)
}

fn is_loopback_authority(authority: &str) -> bool {
    if let Some(rest) = authority.strip_prefix("[::1]") {
        return rest.is_empty()
            || rest
                .strip_prefix(':')
                .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    }
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    if let Some(p) = port
        && (p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost")
}

fn write_http_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    extra_header: Option<(&str, &str)>,
) {
    let mut header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some((k, v)) = extra_header {
        header.push_str(&format!("{k}: {v}\r\n"));
    }
    header.push_str("\r\n");
    let _ = stream.write_all(header.as_bytes());
    if !body.is_empty() {
        let _ = stream.write_all(body);
    }
    let _ = stream.flush();
}
