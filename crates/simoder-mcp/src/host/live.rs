#![forbid(unsafe_code)]

//! Live in-process host backend ([`LiveHostBackend`]) backed by actual Simoder files (`logs/`, `config.toon`,
//! `state.toon`, `native_trust.toon`, `mods/`), Win32 module enumeration, checked process memory reads,
//! real window PNG captures, Win32 viewport input, and `EAWebKit` semantic JS execution via ABI v2.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};

use super::live_session::{
    eval_router_op, execute_native_session, map_native_phase_to_stage,
};
use super::{ExternalModTool, HostDispatchProvider, HostLogFn, StandardHostBackend, UiJsEvalFn};
use crate::config::{DEFAULT_LOG_TAIL_LINES, MAX_LOG_TAIL_LINES, MAX_MEMORY_READ_BYTES};
use crate::types::{
    AllowedKey, LaunchMode, McpToolError, NativeSessionRequest, RoadDrawType, StartupStage,
    ZoneType,
};

const CAPTURE_FRESHNESS_LIMIT: Duration = Duration::from_secs(10);
pub(super) const ROUTER_TIMEOUT_MS: u32 = 8_000;

struct CachedCapture {
    capture_id: String,
    hwnd: usize,
    captured_at: Instant,
    metadata: JsonValue,
    png_base64: String,
}

/// Live [`HostDispatchProvider`] serving real host logs, mod runtime files, process modules,
/// process memory inspection, Win32 window captures/input, and live `EAWebKit` semantic JS actions.
pub struct LiveHostBackend {
    pub(super) game_pid: u32,
    game_root: Option<PathBuf>,
    pub(super) js_bridge: Option<UiJsEvalFn>,
    host_log: Option<HostLogFn>,
    fallback: StandardHostBackend,
    pub(super) session_mutex: Mutex<()>,
    capture_seq: AtomicU64,
    last_capture: Mutex<Option<CachedCapture>>,
}

impl LiveHostBackend {
    /// Construct a [`LiveHostBackend`] bound to `game_pid` and optional `<game_root>` without a JS bridge.
    pub fn new(game_pid: u32, game_root: Option<PathBuf>) -> Self {
        Self::with_js_bridge(game_pid, game_root, None)
    }

    /// Construct a [`LiveHostBackend`] bound to `game_pid`, optional `<game_root>`, and ABI v2 `js_bridge`.
    pub fn with_js_bridge(
        game_pid: u32,
        game_root: Option<PathBuf>,
        js_bridge: Option<UiJsEvalFn>,
    ) -> Self {
        Self {
            game_pid,
            game_root,
            js_bridge,
            host_log: None,
            fallback: StandardHostBackend::new(game_pid),
            session_mutex: Mutex::new(()),
            capture_seq: AtomicU64::new(1),
            last_capture: Mutex::new(None),
        }
    }

    /// Attach an optional [`HostLogFn`] forwarding MCP events into `Simoder`'s console pipeline.
    pub fn with_host_log(mut self, host_log: Option<HostLogFn>) -> Self {
        self.host_log = host_log;
        self
    }

    /// Borrow the underlying [`StandardHostBackend`] (for decoupled mod tool registration).
    pub const fn fallback(&self) -> &StandardHostBackend {
        &self.fallback
    }

    fn simoder_dir(&self) -> Option<PathBuf> {
        self.game_root.as_ref().map(|root| root.join("simoder"))
    }

    fn capture_live_window_impl(&self, target: &str) -> Result<(JsonValue, String), McpToolError> {
        #[cfg(windows)]
        {
            let (win, png_base64) =
                super::win32_live::capture_process_window_for_target(self.game_pid, target)
                    .map_err(McpToolError::public)?;
            let seq = self.capture_seq.fetch_add(1, Ordering::Relaxed);
            let capture_id = format!("live-cap-{}-{seq}", self.game_pid);
            let captured_at_utc = format_utc_now();
            let metadata = json!({
                "captureId": capture_id,
                "capturedAtUtc": captured_at_utc,
                "window": {
                    "handle": win.handle,
                    "processId": win.process_id,
                    "processName": "SimCity.exe",
                    "title": win.title,
                    "left": win.left,
                    "top": win.top,
                    "width": win.width,
                    "height": win.height,
                    "clientLeft": win.client_left,
                    "clientTop": win.client_top,
                    "clientWidth": win.client_width,
                    "clientHeight": win.client_height,
                    "isForeground": win.is_foreground,
                    "isAllowed": true
                },
                "accessibleControls": []
            });
            *self.last_capture.lock() = Some(CachedCapture {
                capture_id,
                hwnd: win.handle,
                captured_at: Instant::now(),
                metadata: metadata.clone(),
                png_base64: png_base64.clone(),
            });
            Ok((metadata, png_base64))
        }
        #[cfg(not(windows))]
        {
            let _ = &self.capture_seq;
            Err(McpToolError::public(
                "window capture is only supported on Windows hosts",
            ))
        }
    }
}

