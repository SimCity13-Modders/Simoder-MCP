//! Native Module Host Boundary (`ABI v2`) for `simoder_mcp.dll` (`Simoder-SDK/Native/native.h`).
//!
//! Exports the five undecorated C ABI v2 lifecycle functions required by `Simoder`:
//! - [`SimoderModule_QueryV2`]
//! - [`SimoderModule_PrepareV2`]
//! - [`SimoderModule_AdmitV2`]
//! - [`SimoderModule_QuiesceV2`]
//! - [`SimoderModule_DrainV2`]
//!
//! Enforces single-owner game-bound lifetime (`U4` / `Simoder-DevTools-MCP#1`):
//! `PrepareV2` stages the generation without opening a listener; `AdmitV2` activates MCP only after
//! native trust and base readiness; `QuiesceV2` immediately closes MCP admission on mod disable or
//! game shutdown; `DrainV2` returns [`SdResult::Busy`] while any tool call is in flight.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
#[cfg(not(test))]
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
#[cfg(not(test))]
use sha2::{Digest, Sha256};
use strum::{FromRepr, IntoStaticStr, VariantArray};

#[cfg(not(test))]
use crate::auth::McpAuthManager;
#[cfg(not(test))]
use crate::config::{
    DEFAULT_LOOPBACK_SESSION_ID, DEFAULT_MCP_HTTP_BIND, ENV_MCP_HTTP_BIND, ENV_MCP_HTTP_TOKEN,
    MCP_SINGLETON_MUTEX_NAME, MCP_TOKEN_RELATIVE_PATH,
};
use crate::config::{
    MAX_MODULE_GENERATIONS, SD_BASE_CONTRACT_VERSION, SD_CONTRACT_SHA256_HEX,
    SD_MODULE_DESCRIPTOR_V2_SIZE, SD_U2A_HOST_VERSION,
};
#[cfg(not(test))]
use crate::host::LiveHostBackend;
use crate::http_server::McpHttpServerHandle;
#[cfg(not(test))]
use crate::protocol::McpServerEngine;

/// Native module ABI v2 result codes (`SdResult` `0..=16` in `Simoder-SDK/Native/{abi_v1.h,native.h}`).
#[repr(u32)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    FromRepr,
    IntoStaticStr,
    VariantArray,
)]
pub enum SdResult {
    /// Operation succeeded (`SD_OK = 0`).
    Ok = 0,
    /// Invalid argument, null pointer, or malformed descriptor (`SD_INVALID = 1`).
    Invalid = 1,
    /// Incompatible host/module ABI version or contract SHA-256 (`SD_VERSION = 2`).
    Version = 2,
    /// Handle, generation, or invocation reference is stale (`SD_STALE = 3`).
    Stale = 3,
    /// Permission, trust, or authentication check denied (`SD_DENIED = 4`).
    Denied = 4,
    /// Required provider or base runtime is not ready (`SD_NOT_READY = 5`).
    NotReady = 5,
    /// Resource or callback is still in flight; retain module without forced unload (`SD_BUSY = 6`).
    Busy = 6,
    /// Duplicate active owner or conflicting state revision (`SD_CONFLICT = 7`).
    Conflict = 7,
    /// Uncommitted operation was cancelled (`SD_CANCELLED = 8`).
    Cancelled = 8,
    /// Outcome could not be confirmed before deadline (`SD_INDETERMINATE = 9`).
    Indeterminate = 9,
    /// Retained native image or generation budget exhausted; process restart required (`SD_RESTART_REQUIRED = 10`).
    RestartRequired = 10,
    /// Caller buffer capacity is smaller than `required` (`SD_BUFFER_SMALL = 11`).
    BufferSmall = 11,
    /// Requested optional capability or cached snapshot is unavailable (`SD_CAPABILITY_UNAVAILABLE = 12`).
    CapabilityUnavailable = 12,
    /// City or region context epoch expired (`SD_CONTEXT_EXPIRED = 13`).
    ContextExpired = 13,
    /// Underlying game or runtime operation failed (`SD_OPERATION_FAILED = 14`).
    OperationFailed = 14,
    /// Per-generation task, mailbox, or subscription cap reached (`SD_RESOURCE_LIMIT = 15`).
    ResourceLimit = 15,
    /// Operation exceeded its bounded execution deadline (`SD_DEADLINE = 16`).
    Deadline = 16,
}

