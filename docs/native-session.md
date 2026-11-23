# Native game sessions

The optional game-owned Developer Tools mod exposes semantic session tools.
Production installation remains one x86 `d3d9.dll` beside `SimCity.exe`; the fixed
native router is embedded in that DLL and uses the game's existing JavaScript
engine. Node.js is not needed to run these session operations.

| Tool | Result |
| --- | --- |
| `simcity_get_native_session` | Updater/menu/loading/city state and current save identities |
| `simcity_enter_game` | Native updater completion command followed by menu readiness |
| `simcity_get_region_templates` | Actual template keys and active status |
| `simcity_get_regions` | Existing region identities and Sandbox metadata |
| `simcity_get_region_details` | Region boxes, claim state and city UIDs |
| `simcity_create_sandbox_region` | New region with verified native identity and Sandbox flag |
| `simcity_claim_city` | Exact unclaimed Sandbox box and verified resulting ownership |
| `simcity_load_city` | Finalized loading with matching box ID, city UID and region ID |

Launch through `simcity_start_session` first when the game is closed. The user
approves any UAC prompt. Native operations do not simulate clicks, restore or
activate the game window. The DLL verifies its ABI and actual game thread for
each command; an incompatible thread/context returns an explicit failure.

Read template keys rather than guessing them from names. Create a dedicated test
region, select an unclaimed box from its returned metadata, claim it with an
explicit name, then load the returned identities. Already claimed cities and
non-Sandbox regions cannot be overwritten by the claim tool.

Creation/claim/loading are mutations. A failure may retain a partial created
identity or an accepted load command; inspect regions and current session before
retrying. The tools do not retry mutations automatically. Cancellation after a
request is published preserves broker transaction ownership through its outcome.

The offline game's native region creation carries its Sandbox/cheats flag.
Disaster state is a separate game option; online request parameters do not prove
its offline behavior. Save identities and loading readiness are authoritative
native observations, while earlier PROP resource snapshots remain historical.

Installed-game acceptance passed on 2026-10-13: the native updater transitioned
to the main menu, a new TwinCities Sandbox region was created, city 34 was claimed,
and loading completed with city ID 34, UID 1026 and region ID 6. The final menu and
load calls returned `completed: true` while the captured game window was not in
the foreground. An already-claimed city was explicitly rejected before mutation.
Scene replacement may temporarily disable native views; the broker retries only
typed read-only observations in that case. It never replays a mutation command.
Native errors and partial save identities are exposed as MCP tool errors.

Build and isolated protocol tests alone do not establish installed-game acceptance.
Read `simoder_get_capabilities` for current owner-bound admission/thread readiness
and check each tool's actual completion result. Developer services still stop
with their owning game process.