impl HostDispatchProvider for LiveHostBackend {
    fn native_session(&self, request: NativeSessionRequest) -> Result<JsonValue, McpToolError> {
        let _guard = self.session_mutex.lock();
        execute_native_session(
            self.js_bridge.as_ref(),
            self.game_pid,
            request,
            &format_utc_now,
        )
    }

    fn read_game_memory(&self, address: &str, length: i64) -> Result<JsonValue, McpToolError> {
        if length <= 0 || length > MAX_MEMORY_READ_BYTES {
            return Err(McpToolError::public(format!(
                "length {length} out of bounds 1..={MAX_MEMORY_READ_BYTES}"
            )));
        }
        let parsed = parse_hex_address(address)?;
        #[cfg(windows)]
        {
            let bytes = super::win32_live::read_process_memory_checked(parsed, length as usize)
                .map_err(McpToolError::public)?;
            let digest = hex::encode(Sha256::digest(&bytes));
            Ok(json!({
                "processId": self.game_pid,
                "address": format!("0x{parsed:08X}"),
                "length": bytes.len(),
                "hex": hex::encode(&bytes),
                "sha256": digest,
                "readAtUtc": format_utc_now()
            }))
        }
        #[cfg(not(windows))]
        {
            let _ = parsed;
            Err(McpToolError::public(
                "live memory inspection is only supported on Windows",
            ))
        }
    }

    fn get_game_modules(&self) -> Result<JsonValue, McpToolError> {
        #[cfg(windows)]
        {
            let modules = super::win32_live::enumerate_process_modules(self.game_pid)
                .map_err(McpToolError::public)?;
            let projected: Vec<JsonValue> = modules
                .into_iter()
                .map(|m| {
                    json!({
                        "name": m.name,
                        "path": m.path,
                        "baseAddress": format!("0x{:08X}", m.base_address),
                        "imageSize": m.image_size
                    })
                })
                .collect();
            Ok(json!({
                "result": {
                    "processId": self.game_pid,
                    "observedAtUtc": format_utc_now(),
                    "modules": projected
                }
            }))
        }
        #[cfg(not(windows))]
        {
            Err(McpToolError::public(
                "module enumeration is only supported on Windows",
            ))
        }
    }

    fn export_game_code(&self) -> Result<JsonValue, McpToolError> {
        self.fallback.export_game_code()
    }

    fn log_event(&self, level: &str, source: &str, message: &str) {
        if let Some(cb) = &self.host_log {
            cb(level, source, message);
        }
    }

    fn set_mod_enabled(&self, mod_id: &str, enabled: bool) -> Result<JsonValue, McpToolError> {
        self.fallback.set_mod_enabled(mod_id, enabled)
    }

    fn close_game(&self) -> Result<JsonValue, McpToolError> {
        #[cfg(windows)]
        {
            let target = super::win32_live::select_primary_window(self.game_pid).ok();
            let requested = target
                .as_ref()
                .is_some_and(|w| super::win32_live::post_close_window(w.handle));
            Ok(json!({
                "result": {
                    "requested": requested,
                    "status": if requested { "close_queued" } else { "no_window_found" },
                    "processId": self.game_pid,
                    "note": "Normal exit requested via WM_CLOSE without force termination."
                }
            }))
        }
        #[cfg(not(windows))]
        {
            self.fallback.close_game()
        }
    }

