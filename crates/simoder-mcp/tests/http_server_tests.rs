#![forbid(unsafe_code)]

//! Integration tests for the in-process Streamable HTTP MCP transport (`http_server.rs`).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;

use serde_json::{Value as JsonValue, json};
use simoder_mcp::{
    McpAuthManager, McpHttpServerHandle, McpServerEngine, NativeMcpLifecycle, SdHandle,
    SdHostApiV2, StandardHostBackend,
};

fn send_http(addr: SocketAddr, req: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect to loopback HTTP MCP");
    stream.write_all(req.as_bytes()).expect("write HTTP request");
    stream.flush().expect("flush HTTP request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read HTTP response");

    let (head, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or((&response, ""));
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    (status, body.to_string())
}

#[test]
fn streamable_http_server_serves_health_and_mcp_only_while_admitted_and_closes_on_quiesce() {
    let lifecycle = Arc::new(NativeMcpLifecycle::new());
    let owner = SdHandle {
        slot: 1,
        generation: 1,
    };
    lifecycle.prepare_v2(&SdHostApiV2::new(owner));
    lifecycle.admit_v2(owner);

    let mut auth = McpAuthManager::new(1);
    auth.register_full_session("sess-http", "http-test-client", "tok-http");
    let backend = Arc::new(StandardHostBackend::new(std::process::id()));
    let engine = Arc::new(McpServerEngine::new(
        Arc::clone(&lifecycle),
        auth,
        backend,
    ));

    let server = McpHttpServerHandle::start(
        "127.0.0.1:0",
        Arc::clone(&engine),
        "sess-http".to_string(),
        "tok-http".to_string(),
        true,
    )
    .expect("start ephemeral HTTP MCP server");
    let addr = server.local_addr();

    // Reject non-loopback bind addresses unconditionally (plaintext non-loopback forbidden per ADR 0001).
    assert!(
        McpHttpServerHandle::start(
            "0.0.0.0:47399",
            Arc::clone(&engine),
            "sess-http".to_string(),
            "tok-http".to_string(),
            true,
        )
        .is_err()
    );

    // 1. GET /health
    let (status, body) = send_http(
        addr,
        &format!("GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(status, 200);
    let health: JsonValue = serde_json::from_str(&body).unwrap();
    assert_eq!(health["status"], "ok");
    assert_eq!(health["active"], true);
    assert_eq!(health["gamePid"], std::process::id());

    // 2. Security gates: missing Bearer token -> 401; non-loopback Host/Origin -> 403.
    let rpc = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "simoder_get_service_info",
            "arguments": {}
        }
    })
    .to_string();

    let anon_req = format!(
        "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{rpc}",
        rpc.len()
    );
    assert_eq!(send_http(addr, &anon_req).0, 401);

    let bad_host_req = format!(
        "POST /mcp HTTP/1.1\r\nHost: evil.example:47391\r\nAuthorization: Bearer tok-http\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{rpc}",
        rpc.len()
    );
    assert_eq!(send_http(addr, &bad_host_req).0, 403);

    let bad_origin_req = format!(
        "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nOrigin: https://localhost.evil.com\r\nAuthorization: Bearer tok-http\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{rpc}",
        rpc.len()
    );
    assert_eq!(send_http(addr, &bad_origin_req).0, 403);

    // 3. Authenticated loopback POST /mcp tools/call simoder_get_service_info -> 200
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://localhost\r\nAuthorization: Bearer tok-http\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{rpc}",
        rpc.len()
    );
    let (status, body) = send_http(addr, &req);
    assert_eq!(status, 200);
    let rpc_resp: JsonValue = serde_json::from_str(&body).unwrap();
    assert_eq!(rpc_resp["result"]["isError"], false);
    assert_eq!(
        rpc_resp["result"]["structuredContent"]["result"]["gamePid"],
        std::process::id()
    );

    // 4. Verify OPTIONS, chunked Transfer-Encoding, notification 202, and empty list methods.
    let (opt_status, _) = send_http(
        addr,
        &format!("OPTIONS /mcp HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(opt_status, 204);

    let init_json = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "chunked-test", "version": "1.0" }
        }
    })
    .to_string();
    let split_at = init_json.len() / 2;
    let chunk1 = &init_json[..split_at];
    let chunk2 = &init_json[split_at..];
    let chunked_req = format!(
        "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer tok-http\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{chunk1}\r\n{:x}\r\n{chunk2}\r\n0\r\n\r\n",
        chunk1.len(),
        chunk2.len()
    );
    let (chunk_status, chunk_body) = send_http(addr, &chunked_req);
    assert_eq!(chunk_status, 200);
    let chunk_resp: JsonValue = serde_json::from_str(&chunk_body).unwrap();
    assert_eq!(chunk_resp["result"]["serverInfo"]["name"], "simoder-mcp");

    let notif = json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string();
    let (notif_status, _) = send_http(
        addr,
        &format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer tok-http\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{notif}",
            notif.len()
        ),
    );
    assert_eq!(notif_status, 202);

    for (method, expected_field) in [
        ("resources/list", "resources"),
        ("resources/templates/list", "resourceTemplates"),
        ("prompts/list", "prompts"),
    ] {
        let body = json!({"jsonrpc": "2.0", "id": 11, "method": method, "params": {}}).to_string();
        let (st, resp_body) = send_http(
            addr,
            &format!(
                "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer tok-http\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        );
        assert_eq!(st, 200);
        let parsed: JsonValue = serde_json::from_str(&resp_body).unwrap();
        assert_eq!(parsed["result"][expected_field], json!([]));
    }

    // 5. Quiesce closes request admission (503) and stop() shuts down socket completely.
    lifecycle.quiesce_v2(owner);
    let (status_after_quiesce, _) = send_http(addr, &req);
    assert_eq!(status_after_quiesce, 503);

    server.stop();
    assert!(TcpStream::connect(addr).is_err());
}
