/** @summary Complete in-process ES5 EAWebKit session, region/city CRUD, world inspection,
 * and city-building router executing against SimCity's verified `window.Client` & `window.simcity` bindings.
 * @param packetText JSON-encoded command packet `{ op, ... }`.
 * @return JSON string `{ ok: true, operation, result }` or `{ ok: false, partial?, error }`.
 */
(function (packetText) {
    "use strict";
    var partial;
    try {
        var packet = JSON.parse(packetText);
        var client = window.Client;
        var game = window.simcity;
        if (!client || !game) {
            throw new Error("The game UI Client is not ready.");
        }

        /** @summary Preserve native JSON errors and unwrap the response envelope used by the game UI. */
        var data = function (query) {
            if (typeof client.RequestGameData !== "function") {
                throw new Error("Game data bindings are not ready.");
            }
            var value = client.RequestGameData(query);
            if (typeof value === "string") {
                value = JSON.parse(value);
            }
            if (
                value &&
                Object.prototype.toString.call(value) === "[object Array]" &&
                value.length === 1 &&
                value[0] &&
                value[0].json !== undefined
            ) {
                value = value[0];
            }
            if (value && value.json !== undefined) {
                value = value.json;
            }
            if (value === undefined || value === null) {
                throw new Error("The native query returned no result: " + query);
            }
            if (
                value.error ||
                (value.errorCode !== undefined && Number(value.errorCode) !== 0)
            ) {
                throw new Error(
                    "Native query failed: " +
                        query +
                        " (" +
                        String(value.errorCode || value.error) +
                        ")"
                );
            }
            return value;
        };

        /** @summary Best-effort RequestGameData that returns null rather than aborting inspection loops. */
        var tryData = function (query) {
            try {
                return data(query);
            } catch (_e) {
                return null;
            }
        };

        /** @summary Validate a decimal native identifier without allowing query path injection. */
        var id = function (value) {
            var text = String(value);
            if (!/^[1-9][0-9]{0,9}$/.test(text) || Number(text) > 4294967295) {
                throw new Error("Invalid native identifier.");
            }
            return text;
        };

        /** @summary Extract numeric instance ID from a raw u32, numeric string, or `{ instance }` key object. */
        var keyInstance = function (entry) {
            if (entry === null || entry === undefined) {
                return null;
            }
            if (typeof entry === "number" && isFinite(entry)) {
                return Math.floor(entry) >>> 0;
            }
            if (typeof entry === "string" && /^[0-9]{1,10}$/.test(entry)) {
                return Number(entry) >>> 0;
            }
            if (typeof entry === "object" && entry.instance !== undefined) {
                var n = Number(entry.instance);
                return isFinite(n) ? (Math.floor(n) >>> 0) : null;
            }
            return null;
        };

        /** @summary Format a key entry into canonical string form for `properties/<key>/...` queries. */
        var keyString = function (entry) {
            if (window.scrui && typeof window.scrui.RequestStringFromKey === "function") {
                try {
                    var formatted = window.scrui.RequestStringFromKey(entry);
                    if (formatted) {
                        return formatted;
                    }
                } catch (_e) {}
            }
            if (typeof entry === "string") {
                return entry;
            }
            if (entry && typeof entry === "object" && entry.instance !== undefined) {
                var groupHex = Number(entry.group || 0).toString(16);
                var typeHex = Number(entry.type || 0).toString(16);
                var instHex = Number(entry.instance || 0).toString(16);
                return "0x" + instHex + "!0x" + groupHex + "!0x" + typeHex;
            }
            return String(entry);
        };

        /** @summary Pure ES5 lowercase FNV-1 32-bit fallback matching `EA::StdC::FNV1_String8(..., kCharCaseLower)`. */
        var fnv1Lower = function (raw) {
            var s = String(raw).toLowerCase();
            if (window.scrui && typeof window.scrui.IDFromName === "function") {
                return Number(window.scrui.IDFromName(raw)) >>> 0;
            }
            var h = 0x811c9dc5;
            for (var i = 0; i < s.length; i++) {
                var hi = (h >>> 16) & 0xffff;
                var lo = h & 0xffff;
                h = ((lo * 0x01000193) + (((hi * 0x0193) & 0xffff) << 16)) >>> 0;
                h = (h ^ s.charCodeAt(i)) >>> 0;
            }
            return h >>> 0;
        };

        /** @summary Validate a game-authored query segment before any region or city mutation. */
        var name = function (value) {
            if (
                typeof value !== "string" ||
                !value.length ||
                value.length > 128 ||
                /[\/\x00-\x1f<>]/.test(value) ||
                value.replace(/^\s+|\s+$/g, "") !== value
            ) {
                throw new Error("Invalid region or city name.");
            }
            return value;
        };

        /** @summary Read current readiness and authoritative loaded-city identities independently. */
        var observe = function () {
            if (game.gUpdater && typeof client.RequestUpdaterData === "function") {
                return {
                    phase: "updater",
                    updaterState: game.gUpdater.mCurrentState,
                    nativeUpdaterState: client.RequestUpdaterData(252943297),
                    blocked: !!(
                        game.gUpdater.mBlockPlay ||
                        game.gUpdater.mBlockUpdate ||
                        game.gUpdater.mBetaBlocked ||
                        game.gUpdater.mWaitForConfiguration
                    )
                };
            }
            if (game.gCurtainUIHandler && game.gCurtainUIHandler.mMoviePlaying === true) {
                return { phase: "intro", loadingFinalized: false, introPlaying: true };
            }
            var finalized = data("isLoadingFinalized");
            if (!finalized) {
                return {
                    phase: game.cLauncher3D && game.cLauncher3D.mPreloadDone === true ? "main_menu" : "loading",
                    loadingFinalized: false
                };
            }
            var cityId = String(data("activeBoxId"));
            return {
                phase: cityId !== "0" ? "city" : "main_menu",
                loadingFinalized: !!finalized,
                cityId: cityId,
                cityUid: String(data("ActiveCityUID")),
                regionId: String(data("RegionID"))
            };
        };

        /** @summary Require an active finalized city simulation context. */
        var requireActiveCity = function () {
            var world = observe();
            if (world.phase !== "city" || !world.loadingFinalized) {
                throw new Error("City simulation is not active and finalized (phase=" + String(world.phase) + ").");
            }
            return world;
        };

        /** @summary Require an existing Sandbox region and select the requested box by its canonical ID. */
        var box = function (regionId, cityId) {
            var region = data("GetRegionDetails/" + id(regionId));
            if (region.sandbox !== true && region.sandbox !== "true") {
                throw new Error("The target region is not Sandbox.");
            }
            if (String(region.id) !== id(regionId) || !region.boxes) {
                throw new Error("Region identity is inconsistent.");
            }
            for (var index = 0; index < region.boxes.length; index++) {
                if (String(region.boxes[index].id) === id(cityId)) {
                    return { region: region, city: region.boxes[index] };
                }
            }
            throw new Error("The requested city does not belong to the Sandbox region.");
        };

        /** @summary Enumerate canonical `slot,stamp` unit keys inside `units`. */
        var listUnitRefs = function () {
            var rawUnits = data("units");
            var refs = Object.keys(rawUnits).filter(function (value) {
                if (!/^(0|[1-9][0-9]{0,9}),(0|[1-9][0-9]{0,9})$/.test(value)) {
                    return false;
                }
                var pair = value.split(",");
                return Number(pair[0]) <= 2147483647 && Number(pair[1]) <= 4294967295;
            });
            refs.sort();
            return refs;
        };

        /** @summary Resolve category tools from `uiToolCategory/<inst>/184264845` (retail `kPropToolCategoryToolList`). */
        var readCategoryTools = function (catInst) {
            var rawList = tryData("uiToolCategory/" + String(catInst) + "/184264845");
            if (!rawList || Object.prototype.toString.call(rawList) !== "[object Array]") {
                return [];
            }
            var items = [];
            for (var i = 0; i < rawList.length && items.length < 64; i++) {
                var tid = keyInstance(rawList[i]);
                if (!tid) {
                    continue;
                }
                var info = tryData("tools/" + String(tid) + "/toolInfo");
                items.push({
                    toolId: tid,
                    key: keyString(rawList[i]),
                    info: info
                });
            }
            return items;
        };

        var KNOWN_CATEGORY_MAP = {
            roads: 1577763374,
            zones: 831766718,
            water: 2654748154,
            sewage: 3441286197,
            garbage: 2092055118,
            police: 291106007,
            education: 2510935805,
            health: 3677180323,
            tourism: 2308696688,
            trading: 3339761385
        };

        var result;
        switch (packet.op) {
        case "skip-intro":
            if (game.cLauncher3D) {
                game.cLauncher3D.mSkipIntroMovie = true;
            }
            var curtain = game.gCurtainUIHandler;
            var wasPlaying = !!(curtain && curtain.mMoviePlaying === true);
            if (wasPlaying) {
                if (typeof curtain.StopIntroMovie !== "function") {
                    throw new Error("The native intro completion callback is not available.");
                }
                curtain.StopIntroMovie();
            }
            result = {
                requested: true,
                wasPlaying: wasPlaying,
                introPlaying: !!(curtain && curtain.mMoviePlaying === true)
            };
            if (result.introPlaying) {
                throw new Error("The intro did not acknowledge its native stop callback.");
            }
            break;

        case "world-units":
        case "world-unit":
            var world = requireActiveCity();
            if (world.regionId !== id(packet.regionId) || world.cityId !== id(packet.cityId)) {
                throw new Error("The requested Sandbox city is not the finalized active city.");
            }
            var region = data("GetRegionDetails/" + id(packet.regionId));
            if (region.sandbox !== true && region.sandbox !== "true") {
                throw new Error("World inspection requires Sandbox.");
            }
            if (packet.op === "world-unit") {
                var reference = packet.argument;
                if (!/^(0|[1-9][0-9]{0,9}),(0|[1-9][0-9]{0,9})$/.test(reference)) {
                    throw new Error("Use an exact slot,stamp unit reference.");
                }
                var referenceParts = reference.split(",");
                if (Number(referenceParts[0]) > 2147483647 || Number(referenceParts[1]) > 4294967295) {
                    throw new Error("Unit reference exceeds its native bounds.");
                }
                result = {
                    unitRef: reference,
                    nativeUnitRef: data("units/" + reference + "/unitRef"),
                    position: data("units/" + reference + "/position"),
                    unitName: data("units/" + reference + "/unitName"),
                    propKey: data("units/" + reference + "/propKey")
                };
            } else {
                var page = packet.argument.split(",");
                if (
                    page.length !== 2 ||
                    !/^(0|[1-9][0-9]{0,5})$/.test(page[0]) ||
                    !/^[1-9][0-9]?$/.test(page[1]) ||
                    Number(page[1]) > 64
                ) {
                    throw new Error("Use a bounded unit page: offset 0..999999, limit 1..64.");
                }
                var references = listUnitRefs();
                result = {
                    regionId: world.regionId,
                    cityId: world.cityId,
                    cityUid: world.cityUid,
                    total: references.length,
                    offset: Number(page[0]),
                    unitRefs: references.slice(Number(page[0]), Number(page[0]) + Number(page[1]))
                };
            }
            break;

        case "observe":
            result = observe();
            break;

        case "enter-menu":
            result = observe();
            if (result.phase !== "updater") {
                break;
            }
            if (result.updaterState !== 2 || result.blocked) {
                throw new Error(
                    "The updater is not ready to enter the game; state=" + String(result.updaterState)
                );
            }
            client.RequestUpdaterData(2052759671);
            result = { queued: true, phase: "entering_menu" };
            break;

        case "region-templates":
            result = data("RegionTemplates");
            break;

        case "regions":
            result = data("PlayingRegions");
            break;

        case "region-details":
            result = data("GetRegionDetails/" + id(packet.regionId));
            break;

        case "create-sandbox-region":
            var templateId = id(packet.templateId);
            var templates = data("RegionTemplates").regionTemplates;
            var found = false;
            if (!templates) {
                throw new Error("Region templates are not available.");
            }
            for (var key in templates) {
                if (
                    Object.prototype.hasOwnProperty.call(templates, key) &&
                    String(templates[key].key) === templateId &&
                    templates[key].status === "ACTIVE"
                ) {
                    found = true;
                }
            }
            if (!found) {
                throw new Error("The requested region template is not active.");
            }
            result = data("CreateRegion/" + name(packet.regionName) + "/" + templateId + "/true");
            if (!result.id) {
                throw new Error(
                    "The native region result lacks an identity; do not retry creation implicitly."
                );
            }
            partial = { createdRegionId: String(result.id) };
            result = data("GetRegionDetails/" + id(result.id));
            if (result.sandbox !== true && result.sandbox !== "true") {
                throw new Error(
                    "The created region did not confirm Sandbox; inspect its identity before retrying."
                );
            }
            break;

        case "claim-city":
            var target = box(packet.regionId, packet.cityId);
            if (target.city.isClaimed === true || target.city.isClaimed === "true") {
                throw new Error("The city is already claimed.");
            }
            data("CreateBox/" + id(packet.regionId) + "/" + id(packet.cityId) + "/" + name(packet.cityName));
            partial = {
                regionId: id(packet.regionId),
                cityId: id(packet.cityId),
                claimMayHaveCompleted: true
            };
            result = box(packet.regionId, packet.cityId);
            if (result.city.isClaimed !== true && result.city.isClaimed !== "true") {
                throw new Error("Native claim did not confirm ownership; do not retry implicitly.");
            }
            break;

        case "load-city":
            var selected = box(packet.regionId, packet.cityId);
            if (selected.city.isClaimed !== true && selected.city.isClaimed !== "true") {
                throw new Error("The requested city is not claimed.");
            }
            if (typeof client.PostGameCommand !== "function") {
                throw new Error("Game command bindings are not ready.");
            }
            client.PostGameCommand("loadGame/" + id(packet.cityId));
            result = {
                queued: true,
                regionId: id(packet.regionId),
                cityId: id(packet.cityId),
                cityUid: String(selected.city.uid)
            };
            break;

        case "get-build-tools":
            requireActiveCity();
            var catFilter = packet.category ? String(packet.category).toLowerCase() : "all";
            var paletteCats = tryData("sortedToolPaletteCategories/161790467") || [];
            var categoriesOut = [];
            var toolsOut = {};
            var collectCat = function (label, catKeyOrInst) {
                var inst = keyInstance(catKeyOrInst);
                if (!inst) {
                    return;
                }
                var subGroup = tryData("properties/" + keyString(catKeyOrInst) + "/203785649");
                var directTools = readCategoryTools(inst);
                var subCategories = [];
                if (subGroup !== null && subGroup !== undefined) {
                    var subList = tryData("sortedToolPaletteCategories/" + String(subGroup)) || [];
                    for (var j = 0; j < subList.length && j < 32; j++) {
                        var subInst = keyInstance(subList[j]);
                        if (subInst) {
                            var subTools = readCategoryTools(subInst);
                            subCategories.push({ subCategoryId: subInst, tools: subTools });
                            if (!toolsOut[label]) {
                                toolsOut[label] = [];
                            }
                            for (var k = 0; k < subTools.length; k++) {
                                toolsOut[label].push(subTools[k]);
                            }
                        }
                    }
                }
                if (directTools.length > 0) {
                    if (!toolsOut[label]) {
                        toolsOut[label] = [];
                    }
                    for (var m = 0; m < directTools.length; m++) {
                        toolsOut[label].push(directTools[m]);
                    }
                }
                categoriesOut.push({
                    label: label,
                    categoryId: inst,
                    directToolCount: directTools.length,
                    subCategoryCount: subCategories.length
                });
            };

            if (catFilter !== "all" && KNOWN_CATEGORY_MAP[catFilter]) {
                collectCat(catFilter, KNOWN_CATEGORY_MAP[catFilter]);
            } else {
                for (var c = 0; c < paletteCats.length && c < 32; c++) {
                    var cid = keyInstance(paletteCats[c]);
                    var label = "category_" + String(cid);
                    for (var kn in KNOWN_CATEGORY_MAP) {
                        if (
                            Object.prototype.hasOwnProperty.call(KNOWN_CATEGORY_MAP, kn) &&
                            KNOWN_CATEGORY_MAP[kn] === cid
                        ) {
                            label = kn;
                            break;
                        }
                    }
                    if (catFilter === "all" || catFilter === label || catFilter === "buildings") {
                        collectCat(label, paletteCats[c]);
                    }
                }
                if (categoriesOut.length === 0) {
                    collectCat("roads", 1577763374);
                    collectCat("zones", 831766718);
                }
            }

            result = {
                selectedToolId: tryData("selectedTool/toolID"),
                knownCategories: KNOWN_CATEGORY_MAP,
                knownZoneTools: {
                    residential: fnv1Lower("LotZoneResidential"),
                    commercial: fnv1Lower("LotZoneCommercial"),
                    industrial: fnv1Lower("LotZoneIndustrial")
                },
                bulldozerToolId: 2536567712,
                roadDrawTypes: { curvy: 0, straight: 1, grid: 2, arc: 3, circle: 4 },
                categories: categoriesOut,
                toolsByCategory: toolsOut
            };
            break;

        case "select-tool":
            requireActiveCity();
            if (typeof client.PostGameCommand !== "function") {
                throw new Error("Game command bindings are not ready.");
            }
            var chosenId = null;
            if (packet.deselect === true || packet.toolId === null || packet.toolId === undefined) {
                if (!packet.zoneType) {
                    client.PostGameCommand("deselecttool");
                }
            }
            if (packet.zoneType) {
                var z = String(packet.zoneType).toLowerCase();
                var zoneTools = readCategoryTools(831766718);
                if (z === "unzone") {
                    var resId = fnv1Lower("LotZoneResidential");
                    var comId = fnv1Lower("LotZoneCommercial");
                    var indId = fnv1Lower("LotZoneIndustrial");
                    for (var zi = 0; zi < zoneTools.length; zi++) {
                        var zt = zoneTools[zi].toolId;
                        if (zt && zt !== resId && zt !== comId && zt !== indId) {
                            chosenId = zt;
                            break;
                        }
                    }
                    if (!chosenId) {
                        chosenId = 2088406873;
                    }
                } else if (z === "residential") {
                    chosenId = packet.toolId ? Number(id(packet.toolId)) : fnv1Lower("LotZoneResidential");
                } else if (z === "commercial") {
                    chosenId = packet.toolId ? Number(id(packet.toolId)) : fnv1Lower("LotZoneCommercial");
                } else if (z === "industrial") {
                    chosenId = packet.toolId ? Number(id(packet.toolId)) : fnv1Lower("LotZoneIndustrial");
                } else {
                    throw new Error("Unsupported zoneType: " + String(packet.zoneType));
                }
            } else if (packet.toolId !== null && packet.toolId !== undefined && packet.deselect !== true) {
                chosenId = Number(id(packet.toolId));
            } else if (packet.pickDefaultRoad === true) {
                var roadTools = readCategoryTools(1577763374);
                if (roadTools.length === 0) {
                    var roadCats = tryData("sortedToolPaletteCategories/1577763374") || [];
                    for (var ri = 0; ri < roadCats.length && roadTools.length === 0; ri++) {
                        var subId = keyInstance(roadCats[ri]);
                        if (subId) {
                            roadTools = readCategoryTools(subId);
                        }
                    }
                }
                if (roadTools.length === 0) {
                    throw new Error("No road tools could be discovered automatically; pass explicit toolId.");
                }
                chosenId = roadTools[0].toolId;
            }

            if (chosenId !== null) {
                client.PostGameCommand("selectTool/" + String(chosenId));
            }
            if (packet.roadDrawType !== undefined && packet.roadDrawType !== null) {
                var modeNum = Number(packet.roadDrawType);
                if (!isFinite(modeNum) || modeNum < 0 || modeNum > 4) {
                    throw new Error("roadDrawType must be 0..4.");
                }
                client.PostGameCommand("RoadDrawType/" + String(Math.floor(modeNum)));
            }
            result = {
                requestedToolId: chosenId,
                deselected: !!(packet.deselect === true && chosenId === null),
                selectedToolId: tryData("selectedTool/toolID"),
                roadDrawType: packet.roadDrawType !== undefined ? packet.roadDrawType : null
            };
            break;

        case "move-camera":
            requireActiveCity();
            if (typeof client.PostGameCommand !== "function") {
                throw new Error("Game command bindings are not ready.");
            }
            var cx = Number(packet.x);
            var cy = Number(packet.y);
            var cz = Number(packet.z);
            if (!isFinite(cx) || !isFinite(cy) || !isFinite(cz)) {
                throw new Error("Camera x, y, z must be finite numbers.");
            }
            var cmdParts = ["camera", "immediate", "true", "move", String(cx) + ", " + String(cy) + ", " + String(cz)];
            if (packet.rotate !== undefined && packet.rotate !== null) {
                var rot = Number(packet.rotate);
                if (!isFinite(rot)) {
                    throw new Error("Camera rotate must be a finite number.");
                }
                cmdParts.push("rotate");
                cmdParts.push(String(rot));
            }
            client.PostGameCommand(cmdParts.join("/"));
            result = {
                moved: true,
                command: cmdParts.join("/"),
                camera: tryData("camera")
            };
            break;

        case "zone-lots":
            requireActiveCity();
            result = {
                residential: tryData("tools/" + String(fnv1Lower("LotZoneResidential")) + "/toolZoneLots"),
                commercial: tryData("tools/" + String(fnv1Lower("LotZoneCommercial")) + "/toolZoneLots"),
                industrial: tryData("tools/" + String(fnv1Lower("LotZoneIndustrial")) + "/toolZoneLots")
            };
            break;

        case "unit-snapshot":
            var active = requireActiveCity();
            var refsNow = listUnitRefs();
            result = {
                regionId: active.regionId,
                cityId: active.cityId,
                cityUid: active.cityUid,
                total: refsNow.length,
                unitRefs: refsNow
            };
            break;

        case "eval-ui-bridge":
            var method = String(packet.method || "");
            var verb = String(packet.verb || "");
            if (!verb.length || verb.length > 256 || !/^[A-Za-z0-9_.,\/\- ]+$/.test(verb)) {
                throw new Error("Invalid bridge verb syntax.");
            }
            if (method === "RequestGameData") {
                result = { method: method, verb: verb, response: data(verb) };
            } else if (method === "PostGameCommand") {
                if (typeof client.PostGameCommand !== "function") {
                    throw new Error("Game command bindings are not ready.");
                }
                client.PostGameCommand(verb);
                result = { method: method, verb: verb, dispatched: true };
            } else {
                throw new Error("method must be RequestGameData or PostGameCommand.");
            }
            break;

        case "inspect-ui-tree":
            var uiMgr = window.gUIManager || (window.scrui && window.scrui.gUIManager);
            if (!uiMgr) {
                throw new Error("scrui.gUIManager is not ready.");
            }
            var rootCtrl = null;
            if (packet.controlId !== undefined && packet.controlId !== null) {
                if (typeof uiMgr.FindControlByID !== "function") {
                    throw new Error("gUIManager.FindControlByID is unavailable.");
                }
                rootCtrl = uiMgr.FindControlByID(Number(packet.controlId));
                if (!rootCtrl) {
                    throw new Error("Control ID " + String(packet.controlId) + " was not found.");
                }
            } else if (typeof uiMgr.GetRootWindow === "function") {
                rootCtrl = uiMgr.GetRootWindow();
            } else if (typeof uiMgr.GetRootLayout === "function") {
                rootCtrl = uiMgr.GetRootLayout();
            }
            if (!rootCtrl) {
                throw new Error("No root UI control is available.");
            }
            var maxDepth = Math.max(0, Math.min(8, Number(packet.maxDepth !== undefined ? packet.maxDepth : 3)));
            var visibleOnly = packet.visibleOnly !== false;
            var maxNodes = Math.max(1, Math.min(300, Number(packet.maxNodes !== undefined ? packet.maxNodes : 120)));
            var uiNodes = [];
            var treeTruncated = false;
            var boxRect = function (el) {
                if (!el || typeof el.getBoundingClientRect !== "function") { return null; }
                var b = el.getBoundingClientRect();
                return {
                    left: Math.round(b.left),
                    top: Math.round(b.top),
                    width: Math.round(b.width),
                    height: Math.round(b.height),
                    right: Math.round(b.right),
                    bottom: Math.round(b.bottom)
                };
            };
            var walkCtrl = function (ctrl, depth) {
                if (!ctrl) { return; }
                if (uiNodes.length >= maxNodes) {
                    treeTruncated = true;
                    return;
                }
                var isVis = typeof ctrl.GetAbsoluteVisibility === "function" ? !!ctrl.GetAbsoluteVisibility() : (ctrl.mIsVisible !== false);
                if (visibleOnly && !isVis) {
                    return;
                }
                var absPos = null;
                if (typeof ctrl.GetAbsolutePosition === "function") {
                    try { absPos = ctrl.GetAbsolutePosition(); } catch (_e) {}
                }
                var txt = "";
                if (ctrl.mTextElement && ctrl.mTextElement.textContent) {
                    txt = String(ctrl.mTextElement.textContent).replace(/\s+/g, " ").replace(/^\s+|\s+$/g, "").slice(0, 120);
                } else if (ctrl.mRootElement && ctrl.mRootElement.textContent) {
                    txt = String(ctrl.mRootElement.textContent).replace(/\s+/g, " ").replace(/^\s+|\s+$/g, "").slice(0, 120);
                }
                var styleCopy = null;
                if (ctrl.mStyleList && ctrl.mStyleList.length) {
                    styleCopy = [];
                    for (var si = 0; si < ctrl.mStyleList.length && si < 8; si++) {
                        styleCopy.push(ctrl.mStyleList[si]);
                    }
                }
                uiNodes.push({
                    depth: depth,
                    controlId: ctrl.mControlID || null,
                    parentId: ctrl.mParentControl ? (ctrl.mParentControl.mControlID || null) : null,
                    visible: isVis,
                    left: ctrl.mLeft !== undefined ? ctrl.mLeft : null,
                    top: ctrl.mTop !== undefined ? ctrl.mTop : null,
                    width: ctrl.mWidth !== undefined ? ctrl.mWidth : null,
                    height: ctrl.mHeight !== undefined ? ctrl.mHeight : null,
                    absX: absPos && absPos.length >= 2 ? absPos[0] : null,
                    absY: absPos && absPos.length >= 2 ? absPos[1] : null,
                    hPin: ctrl.mHorizontalPinType !== undefined ? ctrl.mHorizontalPinType : null,
                    vPin: ctrl.mVerticalPinType !== undefined ? ctrl.mVerticalPinType : null,
                    drawableType: ctrl.mDrawableType !== undefined ? ctrl.mDrawableType : null,
                    styleList: styleCopy,
                    blurBackground: !!ctrl.mBlurBackground,
                    className: ctrl.mRootElement ? String(ctrl.mRootElement.className || "") : "",
                    overflow: (ctrl.mRootElement && ctrl.mRootElement.style) ? String(ctrl.mRootElement.style.overflow || "") : "",
                    rect: boxRect(ctrl.mRootElement),
                    text: txt,
                    childCount: ctrl.mChildren ? ctrl.mChildren.length : 0
                });
                if (depth < maxDepth && ctrl.mChildren && ctrl.mChildren.length) {
                    for (var ci = 0; ci < ctrl.mChildren.length; ci++) {
                        walkCtrl(ctrl.mChildren[ci], depth + 1);
                        if (treeTruncated) { break; }
                    }
                }
            };
            walkCtrl(rootCtrl, 0);
            result = {
                viewport: {
                    width: window.innerWidth || 0,
                    height: window.innerHeight || 0
                },
                rootControlId: rootCtrl.mControlID || null,
                nodeCount: uiNodes.length,
                truncated: treeTruncated,
                nodes: uiNodes
            };
            break;

        case "query-ui-dom":
            var sel = String(packet.selector || "");
            if (!sel.length || sel.length > 256) {
                throw new Error("selector must be 1..256 characters.");
            }
            if (!window.document || typeof window.document.querySelectorAll !== "function") {
                throw new Error("DOM querySelectorAll is unavailable.");
            }
            var matched = window.document.querySelectorAll(sel);
            var domLimit = Math.max(1, Math.min(100, Number(packet.limit !== undefined ? packet.limit : 32)));
            var wantStyle = packet.includeComputedStyle !== false;
            var domOut = [];
            for (var di = 0; di < matched.length && domOut.length < domLimit; di++) {
                var el = matched[di];
                var r = typeof el.getBoundingClientRect === "function" ? el.getBoundingClientRect() : null;
                var cs = (wantStyle && typeof window.getComputedStyle === "function") ? window.getComputedStyle(el, null) : null;
                var attrs = {};
                if (el.getAttribute) {
                    var attrNames = ["data-simoder-slot", "data-simoder-view", "data-simoder-patch", "data-simoder-owner", "data-simoder-gen"];
                    for (var ai = 0; ai < attrNames.length; ai++) {
                        var av = el.getAttribute(attrNames[ai]);
                        if (av !== null && av !== undefined && av !== "") {
                            attrs[attrNames[ai]] = String(av);
                        }
                    }
                }
                domOut.push({
                    tag: el.tagName ? String(el.tagName) : "",
                    id: el.id ? String(el.id) : "",
                    className: el.className ? String(el.className) : "",
                    text: el.textContent ? String(el.textContent).replace(/\s+/g, " ").replace(/^\s+|\s+$/g, "").slice(0, 120) : "",
                    simoderAttrs: attrs,
                    rect: r ? {
                        left: Math.round(r.left),
                        top: Math.round(r.top),
                        width: Math.round(r.width),
                        height: Math.round(r.height),
                        right: Math.round(r.right),
                        bottom: Math.round(r.bottom)
                    } : null,
                    parent: el.parentNode ? {
                        tag: el.parentNode.tagName ? String(el.parentNode.tagName) : "",
                        id: el.parentNode.id ? String(el.parentNode.id) : "",
                        className: el.parentNode.className ? String(el.parentNode.className) : ""
                    } : null,
                    computedStyle: cs ? {
                        display: String(cs.display || ""),
                        position: String(cs.position || ""),
                        visibility: String(cs.visibility || ""),
                        opacity: String(cs.opacity || ""),
                        zIndex: String(cs.zIndex || ""),
                        left: String(cs.left || ""),
                        top: String(cs.top || ""),
                        width: String(cs.width || ""),
                        height: String(cs.height || ""),
                        overflow: String(cs.overflow || ""),
                        color: String(cs.color || ""),
                        fontSize: String(cs.fontSize || ""),
                        fontFamily: String(cs.fontFamily || ""),
                        fontWeight: String(cs.fontWeight || ""),
                        backgroundColor: String(cs.backgroundColor || ""),
                        backgroundImage: String(cs.backgroundImage || "").slice(0, 160),
                        border: String(cs.border || ""),
                        borderRadius: String(cs.borderRadius || "")
                    } : null
                });
            }
            result = {
                viewport: {
                    width: window.innerWidth || 0,
                    height: window.innerHeight || 0
                },
                selector: sel,
                matchedCount: matched.length,
                elements: domOut
            };
            break;

        case "simoder-ui-state":
            var docRef = window.document;
            var uiHost = window.__simoderUiHost || null;
            var styleEl = docRef && docRef.getElementById ? docRef.getElementById("simoder-ui-styles") : null;
            var slotEls = (docRef && docRef.querySelectorAll) ? docRef.querySelectorAll("[data-simoder-slot]") : [];
            var patchEls = (docRef && docRef.querySelectorAll) ? docRef.querySelectorAll("[data-simoder-patch]") : [];
            var slotList = [];
            for (var sli = 0; sli < slotEls.length; sli++) {
                var se = slotEls[sli];
                var sr = typeof se.getBoundingClientRect === "function" ? se.getBoundingClientRect() : null;
                var scs = typeof window.getComputedStyle === "function" ? window.getComputedStyle(se, null) : null;
                slotList.push({
                    id: se.id ? String(se.id) : "",
                    slot: se.getAttribute ? String(se.getAttribute("data-simoder-slot") || "") : "",
                    view: se.getAttribute ? String(se.getAttribute("data-simoder-view") || "") : "",
                    parentTag: se.parentNode && se.parentNode.tagName ? String(se.parentNode.tagName) : "",
                    parentId: se.parentNode && se.parentNode.id ? String(se.parentNode.id) : "",
                    display: scs ? String(scs.display || "") : "",
                    visibility: scs ? String(scs.visibility || "") : "",
                    opacity: scs ? String(scs.opacity || "") : "",
                    rect: sr ? { left: Math.round(sr.left), top: Math.round(sr.top), width: Math.round(sr.width), height: Math.round(sr.height) } : null,
                    childCount: se.childNodes ? se.childNodes.length : 0
                });
            }
            var patchList = [];
            for (var pli = 0; pli < patchEls.length; pli++) {
                var pe = patchEls[pli];
                var pr = typeof pe.getBoundingClientRect === "function" ? pe.getBoundingClientRect() : null;
                patchList.push({
                    id: pe.id ? String(pe.id) : "",
                    patchId: pe.getAttribute ? String(pe.getAttribute("data-simoder-patch") || "") : "",
                    ownerModId: pe.getAttribute ? String(pe.getAttribute("data-simoder-owner") || "") : "",
                    ownerGeneration: pe.getAttribute ? String(pe.getAttribute("data-simoder-gen") || "") : "",
                    rect: pr ? { left: Math.round(pr.left), top: Math.round(pr.top), width: Math.round(pr.width), height: Math.round(pr.height) } : null,
                    text: pe.textContent ? String(pe.textContent).replace(/\s+/g, " ").replace(/^\s+|\s+$/g, "").slice(0, 120) : ""
                });
            }
            result = {
                viewport: {
                    width: window.innerWidth || 0,
                    height: window.innerHeight || 0
                },
                observation: observe(),
                uiHost: uiHost ? {
                    revision: uiHost.revision,
                    reloadGen: uiHost.reloadGen,
                    lastPhase: uiHost.lastPhase,
                    queuedEvents: uiHost.events ? uiHost.events.length : 0,
                    activeRegionContext: uiHost.activeRegionContext || null
                } : null,
                stylesInjected: !!styleEl,
                styleLength: styleEl && styleEl.textContent ? styleEl.textContent.length : 0,
                slots: slotList,
                patches: patchList
            };
            break;

        default:
            throw new Error("Unsupported native session operation.");
        }

        return JSON.stringify({ ok: true, operation: packet.op, result: result });
    } catch (error) {
        return JSON.stringify({
            ok: false,
            partial: partial,
            error: String(error && error.message ? error.message : error)
        });
    }
})