    fn start_session(
        &self,
        mode: LaunchMode,
        region_name: Option<&str>,
        city_name: Option<&str>,
    ) -> Result<JsonValue, McpToolError> {
        let mut state = self.get_startup_state()?;
        let mode_str: &'static str = mode.into();
        state["target"]["mode"] = json!(mode_str.to_ascii_lowercase());
        if let Some(region) = region_name {
            state["target"]["regionName"] = json!(region);
        }
        if let Some(city) = city_name {
            state["target"]["cityName"] = json!(city);
        }
        state["alreadyRunning"] = json!(true);
        Ok(state)
    }

    fn get_startup_state(&self) -> Result<JsonValue, McpToolError> {
        #[cfg(windows)]
        let windows = super::win32_live::list_process_windows(self.game_pid);
        #[cfg(not(windows))]
        let windows: Vec<()> = Vec::new();

        #[cfg(windows)]
        let projected_windows: Vec<JsonValue> = windows
            .iter()
            .map(|w| {
                json!({
                    "handle": w.handle,
                    "processId": w.process_id,
                    "title": w.title,
                    "width": w.width,
                    "height": w.height,
                    "isForeground": w.is_foreground
                })
            })
            .collect();
        #[cfg(not(windows))]
        let projected_windows: Vec<JsonValue> = Vec::new();

        let native_obs = self
            .js_bridge
            .as_ref()
            .and_then(|_| eval_router_op(self.js_bridge.as_ref(), json!({"op": "observe"}), 2_000).ok());

        let stage_enum = native_obs
            .as_ref()
            .and_then(map_native_phase_to_stage)
            .unwrap_or(if projected_windows.is_empty() {
                StartupStage::GameStarting
            } else {
                StartupStage::MainMenu
            });
        let stage_val = serde_json::to_value(stage_enum).unwrap_or_else(|_| json!("mainMenu"));

        Ok(json!({
            "sessionId": format!("live-pid-{}", self.game_pid),
            "target": {
                "mode": "observe",
                "gamePid": self.game_pid,
                "gameRoot": self.game_root.as_ref().map(|p| p.display().to_string()),
            },
            "stage": stage_val,
            "source": "nativeObservation",
            "confidence": 1,
            "humanAction": null,
            "nativeObservation": native_obs,
            "updatedAtUtc": format_utc_now(),
            "processes": [{ "pid": self.game_pid, "name": "SimCity.exe" }],
            "windows": projected_windows,
            "timeline": []
        }))
    }

    fn capture_window(&self, target: &str) -> Result<(JsonValue, String), McpToolError> {
        self.capture_live_window_impl(target)
    }

    fn click(
        &self,
        capture_id: &str,
        normalized_x: f64,
        normalized_y: f64,
    ) -> Result<JsonValue, McpToolError> {
        if !normalized_x.is_finite()
            || !normalized_y.is_finite()
            || !(0.0..=1.0).contains(&normalized_x)
            || !(0.0..=1.0).contains(&normalized_y)
        {
            return Err(McpToolError::public(
                "normalizedX and normalizedY must be finite values within 0.0..=1.0",
            ));
        }

        let hwnd = {
            let guard = self.last_capture.lock();
            let Some(cap) = guard.as_ref() else {
                return Err(McpToolError::public(
                    "capture_id unknown: call simcity_capture_window before clicking",
                ));
            };
            if cap.capture_id != capture_id {
                return Err(McpToolError::public(format!(
                    "stale or mismatched captureId '{capture_id}' (latest is '{}')",
                    cap.capture_id
                )));
            }
            if cap.captured_at.elapsed() > CAPTURE_FRESHNESS_LIMIT {
                return Err(McpToolError::public(format!(
                    "captureId '{capture_id}' expired (>10s old); capture again before clicking"
                )));
            }
            cap.hwnd
        };

        #[cfg(windows)]
        {
            let (cx, cy) =
                super::win32_live::send_client_click(self.game_pid, hwnd, normalized_x, normalized_y)
                    .map_err(McpToolError::public)?;
            Ok(json!({
                "result": {
                    "accepted": true,
                    "captureId": capture_id,
                    "clientX": cx,
                    "clientY": cy,
                    "mechanism": "win32-postmessage"
                }
            }))
        }
        #[cfg(not(windows))]
        {
            let _ = hwnd;
            Err(McpToolError::public(
                "window click injection is only supported on Windows hosts",
            ))
        }
    }

