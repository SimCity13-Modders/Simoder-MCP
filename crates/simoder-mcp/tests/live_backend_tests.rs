#![forbid(unsafe_code)]

//! Integration tests for [`LiveHostBackend`] filesystem log tailing, runtime state inspection,
//! and Win32 live process module enumeration / checked memory reading.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value as JsonValue, json};
use simoder_mcp::{
    HostDispatchProvider, LiveHostBackend, NativeSessionOperation, NativeSessionRequest,
    RoadDrawType, UiJsEvalFn,
};

#[test]
fn live_backend_reads_real_logs_and_runtime_state_from_game_root() {
    let dir = tempfile::tempdir().unwrap();
    let logs_dir = dir.path().join("simoder").join("logs");
    let mods_dir = dir.path().join("mods").join("sample.mod");
    fs::create_dir_all(&logs_dir).unwrap();
    fs::create_dir_all(&mods_dir).unwrap();

    fs::write(
        logs_dir.join("sc13modloader.log"),
        "[10:00:00] [LOADER] [simoder-d3d9] [INFO] Boot OK\n\
         [10:00:01] [MOD] [sample.mod] [INFO] Sample mod initialized\n\
         [10:00:02] [MOD] [other.mod] [WARN] Other warning\n",
    )
    .unwrap();
    fs::write(dir.path().join("simoder").join("config.toon"), "enabled: true\n").unwrap();
    fs::write(dir.path().join("simoder").join("state.toon"), "enabled[0]:\n").unwrap();
    fs::write(
        mods_dir.join("mod.toml"),
        "schemaVersion = 2\nformat = \"simoder.authoring\"\n\n[metadata]\nid = \"sample.mod\"\nversion = \"1.2.3\"\n",
    )
    .unwrap();

    let backend = LiveHostBackend::new(std::process::id(), Some(dir.path().to_path_buf()));

    let all_logs = backend.get_logs(None, 10).unwrap();
    assert_eq!(all_logs["result"]["lines"].as_array().unwrap().len(), 3);

    let filtered = backend.get_logs(Some("sample.mod"), 10).unwrap();
    let lines = filtered["result"]["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].as_str().unwrap().contains("Sample mod initialized"));

    let runtime_state = backend.get_runtime_state().unwrap();
    assert_eq!(runtime_state["result"]["available"], true);
    assert_eq!(runtime_state["result"]["fresh"], true);
    let mods = runtime_state["result"]["snapshot"]["discoveredMods"]
        .as_array()
        .unwrap();
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0]["id"], "sample.mod");
    assert_eq!(mods[0]["version"], "1.2.3");
}

#[cfg(windows)]
#[test]
fn live_backend_enumerates_current_process_modules_and_reads_checked_memory() {
    let backend = LiveHostBackend::new(std::process::id(), None);

    let modules_res = backend.get_game_modules().unwrap();
    let modules = modules_res["result"]["modules"].as_array().unwrap();
    assert!(!modules.is_empty());
    assert!(
        modules
            .iter()
            .any(|m| m["name"].as_str().unwrap_or("").eq_ignore_ascii_case("kernel32.dll")),
        "Expected kernel32.dll among live process modules"
    );

    let sentinel: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];
    let addr = format!("0x{:X}", sentinel.as_ptr() as usize);
    let mem = backend.read_game_memory(&addr, 4).unwrap();
    assert_eq!(mem["hex"], "deadbeef");
    assert_eq!(mem["length"], 4);

    // Null / invalid memory read must fail cleanly without panicking or crashing.
    assert!(backend.read_game_memory("0x0", 4).is_err());
}

