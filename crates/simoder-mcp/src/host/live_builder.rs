#![forbid(unsafe_code)]

//! City-building and live UI inspection helpers for [`LiveHostBackend`].

#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::Duration;

use serde_json::{Value as JsonValue, json};

use super::live::{LiveHostBackend, ROUTER_TIMEOUT_MS};
use super::live_session::eval_router_op;
#[cfg(windows)]
use super::live_session::extract_unit_ref_set;
use crate::types::{McpToolError, RoadDrawType, ZoneType};

impl LiveHostBackend {
    pub(super) fn get_build_tools_live(
        &self,
        category: Option<&str>,
    ) -> Result<JsonValue, McpToolError> {
        let _guard = self.session_mutex.lock();
        let res = eval_router_op(
            self.js_bridge.as_ref(),
            json!({
                "op": "get-build-tools",
                "category": category
            }),
            ROUTER_TIMEOUT_MS,
        )?;
        Ok(json!({ "result": res }))
    }

    pub(super) fn select_tool_live(
        &self,
        tool_id: Option<u32>,
        deselect: bool,
        road_draw_type: Option<RoadDrawType>,
    ) -> Result<JsonValue, McpToolError> {
        let _guard = self.session_mutex.lock();
        let res = eval_router_op(
            self.js_bridge.as_ref(),
            json!({
                "op": "select-tool",
                "toolId": tool_id,
                "deselect": deselect,
                "roadDrawType": road_draw_type.map(|m| m as u32)
            }),
            ROUTER_TIMEOUT_MS,
        )?;
        Ok(json!({ "result": res }))
    }