    fn send_key(&self, _target: &str, key: AllowedKey) -> Result<JsonValue, McpToolError> {
        #[cfg(windows)]
        {
            let vk = match key {
                AllowedKey::Escape => super::win32_live::VK_ESCAPE,
                AllowedKey::Enter => super::win32_live::VK_RETURN,
                AllowedKey::Space => super::win32_live::VK_SPACE,
            };
            let win = super::win32_live::select_primary_window(self.game_pid)
                .map_err(McpToolError::public)?;
            super::win32_live::send_window_key(self.game_pid, win.handle, vk)
                .map_err(McpToolError::public)?;
            let key_str: &'static str = key.into();
            Ok(json!({
                "result": {
                    "accepted": true,
                    "key": key_str.to_ascii_lowercase(),
                    "handle": win.handle
                }
            }))
        }
        #[cfg(not(windows))]
        {
            let _ = key;
            Err(McpToolError::public(
                "window key injection is only supported on Windows hosts",
            ))
        }
    }

    fn confirm_stage(
        &self,
        capture_id: &str,
        stage: StartupStage,
        confidence: f64,
        note: Option<&str>,
    ) -> Result<JsonValue, McpToolError> {
        let mut state = self.get_startup_state()?;
        state["stage"] = serde_json::to_value(stage).unwrap_or_else(|_| json!("mainMenu"));
        state["source"] = json!("aiConfirmed");
        state["confidence"] = json!(confidence);
        state["confirmedCaptureId"] = json!(capture_id);
        state["note"] = json!(note);
        Ok(state)
    }

    fn wait_for_stage(
        &self,
        stage: StartupStage,
        timeout_seconds: i64,
    ) -> Result<JsonValue, McpToolError> {
        let deadline = Instant::now() + Duration::from_secs(timeout_seconds.clamp(1, 300) as u64);
        let expected_json = serde_json::to_value(stage).unwrap_or_else(|_| json!("mainMenu"));
        loop {
            let state = self.get_startup_state()?;
            if state["stage"] == expected_json || Instant::now() >= deadline {
                return Ok(state);
            }
            thread::sleep(Duration::from_millis(250));
        }
    }

