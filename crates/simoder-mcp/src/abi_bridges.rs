#![allow(unsafe_code)]

//! Host-bound FFI bridge builders connecting `simoder_mcp.dll` to `Simoder`'s
//! `SdSemanticHostApiV2` (`v1..=v3`) function table.

use std::mem::size_of;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::abi::{HOST_SEMANTIC_API_PTR, SdHeader, SdResult, SdSemanticHostApiV2};

fn emit_bridge_warn(log_fn: &Option<crate::host::HostLogFn>, message: &str) {
    if let Some(log) = log_fn {
        log("warn", "simoder-mcp.abi", message);
    }
}

/// Build a safe host-bound [`crate::host::UiJsEvalFn`] closure when `Simoder` provided a valid
/// `SdSemanticHostApiV2` pointer in `SimoderModule_PrepareV2`.
pub(crate) fn host_eval_ui_js_bridge() -> Option<crate::host::UiJsEvalFn> {
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

/// Build a host-bound [`crate::host::HostLogFn`] forwarding MCP logs to `Simoder` (`version >= 2`).
pub(crate) fn host_log_bridge() -> Option<crate::host::HostLogFn> {
    let raw_ptr = HOST_SEMANTIC_API_PTR.load(Ordering::Acquire);
    if raw_ptr == 0 {
        return None;
    }
    // SAFETY: `Simoder` keeps `SEMANTIC_HOST_API_V2` static for the process lifetime while this DLL is admitted.
    let size_and_ver = unsafe { *(raw_ptr as *const SdHeader) };
    const V2_MIN_SIZE: usize =
        std::mem::offset_of!(SdSemanticHostApiV2, log_message) + size_of::<usize>();
    if (size_and_ver.size as usize) < V2_MIN_SIZE || size_and_ver.version < 2 {
        return None;
    }
    // SAFETY: `size_and_ver.size >= V2_MIN_SIZE` guarantees `log_message` lies within the host struct.
    let log_fn = unsafe {
        std::ptr::addr_of!((*(raw_ptr as *const SdSemanticHostApiV2)).log_message).read()
    };
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

/// Build a host-bound [`crate::host::HostExternalToolsBridge`] (`version >= 3`).
pub(crate) fn host_external_tools_bridge(
    log_fn: Option<crate::host::HostLogFn>,
) -> Option<crate::host::HostExternalToolsBridge> {
    let raw_ptr = HOST_SEMANTIC_API_PTR.load(Ordering::Acquire);
    if raw_ptr == 0 {
        return None;
    }
    // SAFETY: `Simoder` keeps `SEMANTIC_HOST_API_V2` static for the process lifetime while this DLL is admitted.
    let size_and_ver = unsafe { *(raw_ptr as *const SdHeader) };
    const V3_MIN_SIZE: usize =
        std::mem::offset_of!(SdSemanticHostApiV2, invoke_mod_tool) + size_of::<usize>();
    if (size_and_ver.size as usize) < V3_MIN_SIZE || size_and_ver.version < 3 {
        return None;
    }
    // SAFETY: `size_and_ver.size >= V3_MIN_SIZE` guarantees `list_mod_tools` and `invoke_mod_tool` are present.
    let (list_fn, invoke_fn) = unsafe {
        let api = raw_ptr as *const SdSemanticHostApiV2;
        (
            std::ptr::addr_of!((*api).list_mod_tools).read(),
            std::ptr::addr_of!((*api).invoke_mod_tool).read(),
        )
    };

    let list_log = log_fn.clone();
    let list_tools: crate::host::HostExternalToolsListFn = Arc::new(move || {
        let mut cap = 64 * 1024;
        let mut buf = vec![0u8; cap];
        let mut out_len: usize = 0;
        let mut code = (list_fn)(buf.as_mut_ptr(), cap, &mut out_len);
        if code == SdResult::BufferSmall as u32 && out_len > cap && out_len <= 256 * 1024 {
            cap = out_len;
            buf.resize(cap, 0);
            code = (list_fn)(buf.as_mut_ptr(), cap, &mut out_len);
        }
        if code != SdResult::Ok as u32 {
            emit_bridge_warn(
                &list_log,
                &format!("host list_mod_tools failed (SdResult={code})"),
            );
            return Vec::new();
        }
        let actual_len = out_len.min(buf.len());
        match serde_json::from_slice::<Vec<crate::host::ExternalModTool>>(&buf[..actual_len]) {
            Ok(tools) => tools,
            Err(err) => {
                emit_bridge_warn(
                    &list_log,
                    &format!("host list_mod_tools returned invalid JSON: {err}"),
                );
                Vec::new()
            }
        }
    });

    let invoke_tool: crate::host::HostExternalToolInvokeFn = Arc::new(move |name, args, granted| {
        let args_str = serde_json::to_string(args).map_err(|e| {
            crate::types::McpToolError::public(format!("failed serializing tool args: {e}"))
        })?;
        let grants_str = serde_json::to_string(granted).map_err(|e| {
            crate::types::McpToolError::public(format!("failed serializing tool grants: {e}"))
        })?;
        let mut cap = 64 * 1024;
        let mut buf = vec![0u8; cap];
        let mut out_len: usize = 0;
        let mut code = (invoke_fn)(
            name.as_ptr(),
            name.len(),
            args_str.as_ptr(),
            args_str.len(),
            grants_str.as_ptr(),
            grants_str.len(),
            buf.as_mut_ptr(),
            cap,
            &mut out_len,
        );
        if code == SdResult::BufferSmall as u32 && out_len > cap && out_len <= 256 * 1024 {
            cap = out_len;
            buf.resize(cap, 0);
            code = (invoke_fn)(
                name.as_ptr(),
                name.len(),
                args_str.as_ptr(),
                args_str.len(),
                grants_str.as_ptr(),
                grants_str.len(),
                buf.as_mut_ptr(),
                cap,
                &mut out_len,
            );
        }
        let actual_len = out_len.min(buf.len());
        if code != SdResult::Ok as u32 {
            let err_text = String::from_utf8_lossy(&buf[..actual_len]).into_owned();
            let detail = if err_text.trim().is_empty() {
                format!("Host mod tool '{name}' failed (SdResult={code})")
            } else {
                err_text
            };
            return Err(crate::types::McpToolError::public(detail));
        }
        serde_json::from_slice::<serde_json::Value>(&buf[..actual_len]).map_err(|e| {
            crate::types::McpToolError::public(format!(
                "Host mod tool '{name}' returned invalid JSON ({e})"
            ))
        })
    });

    Some(crate::host::HostExternalToolsBridge {
        list_tools,
        invoke_tool,
    })
}