/// Host-allocated opaque handle (`SdHandle`: `slot` + `generation`, 8 bytes).
#[repr(C)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct SdHandle {
    /// 32-bit slot index in the host registry.
    pub slot: u32,
    /// 32-bit monotonic generation counter.
    pub generation: u32,
}

/// Structure negotiation header prefix (`SdHeader`: `size` + `version`, 8 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SdHeader {
    /// Exact structure byte size.
    pub size: u32,
    /// Structure schema version.
    pub version: u32,
}

/// Root host table passed to `SimoderModule_PrepareV2` (`SdHostApiV2` in `Simoder-SDK/Native/native.h`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdHostApiV2 {
    /// Negotiation header (`version == 2`, `size == 32` on x86).
    pub header: SdHeader,
    /// Host-issued native module generation handle.
    pub owner: SdHandle,
    /// Opaque pointer to `SdLifecycleApi` (or `0` when absent).
    pub lifecycle_api_ptr: usize,
    /// Opaque pointer to `SdDispatchApi` (or `0` when absent).
    pub dispatch_api_ptr: usize,
    /// Opaque pointer to `SdCredentialApi` (or `0` when absent).
    pub credential_api_ptr: usize,
    /// Opaque pointer to `SdSemanticApi` (or `0` when absent).
    pub semantic_api_ptr: usize,
}

impl SdHostApiV2 {
    /// Construct a valid `SdHostApiV2` table for `owner`.
    pub const fn new(owner: SdHandle) -> Self {
        Self {
            header: SdHeader {
                size: size_of::<Self>() as u32,
                version: SD_U2A_HOST_VERSION,
            },
            owner,
            lifecycle_api_ptr: 0,
            dispatch_api_ptr: 0,
            credential_api_ptr: 0,
            semantic_api_ptr: 0,
        }
    }
}

/// Optional host-supplied synchronous `EAWebKit` JS evaluator (`SdHostApiV2::semantic_api_ptr`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SdSemanticHostApiV2 {
    /// Exact struct byte size (`>= 12`/`16` on `i686`, `>= 16`/`24` on `x86_64`).
    pub size: u32,
    /// Schema version (`1` or `2`).
    pub version: u32,
    /// Owning-thread JS evaluation callback (`(script_fn, arg_json) -> Result<String, String>`).
    pub eval_ui_js: extern "C" fn(
        script_ptr: *const u8,
        script_len: usize,
        arg_ptr: *const u8,
        arg_len: usize,
        out_buf: *mut u8,
        out_cap: usize,
        out_len: *mut usize,
        timeout_ms: u32,
    ) -> u32,
    /// Optional v2 host log forwarding callback (`(level, source, message) -> SdResult`).
    pub log_message: extern "C" fn(
        level: u32,
        source_ptr: *const u8,
        source_len: usize,
        msg_ptr: *const u8,
        msg_len: usize,
    ) -> u32,
}

static HOST_SEMANTIC_API_PTR: AtomicUsize = AtomicUsize::new(0);