    fn get_diagnostics(&self) -> Result<(JsonValue, Option<String>), McpToolError> {
        let state = self.get_startup_state()?;
        let logs = self.get_logs(None, 40)?;
        let recent_lines = logs
            .get("result")
            .and_then(|r| r.get("lines"))
            .cloned()
            .unwrap_or_else(|| json!([]));

        let module_names: Vec<String> = self
            .get_game_modules()
            .ok()
            .and_then(|v| {
                v.get("result")?
                    .get("modules")?
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|m| m.get("name").and_then(JsonValue::as_str).map(String::from))
                            .collect()
                    })
            })
            .unwrap_or_default();

        let cached = self.last_capture.lock();
        let (last_cap_meta, png_opt) = if let Some(cap) = cached.as_ref() {
            (cap.metadata.clone(), Some(cap.png_base64.clone()))
        } else {
            drop(cached);
            match self.capture_live_window_impl("auto") {
                Ok((meta, b64)) => (meta, Some(b64)),
                Err(_) => (JsonValue::Null, None),
            }
        };

        Ok((
            json!({
                "state": state,
                "overlayModules": module_names,
                "recentLogLines": recent_lines,
                "lastCapture": last_cap_meta,
                "generatedAtUtc": format_utc_now()
            }),
            png_opt,
        ))
    }

    fn get_runtime_state(&self) -> Result<JsonValue, McpToolError> {
        let now = format_utc_now();
        let Some(root) = self.game_root.as_ref() else {
            return Ok(json!({
                "result": {
                    "available": false,
                    "fresh": false,
                    "message": "gameRoot unavailable for live inspection",
                    "snapshot": null,
                    "readAtUtc": now
                }
            }));
        };
        let simoder_dir = root.join("simoder");
        let config_raw = fs::read_to_string(simoder_dir.join("config.toon")).ok();
        let state_raw = fs::read_to_string(simoder_dir.join("state.toon")).ok();
        let region_mods_raw = fs::read_to_string(simoder_dir.join("region_mods.toon")).ok();
        let trust_raw = fs::read_to_string(simoder_dir.join("native_trust.toon")).ok();
        let mods_dir = root.join("mods");
        let mods = scan_mod_summaries(&mods_dir);
        #[cfg(windows)]
        let dev_console = Some(super::win32_live::inspect_developer_console_mode());
        #[cfg(not(windows))]
        let dev_console: Option<JsonValue> = None;

        Ok(json!({
            "result": {
                "available": true,
                "fresh": true,
                "message": "live filesystem observation",
                "readAtUtc": now,
                "snapshot": {
                    "gamePid": self.game_pid,
                    "gameRoot": root.display().to_string(),
                    "modsDirectory": mods_dir.display().to_string(),
                    "discoveredMods": mods,
                    "configToon": config_raw,
                    "stateToon": state_raw,
                    "regionModsToon": region_mods_raw,
                    "nativeTrustToon": trust_raw,
                    "developerConsole": dev_console,
                }
            }
        }))
    }

    fn get_logs(&self, mod_id: Option<&str>, limit: i64) -> Result<JsonValue, McpToolError> {
        let bounded = if limit <= 0 {
            DEFAULT_LOG_TAIL_LINES as usize
        } else {
            limit.min(MAX_LOG_TAIL_LINES) as usize
        };

        let mut lines = Vec::new();
        if let Some(simoder_dir) = self.simoder_dir() {
            let log_path = simoder_dir.join("logs").join("sc13modloader.log");
            if let Ok(file) = File::open(&log_path) {
                let reader = BufReader::new(file);
                let tag = mod_id.map(|id| format!("[{id}]"));
                let matched: Vec<String> = reader
                    .lines()
                    .map_while(Result::ok)
                    .filter(|line| match (&tag, mod_id) {
                        (Some(t), Some(raw)) => line.contains(t) || line.contains(raw),
                        _ => true,
                    })
                    .collect();
                let start = matched.len().saturating_sub(bounded);
                lines.extend_from_slice(&matched[start..]);
            }
        }

        Ok(json!({
            "result": {
                "lines": lines,
                "modId": mod_id,
                "readAtUtc": format_utc_now()
            }
        }))
    }

    fn get_capabilities(&self) -> JsonValue {
        let mut caps = self.fallback.get_capabilities();
        caps["result"]["eawebkitSemanticBridgeReady"] = json!(self.js_bridge.is_some());
        caps
    }

    fn get_service_info(&self) -> JsonValue {
        self.fallback.get_service_info()
    }

    fn unlock_debug_tools(
        &self,
        category: Option<u32>,
        ui_category: Option<u32>,
    ) -> Result<JsonValue, McpToolError> {
        self.fallback.unlock_debug_tools(category, ui_category)
    }

    fn get_bridge_telemetry(&self, channel: Option<&str>) -> Result<JsonValue, McpToolError> {
        self.fallback.get_bridge_telemetry(channel)
    }

    fn inspect_sldelta(&self, payload_hex: &str) -> Result<JsonValue, McpToolError> {
        self.fallback.inspect_sldelta(payload_hex)
    }

    fn eval_ui_bridge(&self, method: &str, verb: &str) -> Result<JsonValue, McpToolError> {
        let _guard = self.session_mutex.lock();
        let res = eval_router_op(
            self.js_bridge.as_ref(),
            json!({
                "op": "eval-ui-bridge",
                "method": method,
                "verb": verb
            }),
            ROUTER_TIMEOUT_MS,
        )?;
        Ok(json!({ "result": res }))
    }

    fn get_build_tools(&self, category: Option<&str>) -> Result<JsonValue, McpToolError> {
        self.get_build_tools_live(category)
    }

    fn select_tool(
        &self,
        tool_id: Option<u32>,
        deselect: bool,
        road_draw_type: Option<RoadDrawType>,
    ) -> Result<JsonValue, McpToolError> {
        self.select_tool_live(tool_id, deselect, road_draw_type)
    }

    fn move_camera(
        &self,
        x: f64,
        y: f64,
        z: f64,
        rotate: Option<f64>,
    ) -> Result<JsonValue, McpToolError> {
        self.move_camera_live(x, y, z, rotate)
    }

    #[allow(clippy::too_many_arguments)]
    fn build_road(
        &self,
        tool_id: Option<u32>,
        draw_type: RoadDrawType,
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        steps: u32,
    ) -> Result<JsonValue, McpToolError> {
        self.build_road_live(tool_id, draw_type, start_x, start_y, end_x, end_y, steps)
    }

    #[allow(clippy::too_many_arguments)]
    fn assign_zone(
        &self,
        zone_type: ZoneType,
        tool_id: Option<u32>,
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        steps: u32,
    ) -> Result<JsonValue, McpToolError> {
        self.assign_zone_live(zone_type, tool_id, start_x, start_y, end_x, end_y, steps)
    }

    fn place_building(
        &self,
        tool_id: u32,
        normalized_x: f64,
        normalized_y: f64,
        rotate_steps: i32,
    ) -> Result<JsonValue, McpToolError> {
        self.place_building_live(tool_id, normalized_x, normalized_y, rotate_steps)
    }

    fn list_external_mod_tools(&self) -> Vec<ExternalModTool> {
        self.fallback.list_external_mod_tools()
    }

    fn inspect_ui_tree(
        &self,
        control_id: Option<u32>,
        max_depth: u32,
        visible_only: bool,
        max_nodes: u32,
    ) -> Result<JsonValue, McpToolError> {
        self.inspect_ui_tree_live(control_id, max_depth, visible_only, max_nodes)
    }

    fn query_ui_dom(
        &self,
        selector: &str,
        limit: u32,
        include_computed_style: bool,
    ) -> Result<JsonValue, McpToolError> {
        self.query_ui_dom_live(selector, limit, include_computed_style)
    }

    fn get_ui_state(&self) -> Result<JsonValue, McpToolError> {
        self.get_ui_state_live()
    }

    fn invoke_external_mod_tool(
        &self,
        tool_name: &str,
        arguments: &JsonValue,
    ) -> Result<JsonValue, McpToolError> {
        self.fallback.invoke_external_mod_tool(tool_name, arguments)
    }
}