#[test]
fn live_backend_dispatches_session_tools_and_builder_commands_through_js_bridge() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_ref = Arc::clone(&calls);
    let mock_js: UiJsEvalFn = Arc::new(move |_script, packet_json, _timeout_ms| {
        calls_ref.fetch_add(1, Ordering::Relaxed);
        let pkt: JsonValue = serde_json::from_str(packet_json).unwrap();
        let op = pkt["op"].as_str().unwrap_or("");
        let result = match op {
            "observe" => json!({
                "phase": "city",
                "loadingFinalized": true,
                "cityId": "2",
                "cityUid": "300",
                "regionId": "10"
            }),
            "get-build-tools" => json!({
                "selectedToolId": 555,
                "knownCategories": { "roads": 1577763374, "zones": 831766718 },
                "toolsByCategory": { "roads": [{ "toolId": 9001 }] }
            }),
            "select-tool" => json!({
                "requestedToolId": pkt["toolId"],
                "selectedToolId": pkt["toolId"],
                "roadDrawType": pkt["roadDrawType"]
            }),
            "move-camera" => json!({
                "moved": true,
                "command": "camera/immediate/true/move/100, 0, 200/rotate/90"
            }),
            "eval-ui-bridge" => json!({
                "method": pkt["method"],
                "verb": pkt["verb"],
                "response": { "activeBoxId": "2" }
            }),
            other => json!({ "echoOp": other }),
        };
        Ok(json!({ "ok": true, "operation": op, "result": result }).to_string())
    });

    let backend = LiveHostBackend::with_js_bridge(std::process::id(), None, Some(mock_js));

    assert_eq!(
        backend.get_capabilities()["result"]["eawebkitSemanticBridgeReady"],
        true
    );

    let obs = backend
        .native_session(NativeSessionRequest {
            operation: NativeSessionOperation::Observe,
            region_id: None,
            city_id: None,
            template_id: None,
            name: None,
            argument: None,
            timeout_seconds: 5,
        })
        .unwrap();
    assert_eq!(obs["result"]["result"]["phase"], "city");

    let startup = backend.get_startup_state().unwrap();
    assert_eq!(startup["stage"], "inCity");

    let tools = backend.get_build_tools(Some("roads")).unwrap();
    assert_eq!(tools["result"]["toolsByCategory"]["roads"][0]["toolId"], 9001);

    let selected = backend
        .select_tool(Some(9001), false, Some(RoadDrawType::Straight))
        .unwrap();
    assert_eq!(selected["result"]["selectedToolId"], 9001);
    assert_eq!(selected["result"]["roadDrawType"], 1);

    let cam = backend.move_camera(100.0, 0.0, 200.0, Some(90.0)).unwrap();
    assert_eq!(cam["result"]["moved"], true);

    let eval = backend
        .eval_ui_bridge("RequestGameData", "activeBoxId")
        .unwrap();
    assert_eq!(eval["result"]["response"]["activeBoxId"], "2");

    let ui_tree = backend.inspect_ui_tree(Some(241896001), 3, true, 50).unwrap();
    assert_eq!(ui_tree["result"]["echoOp"], "inspect-ui-tree");

    let dom = backend.query_ui_dom("[data-simoder-slot]", 16, true).unwrap();
    assert_eq!(dom["result"]["echoOp"], "query-ui-dom");

    let ui_state = backend.get_ui_state().unwrap();
    assert_eq!(ui_state["result"]["echoOp"], "simoder-ui-state");

    // Full native lifecycle: region templates, sandbox creation, city claim, city load, world inspection.
    let templates = backend
        .native_session(NativeSessionRequest {
            operation: NativeSessionOperation::RegionTemplates,
            region_id: None,
            city_id: None,
            template_id: None,
            name: None,
            argument: None,
            timeout_seconds: 5,
        })
        .unwrap();
    assert_eq!(templates["result"]["result"]["echoOp"], "region-templates");

    let created = backend
        .native_session(NativeSessionRequest {
            operation: NativeSessionOperation::CreateSandboxRegion,
            region_id: None,
            city_id: None,
            template_id: Some(77),
            name: Some("TestSandbox".into()),
            argument: None,
            timeout_seconds: 5,
        })
        .unwrap();
    assert_eq!(
        created["result"]["result"]["echoOp"],
        "create-sandbox-region"
    );

    assert!(calls.load(Ordering::Relaxed) >= 8);
}