/// Build a safe host-bound [`crate::host::UiJsEvalFn`] closure when `Simoder` provided a valid
/// `SdSemanticHostApiV2` pointer in `SimoderModule_PrepareV2`.
#[cfg(not(test))]
#[allow(unsafe_code)]
fn host_eval_ui_js_bridge() -> Option<crate::host::UiJsEvalFn> {
    let raw_ptr = HOST_SEMANTIC_API_PTR.load(Ordering::Acquire);
    if raw_ptr == 0 {
        return None;
    }
    // SAFETY: `Simoder` keeps `SEMANTIC_HOST_API_V2` static for the process lifetime while this DLL is admitted.
    let size_and_ver = unsafe { *(raw_ptr as *const SdHeader) };
    const V1_MIN_SIZE: usize =
        std::mem::offset_of!(SdSemanticHostApiV2, eval_ui_js) + size_of::<usize>();
    if (size_and_ver.size as usize) < V1_MIN_SIZE || size_and_ver.version < 1 {
        return None;
    }
    // SAFETY: `size_and_ver.size >= V1_MIN_SIZE` guarantees `eval_ui_js` lies within the host struct on both 32-bit and 64-bit targets.
    let eval_fn = unsafe {
        std::ptr::addr_of!((*(raw_ptr as *const SdSemanticHostApiV2)).eval_ui_js).read()
    };
    Some(Arc::new(move |script: &str, arg: &str, timeout_ms: u32| {
        let mut cap = 64 * 1024;
        let mut buf = vec![0u8; cap];
        let mut out_len: usize = 0;
        let mut code = (eval_fn)(
            script.as_ptr(),
            script.len(),
            if arg.is_empty() {
                std::ptr::null()
            } else {
                arg.as_ptr()
            },
            arg.len(),
            buf.as_mut_ptr(),
            cap,
            &mut out_len,
            timeout_ms,
        );
        if code == SdResult::BufferSmall as u32 && out_len > cap && out_len <= 4 * 1024 * 1024 {
            cap = out_len;
            buf.resize(cap, 0);
            code = (eval_fn)(
                script.as_ptr(),
                script.len(),
                if arg.is_empty() {
                    std::ptr::null()
                } else {
                    arg.as_ptr()
                },
                arg.len(),
                buf.as_mut_ptr(),
                cap,
                &mut out_len,
                timeout_ms,
            );
        }
        let actual_len = out_len.min(buf.len());
        let text = String::from_utf8_lossy(&buf[..actual_len]).into_owned();
        if code == SdResult::Ok as u32 {
            Ok(text)
        } else if text.is_empty() {
            Err(format!("Host EAWebKit bridge failed (SdResult={code})"))
        } else {
            Err(text)
        }
    }))
}

#[cfg(not(test))]
#[allow(unsafe_code)]
fn host_log_bridge() -> Option<crate::host::HostLogFn> {
    let raw_ptr = HOST_SEMANTIC_API_PTR.load(Ordering::Acquire);
    if raw_ptr == 0 {
        return None;
    }
    // SAFETY: `Simoder` keeps `SEMANTIC_HOST_API_V2` static for the process lifetime while this DLL is admitted.
    let size_and_ver = unsafe { *(raw_ptr as *const SdHeader) };
    if (size_and_ver.size as usize) < size_of::<SdSemanticHostApiV2>() || size_and_ver.version < 2 {
        return None;
    }
    // SAFETY: Verified v2 table size covers `SdSemanticHostApiV2`.
    let table = unsafe { *(raw_ptr as *const SdSemanticHostApiV2) };
    let log_fn = table.log_message;
    Some(Arc::new(move |level: &str, source: &str, message: &str| {
        let lvl = match level {
            "trace" => 0,
            "debug" => 1,
            "info" => 2,
            "warn" | "warning" => 3,
            _ => 4,
        };
        let _ = (log_fn)(
            lvl,
            source.as_ptr(),
            source.len(),
            message.as_ptr(),
            message.len(),
        );
    }))
}

/// Module contract identity descriptor populated by `SimoderModule_QueryV2` (`SdModuleDescriptorV2`, 56 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdModuleDescriptorV2 {
    /// Negotiation header (`size == 56`, `version == 2`).
    pub header: SdHeader,
    /// Requested host ABI version (`2`).
    pub host_version: u32,
    /// Required base contract version (`1`).
    pub base_contract_version: u32,
    /// Reserved field (must be `0`).
    pub reserved0: u32,
    /// Reserved field (must be `0`).
    pub reserved1: u32,
    /// 32-byte raw SHA-256 digest matching [`SD_CONTRACT_SHA256_HEX`].
    pub contract_sha256: [u8; 32],
}

