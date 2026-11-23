//! Comprehensive integration tests for `simoder-mcp` (`[Rust-6]` / `U4` / `U5`):
//! - Native Module `ABI v2` lifecycle (`SimoderModule_QueryV2`..`DrainV2`, single-owner guard,
//!   no callable MCP when stopped/quiesced, `SD_BUSY` drain retention) (`Simoder-DevTools-MCP#1`),
//! - Tool catalog parity + `Simoder-OpenSCP` extensions (`simoder_unlock_debug_tools`,
//!   `simoder_get_bridge_telemetry`, `simoder_inspect_sldelta`, `simoder_eval_ui_bridge`)
//!   + decoupled mod tool extensions (`ADR-004`) (`Simoder-DevTools-MCP#2`, `Simoder#12`),
//! - Client identity, explicit agent consent, permission grants, epoch freshness, and live
//!   revocation without side effects (`Simoder-DevTools-MCP#3`),
//! - JSON-RPC 2.0 wire interoperability, `4096`-byte native error truncation, generic internal
//!   error masking, `32 MiB` frame guard, and partial/unknown mutation handling (`Simoder-DevTools-MCP#4`),
//! - `U5` clean-prefix installation, legacy `.exe` removal, atomic upgrade, and rollback (`Simoder-DevTools#1`, `#2`).

use std::fs;
use std::sync::Arc;

use serde_json::{Value as JsonValue, json};
use simoder_mcp::{
    BackendSimulationMode, BuiltinToolId, ExternalModTool, MAX_MCP_ERROR_MESSAGE_BYTES,
    MAX_MCP_FRAME_BYTES, McpAuthManager, McpError, McpGrant, McpServerEngine, ModuleScopeState,
    NATIVE_MCP_DLL_FILENAME, NativeDevToolInstaller, NativeDevToolPackageSpec, NativeMcpLifecycle,
    RETIRED_LEGACY_SERVICE_PATHS, SD_CONTRACT_SHA256_HEX, SD_U2A_HOST_VERSION, SdHandle,
    SdHostApiV2, SdModuleDescriptorV2, SdResult, StandardHostBackend, ToolAnnotations,
    ToolDefinition,
};
use strum::VariantArray;

fn create_admitted_engine() -> (McpServerEngine, Arc<StandardHostBackend>, SdHandle) {
    let lifecycle = Arc::new(NativeMcpLifecycle::new());
    let owner = SdHandle {
        slot: 1,
        generation: 1,
    };
    let host_api = SdHostApiV2::new(owner);
    assert_eq!(lifecycle.prepare_v2(&host_api), SdResult::Ok);
    assert_eq!(lifecycle.admit_v2(owner), SdResult::Ok);

    let mut auth = McpAuthManager::new(1);
    auth.register_full_session("sess-1", "antigravity-client", "secret-token-1");

    let backend = Arc::new(StandardHostBackend::new(51960));
    let engine = McpServerEngine::new(lifecycle, auth, Arc::clone(&backend) as _);
    (engine, backend, owner)
}

