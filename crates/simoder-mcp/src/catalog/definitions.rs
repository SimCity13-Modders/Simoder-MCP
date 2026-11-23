#![forbid(unsafe_code)]

//! JSON Schema and metadata definitions for all 38 built-in tools in [`BuiltinToolId`].

use serde_json::json;

use super::BuiltinToolId;
use crate::types::{ToolAnnotations, ToolDefinition};

impl BuiltinToolId {
    /// Build the [`ToolDefinition`] advertised in `tools/list`.
    pub fn definition(self) -> ToolDefinition {
        let (description, input_schema, annotations) = match self {
            Self::SimCityAssignZone => (
                "Select an RCI zoning tool (residential, commercial, industrial, or unzone) and drag across normalized viewport coordinates in an active city, returning toolZoneLots delta verification.",
                json!({
                    "type": "object",
                    "properties": {
                        "zoneType": {"type": "string", "enum": ["residential", "commercial", "industrial", "unzone"]},
                        "toolId": {"type": ["integer", "null"], "default": null},
                        "startX": {"type": "number"},
                        "startY": {"type": "number"},
                        "endX": {"type": "number"},
                        "endY": {"type": "number"},
                        "steps": {"type": "integer", "default": 10}
                    },
                    "required": ["zoneType", "startX", "startY", "endX", "endY"]
                }),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimCityBuildRoad => (
                "Select a road tool (optional toolId) and RoadDrawType (straight, curvy, grid, arc, circle), then drag between normalized viewport coordinates to build a road segment in an active city.",
                json!({
                    "type": "object",
                    "properties": {
                        "toolId": {"type": ["integer", "null"], "default": null},
                        "drawType": {"type": "string", "enum": ["curvy", "straight", "grid", "arc", "circle"], "default": "straight"},
                        "startX": {"type": "number"},
                        "startY": {"type": "number"},
                        "endX": {"type": "number"},
                        "endY": {"type": "number"},
                        "steps": {"type": "integer", "default": 12}
                    },
                    "required": ["startX", "startY", "endX", "endY"]
                }),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimCityCaptureWindow => (
                "Capture an allowlisted EA or SimCity window. Returns a native PNG image block plus capture_id, geometry, and accessible controls; use the image and capture_id before clicking.",
                json!({"type": "object", "properties": {"target": {"type": "string", "default": "auto"}}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimCityClaimCity => (
                "Claim an exact unclaimed city/box ID in a Sandbox region via native CreateBox, with an explicit city name. Refuses non-Sandbox regions, foreign box IDs and already claimed cities.",
                json!({"type": "object", "properties": {"regionId": {"type": "integer"}, "cityId": {"type": "integer"}, "cityName": {"type": "string"}}, "required": ["regionId", "cityId", "cityName"]}),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimCityClick => (
                "Click a normalized point in an allowlisted window using an exact capture_id no older than ten seconds. Rejects stale or changed windows.",
                json!({"type": "object", "properties": {"captureId": {"type": "string"}, "normalizedX": {"type": "number"}, "normalizedY": {"type": "number"}}, "required": ["captureId", "normalizedX", "normalizedY"]}),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimCityCloseGame => (
                "Only on explicit user instruction: request normal SimCity exit through its verified game window. Never force terminate or automatically restart.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::mutating(true, true),
            ),
            Self::SimCityConfirmStage => (
                "Confirm a visually observed startup stage using the exact fresh capture_id that supports the inference.",
                json!({"type": "object", "properties": {"captureId": {"type": "string"}, "stage": {"type": "string"}, "confidence": {"type": "number"}, "note": {"type": ["string", "null"], "default": null}}, "required": ["captureId", "stage", "confidence"]}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimCityCreateSandboxRegion => (
                "Create a new offline Sandbox region through native CreateRegion using an ACTIVE template ID and explicit name.",
                json!({"type": "object", "properties": {"templateId": {"type": "integer"}, "regionName": {"type": "string"}}, "required": ["templateId", "regionName"]}),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimCityEnterGame => (
                "Enter the game using its native RequestUpdaterData UPDATE_COMPLETE command and wait for menu/city readiness.",
                json!({"type": "object", "properties": {"timeoutSeconds": {"type": "integer", "default": 60}}}),
                ToolAnnotations::mutating(true, false),
            ),
            Self::SimCityGetBuildTools => (
                "Inspect available city-building tool palette categories, road/zone/building tool IDs, and currently selected tool inside an active finalized city.",
                json!({"type": "object", "properties": {"category": {"type": ["string", "null"], "default": null}}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimCityGetDiagnostics => (
                "Collect bounded startup timeline, focus/window state, overlay module names, and recent EA/Simoder log tails.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimCityGetNativeSession => (
                "Read native updater/menu/loading/city state through the DLL's verified owning thread. Returns loading finalization and current save identities.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::read_only(false),
            ),
            Self::SimCityGetRegionDetails => (
                "Read GetRegionDetails for an exact region ID, including claimable/claimed boxes and regional city UIDs.",
                json!({"type": "object", "properties": {"regionId": {"type": "integer"}}, "required": ["regionId"]}),
                ToolAnnotations::read_only(false),
            ),
            Self::SimCityGetRegionTemplates => (
                "Read the game's native RegionTemplates data. Use an actual ACTIVE template key in Sandbox creation.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimCityGetRegions => (
                "Read native PlayingRegions, including Sandbox flags and save identities.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::read_only(false),
            ),
            Self::SimCityGetStartupState => (
                "Read the current SimCity startup state without launching, clicking, typing, terminating, or restarting anything.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimCityGetWorldUnit => (
                "Inspect one native slot,stamp reference in the exact loaded Sandbox city: native reference, position, unit name and property key.",
                json!({"type": "object", "properties": {"regionId": {"type": "integer"}, "cityId": {"type": "integer"}, "unitRef": {"type": "string"}}, "required": ["regionId", "cityId", "unitRef"]}),
                ToolAnnotations::read_only(false),
            ),
            Self::SimCityGetWorldUnits => (
                "List a bounded page of native unit references in the exact finalized Sandbox city. Each reference contains both slot and generation stamp.",
                json!({"type": "object", "properties": {"regionId": {"type": "integer"}, "cityId": {"type": "integer"}, "offset": {"type": "integer", "default": 0}, "limit": {"type": "integer", "default": 32}}, "required": ["regionId", "cityId"]}),
                ToolAnnotations::read_only(false),
            ),
            Self::SimCityInspectUiTree => (
                "Inspect the live scrui UI control hierarchy (control IDs, coordinates, pinning, drawable types, styles, and bounding rects) inside EAWebKit.",
                json!({"type": "object", "properties": {"controlId": {"type": ["integer", "null"], "default": null}, "maxDepth": {"type": "integer", "default": 3, "minimum": 0, "maximum": 8}, "visibleOnly": {"type": "boolean", "default": true}, "maxNodes": {"type": "integer", "default": 120, "minimum": 1, "maximum": 300}}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimCityLoadCity => (
                "Load a claimed Sandbox city using native loadGame and await isLoadingFinalized plus matching activeBoxId, ActiveCityUID and RegionID.",
                json!({"type": "object", "properties": {"regionId": {"type": "integer"}, "cityId": {"type": "integer"}, "timeoutSeconds": {"type": "integer", "default": 90}}, "required": ["regionId", "cityId"]}),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimCityMoveCamera => (
                "Move and optionally rotate the active city camera immediately using PostGameCommand camera/immediate/true/move/x,y,z.",
                json!({"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "z": {"type": "number"}, "rotate": {"type": ["number", "null"], "default": null}}, "required": ["x", "y", "z"]}),
                ToolAnnotations::mutating(true, false),
            ),
            Self::SimCityPlaceBuilding => (
                "Select a building/plop tool by toolId, optionally rotate using ',' or '.', click normalized viewport coordinates, and report newly spawned unit references.",
                json!({"type": "object", "properties": {"toolId": {"type": "integer"}, "normalizedX": {"type": "number"}, "normalizedY": {"type": "number"}, "rotateSteps": {"type": "integer", "default": 0}}, "required": ["toolId", "normalizedX", "normalizedY"]}),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimCityQueryUiDom => (
                "Query live EAWebKit DOM elements by CSS selector and inspect tag, attributes, bounding rects, parent linkage, and computed CSS styles.",
                json!({"type": "object", "properties": {"selector": {"type": "string"}, "limit": {"type": "integer", "default": 32, "minimum": 1, "maximum": 100}, "includeComputedStyle": {"type": "boolean", "default": true}}, "required": ["selector"]}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimCitySelectTool => (
                "Select (or deselect) a SimCity build/demolish tool ID via PostGameCommand selectTool/deselecttool and optionally set RoadDrawType.",
                json!({"type": "object", "properties": {"toolId": {"type": ["integer", "null"], "default": null}, "deselect": {"type": "boolean", "default": false}, "roadDrawType": {"type": ["string", "null"], "enum": ["curvy", "straight", "grid", "arc", "circle", null], "default": null}}}),
                ToolAnnotations::mutating(true, false),
            ),
            Self::SimCitySendKey => (
                "Send exactly one Escape, Enter, or Space key to an allowlisted EA or SimCity window.",
                json!({"type": "object", "properties": {"target": {"type": "string"}, "key": {"type": "string", "enum": ["Escape", "Enter", "Space"]}}, "required": ["target", "key"]}),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimCitySkipIntro => (
                "Skip the intro via the native game UI StopIntroMovie callback and suppress a pending intro in the current launcher.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::mutating(true, false),
            ),
            Self::SimCityStartSession => (
                "Start through EA App or observe the running game. mode is Continue, or Play with exact visible regionName and cityName.",
                json!({"type": "object", "properties": {"mode": {"type": "string", "enum": ["Continue", "Play"]}, "regionName": {"type": ["string", "null"], "default": null}, "cityName": {"type": ["string", "null"], "default": null}}, "required": ["mode"]}),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimCityWaitForStage => (
                "Wait between 1 and 300 seconds for a target stage, returning early for failure, required human action, or suspected Infinite Loading.",
                json!({"type": "object", "properties": {"stage": {"type": "string"}, "timeoutSeconds": {"type": "integer"}}, "required": ["stage", "timeoutSeconds"]}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimoderEvalUiBridge => (
                "Dispatch a validated RequestGameData or PostGameCommand operation across the Backend <-> Frontend EAWebKit bridge (OpenSCP).",
                json!({"type": "object", "properties": {"method": {"type": "string", "enum": ["RequestGameData", "PostGameCommand"]}, "verb": {"type": "string"}}, "required": ["method", "verb"]}),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimoderExportGameCode => (
                "Export a PE analysis artifact to the game's fixed Simoder log directory for REA/Ghidra analysis.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimoderGetBridgeTelemetry => (
                "Inspect the 14-channel Backend <-> Frontend EAWebKit bridge observer, BridgeTime simulation clock/pause state, and CaptureQualityReport (OpenSCP).",
                json!({"type": "object", "properties": {"channel": {"type": ["string", "null"], "default": null}}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimoderGetCapabilities => (
                "Discover actual integration support, OpenSCP extensions, decoupled mod tool count, and fresh loader readiness.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimoderGetGameModules => (
                "List modules of the exact game instance owning the native MCP mod. Returns base addresses and image sizes.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimoderGetLogs => (
                "Read up to 200 recent Simoder log lines, optionally filtering one exact mod ID.",
                json!({"type": "object", "properties": {"modId": {"type": ["string", "null"], "default": null}, "limit": {"type": "integer", "default": 80}}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimoderGetRuntimeState => (
                "Read Simoder mods, resolved overrides, pending changes, conflicts and resource observations.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimoderGetServiceInfo => (
                "Read the actual native MCP mod PID, owning game PID, and ABI v2 transport mode.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimoderGetUiState => (
                "Inspect Simoder's live declarative UI composition state inside EAWebKit: active phase, __simoderUiHost, injected stylesheets, slot containers, and mounted patches.",
                json!({"type": "object", "properties": {}}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimoderInspectSlDelta => (
                "Parse and validate a retail SLDelta v4 container hex payload, checking 0xFF020300 record headers and deltaId monotonicity (OpenSCP).",
                json!({"type": "object", "properties": {"payloadHex": {"type": "string"}}, "required": ["payloadHex"]}),
                ToolAnnotations::read_only(true),
            ),
            Self::SimoderReadGameMemory => (
                "Read 1..65536 bytes at an exact hexadecimal virtual address in the owning game's 32-bit address space.",
                json!({"type": "object", "properties": {"address": {"type": "string"}, "length": {"type": "integer"}}, "required": ["address", "length"]}),
                ToolAnnotations::read_only(false),
            ),
            Self::SimoderSetModEnabled => (
                "Enable or disable one exact discovered mod ID through the running Simoder DLL and return its committed generation.",
                json!({"type": "object", "properties": {"modId": {"type": "string"}, "enabled": {"type": "boolean"}}, "required": ["modId", "enabled"]}),
                ToolAnnotations::mutating(false, false),
            ),
            Self::SimoderUnlockDebugTools => (
                "Build the in-place 4-byte DBPF property patch overlay unlocking SimCity's three retail development tools (0x53B77423, 0xA2AA4268, 0xB65E1F2E) via OpenSCP sc-properties::debug_tools.",
                json!({"type": "object", "properties": {"category": {"type": ["integer", "null"], "default": null}, "uiCategory": {"type": ["integer", "null"], "default": null}}}),
                ToolAnnotations::mutating(true, false),
            ),
        };
        ToolDefinition {
            name: self.as_str().to_string(),
            description: description.to_string(),
            input_schema,
            output_schema: Some(json!({"type": "object", "properties": {"result": true}})),
            annotations,
        }
    }
}