impl SdModuleDescriptorV2 {
    /// Construct an empty caller-sized V2 descriptor header ready for `SimoderModule_QueryV2`.
    pub const fn new_for_query() -> Self {
        Self {
            header: SdHeader {
                size: SD_MODULE_DESCRIPTOR_V2_SIZE,
                version: SD_U2A_HOST_VERSION,
            },
            host_version: 0,
            base_contract_version: 0,
            reserved0: 0,
            reserved1: 0,
            contract_sha256: [0; 32],
        }
    }
}

/// Internal lifecycle state of one native MCP module generation (`native-module-boundary.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleScopeState {
    /// Staged by `PrepareV2`; MCP listener and tools are not yet callable.
    Prepared,
    /// Admitted by `AdmitV2`; MCP server is active and bound to this single owner generation.
    Active,
    /// Quiesced by `QuiesceV2`; new MCP calls are rejected, waiting for in-flight calls to finish.
    Stopping,
    /// Drained by `DrainV2`; all resources released, generation permanently retired.
    Drained,
}

#[derive(Debug, Clone)]
struct GenerationSlot {
    owner: SdHandle,
    state: ModuleScopeState,
    in_flight_calls: usize,
}

/// Thread-safe lifecycle coordinator backing the native mod `.dll` exports (`U4` single-owner contract).
#[derive(Debug, Default)]
pub struct NativeMcpLifecycle {
    generations: Mutex<Vec<GenerationSlot>>,
}

impl NativeMcpLifecycle {
    /// Create a fresh [`NativeMcpLifecycle`] tracker.
    pub const fn new() -> Self {
        Self {
            generations: Mutex::new(Vec::new()),
        }
    }

    /// Safe implementation of `SimoderModule_QueryV2`.
    pub fn query_v2(&self, host_version: u32, out: &mut SdModuleDescriptorV2) -> SdResult {
        if host_version != SD_U2A_HOST_VERSION
            || out.header.size != SD_MODULE_DESCRIPTOR_V2_SIZE
            || out.header.version != SD_U2A_HOST_VERSION
        {
            return SdResult::Version;
        }
        let Ok(bytes) = hex::decode(SD_CONTRACT_SHA256_HEX) else {
            return SdResult::Invalid;
        };
        let Ok(digest) = <[u8; 32]>::try_from(bytes.as_slice()) else {
            return SdResult::Invalid;
        };
        out.host_version = SD_U2A_HOST_VERSION;
        out.base_contract_version = SD_BASE_CONTRACT_VERSION;
        out.reserved0 = 0;
        out.reserved1 = 0;
        out.contract_sha256 = digest;
        SdResult::Ok
    }

    /// Safe implementation of `SimoderModule_PrepareV2`.
    pub fn prepare_v2(&self, host: &SdHostApiV2) -> SdResult {
        if host.header.version != SD_U2A_HOST_VERSION
            || host.header.size != size_of::<SdHostApiV2>() as u32
            || host.owner.generation == 0
        {
            return SdResult::Invalid;
        }
        let mut gens = self.generations.lock();
        if gens.len() >= MAX_MODULE_GENERATIONS {
            return SdResult::RestartRequired;
        }
        // Reject duplicate active or non-drained owner.
        if gens
            .iter()
            .any(|g| g.owner == host.owner || g.state != ModuleScopeState::Drained)
        {
            return SdResult::Conflict;
        }
        gens.push(GenerationSlot {
            owner: host.owner,
            state: ModuleScopeState::Prepared,
            in_flight_calls: 0,
        });
        SdResult::Ok
    }

    /// Safe implementation of `SimoderModule_AdmitV2`.
    pub fn admit_v2(&self, owner: SdHandle) -> SdResult {
        let mut gens = self.generations.lock();
        if gens
            .iter()
            .any(|g| g.owner != owner && g.state == ModuleScopeState::Active)
        {
            return SdResult::Conflict;
        }
        let Some(slot) = gens.iter_mut().find(|g| g.owner == owner) else {
            return SdResult::Stale;
        };
        if slot.state != ModuleScopeState::Prepared {
            return SdResult::Invalid;
        }
        slot.state = ModuleScopeState::Active;
        SdResult::Ok
    }