#[test]
fn u4_issue1_native_mod_abi_v2_enforces_single_owner_and_blocks_mcp_when_stopped_or_quiesced() {
    let lifecycle = Arc::new(NativeMcpLifecycle::new());

    // 1. QueryV2 validates host version 2 and populates exact canonical SD_CONTRACT_SHA256_HEX.
    let mut desc = SdModuleDescriptorV2::new_for_query();
    assert_eq!(lifecycle.query_v2(1, &mut desc), SdResult::Version);
    assert_eq!(
        lifecycle.query_v2(SD_U2A_HOST_VERSION, &mut desc),
        SdResult::Ok
    );
    assert_eq!(hex::encode(desc.contract_sha256), SD_CONTRACT_SHA256_HEX);

    let mut auth = McpAuthManager::new(1);
    auth.register_full_session("sess-1", "codex-client", "tok-1");
    let backend = Arc::new(StandardHostBackend::new(4242));
    let engine = McpServerEngine::new(Arc::clone(&lifecycle), auth, backend);

    // 2. Before PrepareV2 / AdmitV2 (or when game is not running), no MCP method is callable!
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {"protocolVersion": "2025-11-25"}
    });
    assert_eq!(
        engine.handle_jsonrpc(&init_req, "sess-1", "tok-1"),
        Err(McpError::ServiceUnavailable)
    );

    // 3. PrepareV2 stages generation (Prepared), still not callable until AdmitV2!
    let owner1 = SdHandle {
        slot: 1,
        generation: 1,
    };
    assert_eq!(
        lifecycle.prepare_v2(&SdHostApiV2::new(owner1)),
        SdResult::Ok
    );
    assert_eq!(
        lifecycle.state_of(owner1),
        Some(ModuleScopeState::Prepared)
    );
    assert_eq!(
        engine.handle_jsonrpc(&init_req, "sess-1", "tok-1"),
        Err(McpError::ServiceUnavailable)
    );

    // Duplicate non-drained PrepareV2 is rejected with Conflict (single owner rule).
    let owner2 = SdHandle {
        slot: 2,
        generation: 2,
    };
    assert_eq!(
        lifecycle.prepare_v2(&SdHostApiV2::new(owner2)),
        SdResult::Conflict
    );

    // 4. AdmitV2 activates the MCP service.
    assert_eq!(lifecycle.admit_v2(owner1), SdResult::Ok);
    assert_eq!(lifecycle.active_owner(), Some(owner1));
    let init_resp = engine
        .handle_jsonrpc(&init_req, "sess-1", "tok-1")
        .unwrap()
        .unwrap();
    assert_eq!(init_resp["result"]["serverInfo"]["name"], "simoder-mcp");

    // 5. QuiesceV2 immediately stops MCP admission; DrainV2 while in-flight returns Busy.
    lifecycle.set_in_flight_for_test(owner1, 1);
    assert_eq!(lifecycle.quiesce_v2(owner1), SdResult::Ok);
    assert_eq!(
        engine.handle_jsonrpc(&init_req, "sess-1", "tok-1"),
        Err(McpError::ServiceUnavailable)
    );
    assert_eq!(lifecycle.drain_v2(owner1), SdResult::Busy);

    lifecycle.set_in_flight_for_test(owner1, 0);
    assert_eq!(lifecycle.drain_v2(owner1), SdResult::Ok);
    assert_eq!(lifecycle.state_of(owner1), Some(ModuleScopeState::Drained));
}

