# Game-owned memory analysis and REA

SimCity can run elevated after EA's UAC prompt. The DLL starts DevBridge and MCP
inside its owned service job, inheriting the game's token. The external MCP
client does not need to read elevated process memory itself. It talks to the
authenticated installed MCP service, which uses the game-owned broker.

## Available tools

| Tool | Result |
| --- | --- |
| `simoder_get_game_modules` | Loaded module names, paths, virtual base addresses and image sizes. |
| `simoder_read_game_memory` | A complete 1–65536-byte read, hex bytes, SHA256, owning PID and observation time. |
| `simoder_export_game_code` | PE analysis artifact, separate provenance file, disk/artifact digests and executable-section mappings. |

These tools require a fresh, consistent, control-capable Loader snapshot for the
broker's owning PID and the exact configured game executable in the same Windows
session. They do not accept another PID, arbitrary output paths or memory writes.
They do not require a foreground game window. Unreadable/partial ranges and
process exit are errors, not successful reads padded with zeroes.

Use `simoder_get_capabilities` and module discovery first. Addresses supplied to
the memory reader are hexadecimal virtual addresses, such as `0x400000`, not file
offsets. For a static location, derive its RVA from the source image base and add
the observed loaded module base. Do not assume ASLR preserves absolute addresses.
Runtime data may change between reads; a byte observation is not an atomic
simulation snapshot or gameplay acceptance.

## Import code into REA

1. Call `simoder_export_game_code`.
2. Verify `artifactSha256` against the returned `artifactPath`; retain the
   `provenancePath`, source digest, PID, loaded base and section mappings.
3. Open that artifact as a separate native PE target with REA `open_binary`, or
   use `rea analyze <artifactPath> --provider ghidra --format json`.
4. Preserve REA Evidence IDs and its target digest. Compare them to the exported
   artifact, not to the original disk EXE's digest.
5. Convert REA locations through the exported module base and section mapping
   before querying live bytes. Revalidate the owning game after a restart.

The artifact retains the original on-disk PE layout and replaces executable
sections with two matching observations of live code. Its image base is the
observed loaded base. Non-code sections retain disk bytes; writable runtime
data and heap are not exported. Existing game/mod hooks are included. Imports,
calling conventions and object lifetimes remain unverified. This is an analysis
artifact, not a repaired runnable EXE or a complete process dump.

Artifact publication uses the fixed Simoder log directory, rejects linked paths,
preserves existing evidence and gives each provenance record its own name. Code
reads and artifact creation do not mutate the game. Scope interoperability to
the tested REA/Ghidra import and explicit address/provenance workflow; do not
claim equivalence to every REA runtime provider.

## Verified integration

The 2026-10-13 acceptance run read the elevated EA-launched game through its
owned broker without foreground input, discovered modules, read the MZ header,
and exported its live executable section. REA 6.4.0 with Ghidra 12.1.4 accepted
the exported native x86 PE and reported the same artifact SHA256. The initial
330-second import deadline expired; the supported
`REA_GHIDRA_STARTUP_TIMEOUT_MS=1200000` setting allowed completion. The run used
`GHIDRA_HEADLESS_MAXMEM=4G`. Choose resource controls for the analysis host;
these limits configure analysis, not the game's services.

This verifies the code-export/import workflow, not region creation, construction
or every REA runtime provider. Keep exact PIDs, digests, raw results and REA
Evidence identifiers in ignored `.analysis/`, rather than maintained guides.