    /// Safe implementation of `SimoderModule_QuiesceV2` (idempotent, nonblocking, immediately closes MCP admission).
    pub fn quiesce_v2(&self, owner: SdHandle) -> SdResult {
        let mut gens = self.generations.lock();
        let Some(slot) = gens.iter_mut().find(|g| g.owner == owner) else {
            return SdResult::Stale;
        };
        if slot.state != ModuleScopeState::Drained {
            slot.state = ModuleScopeState::Stopping;
        }
        SdResult::Ok
    }

    /// Safe implementation of `SimoderModule_DrainV2` (returns `SdResult::Busy` while MCP tool calls are in flight).
    pub fn drain_v2(&self, owner: SdHandle) -> SdResult {
        let mut gens = self.generations.lock();
        let Some(slot) = gens.iter_mut().find(|g| g.owner == owner) else {
            return SdResult::Stale;
        };
        if slot.in_flight_calls > 0 {
            return SdResult::Busy;
        }
        slot.state = ModuleScopeState::Drained;
        SdResult::Ok
    }

    /// Return the currently active owner handle, if an admitted generation is running.
    pub fn active_owner(&self) -> Option<SdHandle> {
        let gens = self.generations.lock();
        gens.iter()
            .find(|g| g.state == ModuleScopeState::Active)
            .map(|g| g.owner)
    }

    /// Return the [`ModuleScopeState`] for `owner`.
    pub fn state_of(&self, owner: SdHandle) -> Option<ModuleScopeState> {
        let gens = self.generations.lock();
        gens.iter().find(|g| g.owner == owner).map(|g| g.state)
    }

    /// Try to acquire an in-flight call lease on the currently active generation (`RAII` guard).
    pub fn try_enter_call(&self) -> Result<SdHandle, SdResult> {
        let mut gens = self.generations.lock();
        let Some(slot) = gens.iter_mut().find(|g| g.state == ModuleScopeState::Active) else {
            return Err(SdResult::NotReady);
        };
        slot.in_flight_calls = slot.in_flight_calls.saturating_add(1);
        Ok(slot.owner)
    }

    /// Release an in-flight call lease on `owner`.
    pub fn leave_call(&self, owner: SdHandle) {
        let mut gens = self.generations.lock();
        if let Some(slot) = gens.iter_mut().find(|g| g.owner == owner) {
            slot.in_flight_calls = slot.in_flight_calls.saturating_sub(1);
        }
    }

    /// Simulate an in-flight call count adjustment for deterministic `DrainV2` `SD_BUSY` testing.
    pub fn set_in_flight_for_test(&self, owner: SdHandle, count: usize) {
        let mut gens = self.generations.lock();
        if let Some(slot) = gens.iter_mut().find(|g| g.owner == owner) {
            slot.in_flight_calls = count;
        }
    }
}

static GLOBAL_LIFECYCLE: OnceLock<Arc<NativeMcpLifecycle>> = OnceLock::new();
static GLOBAL_HTTP_SERVER: OnceLock<Mutex<Option<McpHttpServerHandle>>> = OnceLock::new();
#[cfg(not(test))]
static GLOBAL_SINGLETON_MUTEX: OnceLock<Mutex<usize>> = OnceLock::new();

#[cfg(all(windows, not(test)))]
#[allow(unsafe_code)]
mod singleton_ffi {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    const ERROR_ALREADY_EXISTS: u32 = 183;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(
            lp_mutex_attributes: *mut c_void,
            b_initial_owner: i32,
            lp_name: *const u16,
        ) -> *mut c_void;
        fn ReleaseMutex(h_mutex: *mut c_void) -> i32;
        fn CloseHandle(h_object: *mut c_void) -> i32;
        fn GetLastError() -> u32;
    }

    #[allow(unsafe_code)]
    pub(super) fn acquire(name: &str) -> Result<usize, super::SdResult> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: Valid NUL-terminated UTF-16 Win32 named mutex string in local session namespace.
        let handle = unsafe { CreateMutexW(null_mut(), 1, wide.as_ptr()) };
        if handle.is_null() {
            return Err(super::SdResult::OperationFailed);
        }
        // SAFETY: Query immediately after `CreateMutexW` to detect duplicate owner across instances.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(super::SdResult::Conflict);
        }
        Ok(handle as usize)
    }

    #[allow(unsafe_code)]
    pub(super) fn release(raw: usize) {
        if raw != 0 {
            let h = raw as *mut c_void;
            // SAFETY: Valid mutex handle acquired earlier by `acquire`.
            unsafe {
                let _ = ReleaseMutex(h);
                let _ = CloseHandle(h);
            }
        }
    }
}