#[test]
fn u4_issue2_and_issue4_catalog_openscp_extensions_decoupled_mod_tools_and_error_boundaries() {
    let (engine, backend, _owner) = create_admitted_engine();

    // 1. Register a decoupled mod tool ("dawid.traffic.analyze_avenues") on the host.
    backend
        .register_mod_tool(
            ExternalModTool {
                owner_mod_id: "dawid.traffic".into(),
                owner_generation: 7,
                definition: ToolDefinition {
                    name: "analyze_avenues".into(),
                    description: "Analyze avenue density from mod telemetry.".into(),
                    input_schema: json!({"type": "object", "properties": {"corridor": {"type": "string"}}, "required": ["corridor"]}),
                    output_schema: None,
                    annotations: ToolAnnotations::read_only(true),
                },
                required_grants: vec!["mod.traffic.read".into()],
            },
            Arc::new(|args| {
                let corridor = args
                    .get("corridor")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("default");
                Ok(json!({"corridor": corridor, "congestion": 0.31}))
            }),
        )
        .unwrap();
    engine.with_auth_mut(|auth| {
        auth.grant_permission("sess-1", "mod.traffic.read");
    });

    // 2. tools/list returns all 41 built-in tools + 1 decoupled mod tool = 42 tools sorted alphabetically.
    let list_resp = engine
        .handle_jsonrpc(
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
            "sess-1",
            "secret-token-1",
        )
        .unwrap()
        .unwrap();
    let tools = list_resp["result"]["tools"].as_array().unwrap();
    assert_eq!(BuiltinToolId::VARIANTS.len(), 41);
    assert_eq!(tools.len(), BuiltinToolId::VARIANTS.len() + 1);
    assert!(tools.iter().any(|t| t["name"] == "simoder_unlock_debug_tools"));
    assert!(tools.iter().any(|t| t["name"] == "simoder_get_bridge_telemetry"));
    assert!(tools.iter().any(|t| t["name"] == "simoder_inspect_sldelta"));
    assert!(tools.iter().any(|t| t["name"] == "simcity_get_build_tools"));
    assert!(tools.iter().any(|t| t["name"] == "simcity_build_road"));
    assert!(tools.iter().any(|t| t["name"] == "simcity_assign_zone"));
    assert!(tools.iter().any(|t| t["name"] == "simcity_place_building"));
    assert!(tools.iter().any(|t| t["name"] == "simcity_inspect_ui_tree"));
    assert!(tools.iter().any(|t| t["name"] == "simcity_query_ui_dom"));
    assert!(tools.iter().any(|t| t["name"] == "simoder_get_ui_state"));
    assert!(tools.iter().any(|t| t["name"] == "dawid.traffic.analyze_avenues"));

    // 3. Execute OpenSCP debug-tools unlock, bridge telemetry, and SLDelta v4 inspector tools.
    let unlock_resp = engine
        .handle_jsonrpc(
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "simoder_unlock_debug_tools", "arguments": {}}
            }),
            "sess-1",
            "secret-token-1",
        )
        .unwrap()
        .unwrap();
    assert_eq!(unlock_resp["result"]["isError"], false);
    assert_eq!(
        unlock_resp["result"]["structuredContent"]["result"]["unlockedTools"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    // Build a valid SLDelta v4 container hex payload with 1 record (26 bytes total).
    let mut sldelta = Vec::new();
    sldelta.extend_from_slice(&4_u32.to_le_bytes());
    sldelta.extend_from_slice(&0xFF02_0300_u32.to_be_bytes());
    sldelta.extend_from_slice(&26_u32.to_be_bytes());
    sldelta.extend_from_slice(&42_u64.to_be_bytes());
    sldelta.extend_from_slice(&500_u32.to_be_bytes());
    sldelta.extend_from_slice(&1_u32.to_be_bytes());
    sldelta.extend_from_slice(&[0xAA, 0xBB]);
    let sldelta_resp = engine
        .handle_jsonrpc(
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "simoder_inspect_sldelta",
                    "arguments": {"payloadHex": hex::encode(&sldelta)}
                }
            }),
            "sess-1",
            "secret-token-1",
        )
        .unwrap()
        .unwrap();
    assert_eq!(sldelta_resp["result"]["isError"], false);
    assert_eq!(
        sldelta_resp["result"]["structuredContent"]["result"]["recordCount"],
        1
    );

    // 4. Execute the decoupled mod tool, then unload mod generation 7 and confirm it disappears!
    let mod_call = engine
        .handle_jsonrpc(
            &json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "dawid.traffic.analyze_avenues",
                    "arguments": {"corridor": "main-street"}
                }
            }),
            "sess-1",
            "secret-token-1",
        )
        .unwrap()
        .unwrap();
    assert_eq!(mod_call["result"]["isError"], false);
    assert_eq!(
        mod_call["result"]["structuredContent"]["result"]["corridor"],
        "main-street"
    );

    assert_eq!(backend.revoke_mod_generation("dawid.traffic", 7), 1);
    let stale_mod_call = engine
        .handle_jsonrpc(
            &json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {
                    "name": "dawid.traffic.analyze_avenues",
                    "arguments": {"corridor": "main-street"}
                }
            }),
            "sess-1",
            "secret-token-1",
        )
        .unwrap()
        .unwrap();
    assert_eq!(stale_mod_call["result"]["isError"], true);

    // 5. Verify world pagination & canonical slot,stamp validation (rejects before touching backend).
    backend.take_recorded_calls();
    for bad_call in [
        json!({"name": "simcity_get_world_units", "arguments": {"regionId": 1, "cityId": 2, "offset": -1}}),
        json!({"name": "simcity_get_world_units", "arguments": {"regionId": 1, "cityId": 2, "limit": 65}}),
        json!({"name": "simcity_get_world_unit", "arguments": {"regionId": 1, "cityId": 2, "unitRef": "01,2"}}),
        json!({"name": "simcity_get_world_unit", "arguments": {"regionId": 1, "cityId": 2, "unitRef": "2147483648,1"}}),
        json!({"name": "simcity_get_world_unit", "arguments": {"regionId": 1, "cityId": 2, "unitRef": "1,4294967296"}}),
    ] {
        let resp = engine
            .handle_jsonrpc(
                &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": bad_call}),
                "sess-1",
                "secret-token-1",
            )
            .unwrap()
            .unwrap();
        assert_eq!(resp["result"]["isError"], true);
    }
    assert!(
        backend.take_recorded_calls().is_empty(),
        "Invalid world arguments must never dispatch to the host"
    );

    // 5b. Verify city-building tools validation and host dispatch.
    for bad_build_call in [
        json!({"name": "simcity_select_tool", "arguments": {"deselect": false}}),
        json!({"name": "simcity_build_road", "arguments": {"startX": 1.2, "startY": 0.5, "endX": 0.6, "endY": 0.5}}),
        json!({"name": "simcity_assign_zone", "arguments": {"zoneType": "invalid", "startX": 0.2, "startY": 0.2, "endX": 0.4, "endY": 0.4}}),
        json!({"name": "simcity_place_building", "arguments": {"toolId": 1001, "normalizedX": -0.1, "normalizedY": 0.5}}),
    ] {
        let resp = engine
            .handle_jsonrpc(
                &json!({"jsonrpc": "2.0", "id": 70, "method": "tools/call", "params": bad_build_call}),
                "sess-1",
                "secret-token-1",
            )
            .unwrap()
            .unwrap();
        assert_eq!(resp["result"]["isError"], true);
    }
    assert!(
        backend.take_recorded_calls().is_empty(),
        "Invalid builder arguments must never dispatch to the host"
    );

    for ok_build_call in [
        json!({"name": "simcity_get_build_tools", "arguments": {"category": "roads"}}),
        json!({"name": "simcity_select_tool", "arguments": {"toolId": 123456, "roadDrawType": "grid"}}),
        json!({"name": "simcity_move_camera", "arguments": {"x": 10.0, "y": 20.0, "z": 30.0, "rotate": 45.0}}),
        json!({"name": "simcity_build_road", "arguments": {"drawType": "straight", "startX": 0.2, "startY": 0.5, "endX": 0.8, "endY": 0.5}}),
        json!({"name": "simcity_assign_zone", "arguments": {"zoneType": "residential", "startX": 0.3, "startY": 0.3, "endX": 0.7, "endY": 0.6}}),
        json!({"name": "simcity_place_building", "arguments": {"toolId": 778899, "normalizedX": 0.5, "normalizedY": 0.5, "rotateSteps": 2}}),
    ] {
        let resp = engine
            .handle_jsonrpc(
                &json!({"jsonrpc": "2.0", "id": 71, "method": "tools/call", "params": ok_build_call}),
                "sess-1",
                "secret-token-1",
            )
            .unwrap()
            .unwrap();
        assert_eq!(resp["result"]["isError"], false);
    }
    assert_eq!(backend.take_recorded_calls().len(), 6);

    // 6. Verify generic internal error masking vs 4096-byte public native error truncation.
    backend.set_mode(BackendSimulationMode::GenericError);
    let generic_err = engine
        .handle_jsonrpc(
            &json!({
                "jsonrpc": "2.0",
                "id": 8,
                "method": "tools/call",
                "params": {"name": "simoder_get_game_modules", "arguments": {}}
            }),
            "sess-1",
            "secret-token-1",
        )
        .unwrap()
        .unwrap();
    assert_eq!(generic_err["result"]["isError"], true);
    let generic_text = generic_err["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        generic_text,
        "An error occurred invoking 'simoder_get_game_modules'."
    );
    assert!(!generic_text.contains("synthetic_sensitive_detail"));

    backend.set_mode(BackendSimulationMode::OversizedNativeError4100);
    let trunc_err = engine
        .handle_jsonrpc(
            &json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "tools/call",
                "params": {"name": "simcity_get_native_session", "arguments": {}}
            }),
            "sess-1",
            "secret-token-1",
        )
        .unwrap()
        .unwrap();
    let trunc_text = trunc_err["result"]["content"][0]["text"].as_str().unwrap();
    let prefix = "An error occurred invoking 'simcity_get_native_session': ";
    assert_eq!(trunc_text.len(), prefix.len() + MAX_MCP_ERROR_MESSAGE_BYTES);

    // 7. Verify 32 MiB frame limit and empty EOF rejection.
    assert!(matches!(
        engine.handle_raw_frame("   ", "sess-1", "secret-token-1"),
        Err(McpError::InvalidJson(_))
    ));
    let oversized_frame = "x".repeat(MAX_MCP_FRAME_BYTES + 1);
    assert_eq!(
        engine.handle_raw_frame(&oversized_frame, "sess-1", "secret-token-1"),
        Err(McpError::FrameTooLarge(MAX_MCP_FRAME_BYTES + 1))
    );
}