fn parse_hex_address(raw: &str) -> Result<usize, McpToolError> {
    let trimmed = raw.trim();
    let hex_digits = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    usize::from_str_radix(hex_digits, 16)
        .map_err(|e| McpToolError::public(format!("invalid hexadecimal address '{raw}': {e}")))
}

fn scan_mod_summaries(mods_dir: &Path) -> Vec<JsonValue> {
    let Ok(entries) = fs::read_dir(mods_dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();
    let mut out = Vec::new();
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let dir_name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        if dir_name.eq_ignore_ascii_case("cache") {
            continue;
        }
        let manifest_path = dir.join("mod.toml");
        let manifest_present = manifest_path.is_file();
        let (mod_id, version, has_scripts, has_native) = if let Ok(raw) = fs::read_to_string(&manifest_path) {
            let mut id = None;
            let mut ver = None;
            let mut section = "";
            let mut scripts = false;
            let mut native = false;
            for line in raw.lines() {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') {
                    continue;
                }
                if t.starts_with('[') && t.ends_with(']') {
                    section = t;
                    if t == "[scripts]" {
                        scripts = true;
                    } else if t == "[native]" {
                        native = true;
                    }
                    continue;
                }
                if section == "[metadata]"
                    && let Some((k, v)) = t.split_once('=')
                {
                    let val = v.trim().trim_matches('"').trim_matches('\'');
                    match k.trim() {
                        "id" => id = Some(val.to_string()),
                        "version" => ver = Some(val.to_string()),
                        _ => {}
                    }
                }
            }
            (id.unwrap_or_else(|| dir_name.clone()), ver, scripts, native)
        } else {
            (dir_name.clone(), None, false, false)
        };
        out.push(json!({
            "directory": dir_name,
            "id": mod_id,
            "version": version,
            "hasManifest": manifest_present,
            "hasScripts": has_scripts,
            "hasNative": has_native,
        }));
    }
    out
}

/// Format the current UTC wall clock as an ISO-8601 timestamp (`YYYY-MM-DDTHH:MM:SS+00:00`) using Howard Hinnant's civil calendar conversion.
pub fn format_utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_i64, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}+00:00")
}