/// Access the process-wide [`NativeMcpLifecycle`] singleton backing the DLL's C ABI v2 exports.
pub fn global_mcp_lifecycle() -> &'static Arc<NativeMcpLifecycle> {
    GLOBAL_LIFECYCLE.get_or_init(|| Arc::new(NativeMcpLifecycle::new()))
}

fn global_http_slot() -> &'static Mutex<Option<McpHttpServerHandle>> {
    GLOBAL_HTTP_SERVER.get_or_init(|| Mutex::new(None))
}

#[cfg(not(test))]
fn global_singleton_slot() -> &'static Mutex<usize> {
    GLOBAL_SINGLETON_MUTEX.get_or_init(|| Mutex::new(0))
}

#[cfg(not(test))]
fn acquire_singleton_guard() -> SdResult {
    let mut slot = global_singleton_slot().lock();
    if *slot != 0 {
        return SdResult::Ok;
    }
    #[cfg(windows)]
    {
        match singleton_ffi::acquire(MCP_SINGLETON_MUTEX_NAME) {
            Ok(raw) => {
                *slot = raw;
                SdResult::Ok
            }
            Err(err) => err,
        }
    }
    #[cfg(not(windows))]
    {
        *slot = 1;
        SdResult::Ok
    }
}

#[cfg(not(test))]
fn release_singleton_guard() {
    let raw = {
        let mut slot = global_singleton_slot().lock();
        let prev = *slot;
        *slot = 0;
        prev
    };
    #[cfg(windows)]
    singleton_ffi::release(raw);
    #[cfg(not(windows))]
    let _ = raw;
}

#[cfg(not(test))]
fn detect_game_root() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent().map(Path::to_path_buf))
}

#[cfg(not(test))]
fn resolve_or_create_mcp_token(game_root: Option<&Path>) -> String {
    if let Ok(val) = std::env::var(ENV_MCP_HTTP_TOKEN) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let token_path = game_root.map(|root| root.join(MCP_TOKEN_RELATIVE_PATH));
    if let Some(ref path) = token_path
        && let Ok(existing) = fs::read_to_string(path)
    {
        let trimmed = existing.trim();
        if trimmed.len() >= 16 {
            return trimmed.to_string();
        }
    }

    let stack_marker = 0_usize;
    let heap_marker = Box::new(std::process::id());
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u128, |d| d.as_nanos());
    let mut hasher = Sha256::new();
    hasher.update(now_ns.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update((&stack_marker as *const usize as usize).to_le_bytes());
    hasher.update((&*heap_marker as *const u32 as usize).to_le_bytes());
    let generated = hex::encode(hasher.finalize());

    if let Some(path) = token_path {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, format!("{generated}\n"));
    }
    generated
}