#[test]
fn u4_issue3_enforces_consent_required_grants_live_revocation_and_epoch_expiration_without_side_effects() {
    let (engine, backend, _owner) = create_admitted_engine();

    // Provision a client session with only `session.read` (no `session.mutate` or `memory.analysis`).
    engine.with_auth_mut(|auth| {
        auth.register_session(
            "sess-limited",
            "limited-agent",
            "tok-limited",
            true,
            vec![McpGrant::SessionRead.as_str().into()],
        );
    });
    backend.take_recorded_calls();

    // 1. Calling a mutation tool without `session.mutate` is denied with zero host calls!
    let denied_mutate = engine
        .handle_jsonrpc(
            &json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "tools/call",
                "params": {"name": "simcity_close_game", "arguments": {}}
            }),
            "sess-limited",
            "tok-limited",
        )
        .unwrap()
        .unwrap();
    assert_eq!(denied_mutate["result"]["isError"], true);
    assert!(
        denied_mutate["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("missing required permission grant 'session.mutate'")
    );
    assert!(backend.take_recorded_calls().is_empty());

    // 2. Reading session state with `session.read` succeeds.
    let ok_read = engine
        .handle_jsonrpc(
            &json!({
                "jsonrpc": "2.0",
                "id": 11,
                "method": "tools/call",
                "params": {"name": "simcity_get_startup_state", "arguments": {}}
            }),
            "sess-limited",
            "tok-limited",
        )
        .unwrap()
        .unwrap();
    assert_eq!(ok_read["result"]["isError"], false);
    assert_eq!(backend.take_recorded_calls().len(), 1);

    // 3. Revoking `session.read` mid-session immediately blocks subsequent reads without side effects.
    engine.with_auth_mut(|auth| {
        assert!(auth.revoke_permission("sess-limited", McpGrant::SessionRead.as_str()));
    });
    let revoked_read = engine
        .handle_jsonrpc(
            &json!({
                "jsonrpc": "2.0",
                "id": 12,
                "method": "tools/call",
                "params": {"name": "simcity_get_startup_state", "arguments": {}}
            }),
            "sess-limited",
            "tok-limited",
        )
        .unwrap()
        .unwrap();
    assert_eq!(revoked_read["result"]["isError"], true);
    assert!(backend.take_recorded_calls().is_empty());

    // 4. Revoking explicit user consent blocks even basic `tools/list`.
    engine.with_auth_mut(|auth| {
        assert!(auth.set_user_consent("sess-1", false));
    });
    assert!(matches!(
        engine.handle_jsonrpc(
            &json!({"jsonrpc": "2.0", "id": 13, "method": "tools/list", "params": {}}),
            "sess-1",
            "secret-token-1"
        ),
        Err(McpError::AuthorizationDenied(_))
    ));

    // 5. Advancing the runtime epoch invalidates all previously issued tokens (`stale session token epoch`).
    engine.with_auth_mut(|auth| {
        auth.set_user_consent("sess-1", true);
        auth.advance_epoch();
    });
    let stale_call = engine
        .handle_jsonrpc(
            &json!({
                "jsonrpc": "2.0",
                "id": 14,
                "method": "tools/call",
                "params": {"name": "simcity_get_startup_state", "arguments": {}}
            }),
            "sess-1",
            "secret-token-1",
        )
        .unwrap()
        .unwrap();
    assert_eq!(stale_call["result"]["isError"], true);
    assert!(
        stale_call["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("stale session token epoch")
    );
    assert!(backend.take_recorded_calls().is_empty());
}

#[test]
fn u5_issue1_and_issue2_installer_purges_legacy_exes_stages_clean_native_mod_and_rolls_back_on_failure() {
    let temp = tempfile::tempdir().unwrap();
    let mod_root = temp.path().join("mods").join("simoder.devtools");

    // Seed retired legacy .NET executables (`Broker/Simoder.DevBridge.exe`, `MCP/Simoder.McpServer.exe`).
    for &rel in RETIRED_LEGACY_SERVICE_PATHS {
        let path = mod_root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"legacy-dotnet-binary").unwrap();
    }

    let installer = NativeDevToolInstaller::new(&mod_root);
    assert_eq!(
        installer.find_legacy_leftovers().len(),
        RETIRED_LEGACY_SERVICE_PATHS.len()
    );

    // 1. Clean installation of v0.2.0 purges all legacy .exe/.dll services and writes native `simoder_mcp.dll` + `mod.toml`.
    let v020_spec = NativeDevToolPackageSpec {
        version: "0.2.0".into(),
        dll_bytes: b"native-simoder-mcp-dll-v020".to_vec(),
        config_text: "port: 0\nbindAddress: 127.0.0.1\n".into(),
        simulate_commit_failure: false,
    };
    let report1 = installer.install_or_upgrade(&v020_spec).unwrap();
    assert_eq!(report1.active_version, "0.2.0");
    assert_eq!(
        report1.purged_legacy_paths.len(),
        RETIRED_LEGACY_SERVICE_PATHS.len()
    );
    assert!(!report1.rolled_back);
    assert!(installer.find_legacy_leftovers().is_empty());
    assert!(mod_root.join(NATIVE_MCP_DLL_FILENAME).exists());
    let manifest_v020 = fs::read_to_string(mod_root.join("mod.toml")).unwrap();
    assert!(manifest_v020.contains("simoder.native-devtool"));
    assert!(!manifest_v020.contains("Simoder.DevBridge.exe"));
    assert!(!manifest_v020.contains("Simoder.McpServer.exe"));

    // 2. Simulated failure during upgrade to v0.3.0 rolls back atomically to v0.2.0 (`Simoder-DevTools#2`).
    let v030_failed_spec = NativeDevToolPackageSpec {
        version: "0.3.0".into(),
        dll_bytes: b"broken-upgrade-dll-v030".to_vec(),
        config_text: "port: 9999\n".into(),
        simulate_commit_failure: true,
    };
    let report2 = installer.install_or_upgrade(&v030_failed_spec).unwrap();
    assert!(report2.rolled_back);
    assert_eq!(report2.active_version, "0.2.0");
    assert_eq!(report2.active_dll_sha256, report1.active_dll_sha256);
    assert!(!report2.operation_logs.is_empty());
    assert_eq!(
        fs::read(mod_root.join(NATIVE_MCP_DLL_FILENAME)).unwrap(),
        b"native-simoder-mcp-dll-v020"
    );
    assert_eq!(
        fs::read_to_string(mod_root.join("config.toon")).unwrap(),
        "port: 0\nbindAddress: 127.0.0.1\n"
    );
}