    pub(super) fn move_camera_live(
        &self,
        x: f64,
        y: f64,
        z: f64,
        rotate: Option<f64>,
    ) -> Result<JsonValue, McpToolError> {
        let _guard = self.session_mutex.lock();
        let res = eval_router_op(
            self.js_bridge.as_ref(),
            json!({
                "op": "move-camera",
                "x": x,
                "y": y,
                "z": z,
                "rotate": rotate
            }),
            ROUTER_TIMEOUT_MS,
        )?;
        Ok(json!({ "result": res }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_road_live(
        &self,
        tool_id: Option<u32>,
        draw_type: RoadDrawType,
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        steps: u32,
    ) -> Result<JsonValue, McpToolError> {
        #[cfg(windows)]
        {
            let _guard = self.session_mutex.lock();
            let before_snap = eval_router_op(
                self.js_bridge.as_ref(),
                json!({ "op": "unit-snapshot" }),
                ROUTER_TIMEOUT_MS,
            )?;
            let before_refs = extract_unit_ref_set(&before_snap);

            let select_res = eval_router_op(
                self.js_bridge.as_ref(),
                json!({
                    "op": "select-tool",
                    "toolId": tool_id,
                    "pickDefaultRoad": tool_id.is_none(),
                    "roadDrawType": draw_type as u32
                }),
                ROUTER_TIMEOUT_MS,
            )?;

            let win = super::win32_live::select_primary_window(self.game_pid)
                .map_err(McpToolError::public)?;
            let ((sx, sy), (ex, ey)) = super::win32_live::send_client_drag(
                self.game_pid,
                win.handle,
                start_x,
                start_y,
                end_x,
                end_y,
                steps,
            )
            .map_err(McpToolError::public)?;

            thread::sleep(Duration::from_millis(180));
            let after_snap = eval_router_op(
                self.js_bridge.as_ref(),
                json!({ "op": "unit-snapshot" }),
                ROUTER_TIMEOUT_MS,
            )?;
            let after_refs = extract_unit_ref_set(&after_snap);
            let added: Vec<String> = after_refs.difference(&before_refs).cloned().collect();
            let draw_str: &'static str = draw_type.into();

            Ok(json!({
                "result": {
                    "completed": true,
                    "drawType": draw_str,
                    "tool": select_res,
                    "dragPixels": { "start": [sx, sy], "end": [ex, ey] },
                    "unitTotalBefore": before_refs.len(),
                    "unitTotalAfter": after_refs.len(),
                    "newUnits": added
                }
            }))
        }
        #[cfg(not(windows))]
        {
            let _ = (tool_id, draw_type, start_x, start_y, end_x, end_y, steps);
            Err(McpToolError::public(
                "building roads requires a Windows game window",
            ))
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn assign_zone_live(
        &self,
        zone_type: ZoneType,
        tool_id: Option<u32>,
        start_x: f64,
        start_y: f64,
        end_x: f64,
        end_y: f64,
        steps: u32,
    ) -> Result<JsonValue, McpToolError> {
        #[cfg(windows)]
        {
            let _guard = self.session_mutex.lock();
            let before_lots = eval_router_op(
                self.js_bridge.as_ref(),
                json!({ "op": "zone-lots" }),
                ROUTER_TIMEOUT_MS,
            )?;
            let zone_str: &'static str = zone_type.into();
            let select_res = eval_router_op(
                self.js_bridge.as_ref(),
                json!({
                    "op": "select-tool",
                    "zoneType": zone_str,
                    "toolId": tool_id
                }),
                ROUTER_TIMEOUT_MS,
            )?;

            let win = super::win32_live::select_primary_window(self.game_pid)
                .map_err(McpToolError::public)?;
            let ((sx, sy), (ex, ey)) = super::win32_live::send_client_drag(
                self.game_pid,
                win.handle,
                start_x,
                start_y,
                end_x,
                end_y,
                steps,
            )
            .map_err(McpToolError::public)?;

            thread::sleep(Duration::from_millis(180));
            let after_lots = eval_router_op(
                self.js_bridge.as_ref(),
                json!({ "op": "zone-lots" }),
                ROUTER_TIMEOUT_MS,
            )?;

            Ok(json!({
                "result": {
                    "completed": true,
                    "zoneType": zone_str,
                    "tool": select_res,
                    "dragPixels": { "start": [sx, sy], "end": [ex, ey] },
                    "zoneLotsBefore": before_lots,
                    "zoneLotsAfter": after_lots
                }
            }))
        }
        #[cfg(not(windows))]
        {
            let _ = (zone_type, tool_id, start_x, start_y, end_x, end_y, steps);
            Err(McpToolError::public(
                "assigning zones requires a Windows game window",
            ))
        }
    }

    pub(super) fn place_building_live(
        &self,
        tool_id: u32,
        normalized_x: f64,
        normalized_y: f64,
        rotate_steps: i32,
    ) -> Result<JsonValue, McpToolError> {
        #[cfg(windows)]
        {
            let _guard = self.session_mutex.lock();
            let before_snap = eval_router_op(
                self.js_bridge.as_ref(),
                json!({ "op": "unit-snapshot" }),
                ROUTER_TIMEOUT_MS,
            )?;
            let before_refs = extract_unit_ref_set(&before_snap);

            let select_res = eval_router_op(
                self.js_bridge.as_ref(),
                json!({
                    "op": "select-tool",
                    "toolId": tool_id
                }),
                ROUTER_TIMEOUT_MS,
            )?;

            let win = super::win32_live::select_primary_window(self.game_pid)
                .map_err(McpToolError::public)?;

            let rot_vk = if rotate_steps >= 0 {
                super::win32_live::VK_OEM_PERIOD
            } else {
                super::win32_live::VK_OEM_COMMA
            };
            for _ in 0..rotate_steps.abs().clamp(0, 4) {
                super::win32_live::send_window_key(self.game_pid, win.handle, rot_vk)
                    .map_err(McpToolError::public)?;
                thread::sleep(Duration::from_millis(35));
            }

            let (cx, cy) = super::win32_live::send_client_click(
                self.game_pid,
                win.handle,
                normalized_x,
                normalized_y,
            )
            .map_err(McpToolError::public)?;

            thread::sleep(Duration::from_millis(180));
            let after_snap = eval_router_op(
                self.js_bridge.as_ref(),
                json!({ "op": "unit-snapshot" }),
                ROUTER_TIMEOUT_MS,
            )?;
            let after_refs = extract_unit_ref_set(&after_snap);
            let added: Vec<String> = after_refs.difference(&before_refs).cloned().collect();

            Ok(json!({
                "result": {
                    "completed": true,
                    "toolId": tool_id,
                    "tool": select_res,
                    "rotateSteps": rotate_steps,
                    "clientClick": [cx, cy],
                    "unitTotalBefore": before_refs.len(),
                    "unitTotalAfter": after_refs.len(),
                    "newUnits": added
                }
            }))
        }
        #[cfg(not(windows))]
        {
            let _ = (tool_id, normalized_x, normalized_y, rotate_steps);
            Err(McpToolError::public(
                "placing buildings requires a Windows game window",
            ))
        }
    }

    pub(super) fn inspect_ui_tree_live(
        &self,
        control_id: Option<u32>,
        max_depth: u32,
        visible_only: bool,
        max_nodes: u32,
    ) -> Result<JsonValue, McpToolError> {
        let _guard = self.session_mutex.lock();
        let res = eval_router_op(
            self.js_bridge.as_ref(),
            json!({
                "op": "inspect-ui-tree",
                "controlId": control_id,
                "maxDepth": max_depth,
                "visibleOnly": visible_only,
                "maxNodes": max_nodes
            }),
            ROUTER_TIMEOUT_MS,
        )?;
        Ok(json!({ "result": res }))
    }

    pub(super) fn query_ui_dom_live(
        &self,
        selector: &str,
        limit: u32,
        include_computed_style: bool,
    ) -> Result<JsonValue, McpToolError> {
        let _guard = self.session_mutex.lock();
        let res = eval_router_op(
            self.js_bridge.as_ref(),
            json!({
                "op": "query-ui-dom",
                "selector": selector,
                "limit": limit,
                "includeComputedStyle": include_computed_style
            }),
            ROUTER_TIMEOUT_MS,
        )?;
        Ok(json!({ "result": res }))
    }

    pub(super) fn get_ui_state_live(&self) -> Result<JsonValue, McpToolError> {
        let _guard = self.session_mutex.lock();
        let res = eval_router_op(
            self.js_bridge.as_ref(),
            json!({ "op": "simoder-ui-state" }),
            ROUTER_TIMEOUT_MS,
        )?;
        Ok(json!({ "result": res }))
    }
}