#[cfg(not(test))]
fn start_global_http_server_on_admit() -> SdResult {
    let mutex_res = acquire_singleton_guard();
    if mutex_res != SdResult::Ok {
        return mutex_res;
    }

    let mut slot = global_http_slot().lock();
    if slot.is_some() {
        return SdResult::Ok;
    }
    let lifecycle = Arc::clone(global_mcp_lifecycle());
    let game_root = detect_game_root();
    let token = resolve_or_create_mcp_token(game_root.as_deref());
    let mut auth = McpAuthManager::new(1);
    auth.register_full_session(
        DEFAULT_LOOPBACK_SESSION_ID,
        "antigravity-http-client",
        &token,
    );
    let backend = Arc::new(
        LiveHostBackend::with_js_bridge(
            std::process::id(),
            game_root,
            host_eval_ui_js_bridge(),
        )
        .with_host_log(host_log_bridge()),
    );
    let engine = Arc::new(McpServerEngine::new(lifecycle, auth, backend));
    let bind = std::env::var(ENV_MCP_HTTP_BIND)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MCP_HTTP_BIND.to_string());

    match McpHttpServerHandle::start(
        &bind,
        engine,
        DEFAULT_LOOPBACK_SESSION_ID.to_string(),
        token,
        true,
    ) {
        Ok(handle) => {
            *slot = Some(handle);
            SdResult::Ok
        }
        Err(_) => {
            release_singleton_guard();
            SdResult::Conflict
        }
    }
}

fn stop_global_http_server() {
    if let Some(handle) = global_http_slot().lock().take() {
        handle.stop();
    }
    #[cfg(not(test))]
    release_singleton_guard();
}

/// Required undecorated C ABI v2 export (`SimoderModule_QueryV2`) negotiating host version 2 and contract SHA-256.
// SAFETY: Required undecorated C ABI v2 export symbol (`Simoder-SDK/Native/native.h`) invoked by `Simoder` after PE export verification.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn SimoderModule_QueryV2(
    host_version: u32,
    descriptor: Option<&mut SdModuleDescriptorV2>,
) -> SdResult {
    let Some(desc) = descriptor else {
        return SdResult::Invalid;
    };
    global_mcp_lifecycle().query_v2(host_version, desc)
}

/// Required undecorated C ABI v2 export (`SimoderModule_PrepareV2`) staging a fresh native module generation.
// SAFETY: Required undecorated C ABI v2 export symbol (`Simoder-SDK/Native/native.h`) invoked by `Simoder` outside loader locks.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn SimoderModule_PrepareV2(host: Option<&SdHostApiV2>) -> SdResult {
    let Some(host_ref) = host else {
        return SdResult::Invalid;
    };
    let res = global_mcp_lifecycle().prepare_v2(host_ref);
    if res == SdResult::Ok {
        HOST_SEMANTIC_API_PTR.store(host_ref.semantic_api_ptr, Ordering::Release);
    }
    res
}

/// Required undecorated C ABI v2 export (`SimoderModule_AdmitV2`) activating the MCP service after native trust and base readiness.
// SAFETY: Required undecorated C ABI v2 export symbol (`Simoder-SDK/Native/native.h`) invoked by `Simoder` after native trust and base readiness.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn SimoderModule_AdmitV2(owner: SdHandle) -> SdResult {
    let res = global_mcp_lifecycle().admit_v2(owner);
    #[cfg(not(test))]
    if res == SdResult::Ok {
        let http_res = start_global_http_server_on_admit();
        if http_res != SdResult::Ok {
            let _ = global_mcp_lifecycle().quiesce_v2(owner);
            return http_res;
        }
    }
    res
}

/// Required undecorated C ABI v2 export (`SimoderModule_QuiesceV2`) immediately closing MCP request admission.
// SAFETY: Required undecorated C ABI v2 export symbol (`Simoder-SDK/Native/native.h`) invoked by `Simoder` to close admission nonblockingly.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn SimoderModule_QuiesceV2(owner: SdHandle) -> SdResult {
    HOST_SEMANTIC_API_PTR.store(0, Ordering::Release);
    stop_global_http_server();
    global_mcp_lifecycle().quiesce_v2(owner)
}

/// Required undecorated C ABI v2 export (`SimoderModule_DrainV2`) confirming no in-flight MCP tool calls remain.
// SAFETY: Required undecorated C ABI v2 export symbol (`Simoder-SDK/Native/native.h`) invoked by `Simoder` to check drain completion.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn SimoderModule_DrainV2(owner: SdHandle) -> SdResult {
    HOST_SEMANTIC_API_PTR.store(0, Ordering::Release);
    stop_global_http_server();
    global_mcp_lifecycle().drain_v2(owner)
}
