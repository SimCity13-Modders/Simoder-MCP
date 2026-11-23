#![allow(unsafe_code)]

//! Safe RAII-guarded Win32 inspection and window-capture primitives for [`super::LiveHostBackend`].
//!
//! Operates in-process (`x86` and `x86_64` compatible) inside `SimCity.exe` using `kernel32`, `user32`,
//! and `gdi32` entrypoints with compile-time ABI struct size assertions and deterministic handle cleanup.

use std::ffi::c_void;
use std::io::Cursor;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

type Handle = *mut c_void;
type Hwnd = *mut c_void;
type Hdc = *mut c_void;
type Hgdiobj = *mut c_void;
type Hbitmap = *mut c_void;

const INVALID_HANDLE_VALUE_ADDR: usize = !0usize;
const TH32CS_SNAPMODULE: u32 = 0x0000_0008;
const TH32CS_SNAPMODULE32: u32 = 0x0000_0010;
const MEM_COMMIT: u32 = 0x1000;
const PAGE_NOACCESS: u32 = 0x01;
const PAGE_GUARD: u32 = 0x100;
const DIB_RGB_COLORS: u32 = 0;
const BI_RGB: u32 = 0;
const SRCCOPY: u32 = 0x00CC_0020;
const PW_RENDERFULLCONTENT: u32 = 2;
const WM_CLOSE: u32 = 0x0010;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const MK_LBUTTON: usize = 0x0001;
const DEV_CONSOLE_TITLE: &str = "Simoder Developer Console";

/// Virtual-key constants mapped from allowlisted MCP keys and building rotation shortcuts.
pub const VK_RETURN: u16 = 0x0D;
/// Escape virtual key.
pub const VK_ESCAPE: u16 = 0x1B;
/// Spacebar virtual key.
pub const VK_SPACE: u16 = 0x20;
/// Comma key (`','` — rotate building counter-clockwise in SimCity).
pub const VK_OEM_COMMA: u16 = 0xBC;
/// Period key (`'.'` — rotate building clockwise in SimCity).
pub const VK_OEM_PERIOD: u16 = 0xBE;

/// Metadata for a single loaded DLL/EXE module observed in the target game process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveModuleInfo {
    /// Module filename (for example `"SimCity.exe"`, `"d3d9.dll"`, `"simoder_mcp.dll"`).
    pub name: String,
    /// Full Win32 filesystem path to the loaded module.
    pub path: String,
    /// Runtime virtual base address in process address space.
    pub base_address: usize,
    /// Mapped image size in bytes.
    pub image_size: u32,
}

/// Geometry and metadata for a visible top-level window owned by the target game process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveWindowInfo {
    /// Raw Win32 `HWND` numeric value.
    pub handle: usize,
    /// Owning process identifier.
    pub process_id: u32,
    /// Window title text.
    pub title: String,
    /// Outer window left screen coordinate.
    pub left: i32,
    /// Outer window top screen coordinate.
    pub top: i32,
    /// Outer window width in pixels.
    pub width: i32,
    /// Outer window height in pixels.
    pub height: i32,
    /// Client area screen left coordinate.
    pub client_left: i32,
    /// Client area screen top coordinate.
    pub client_top: i32,
    /// Client area width in pixels.
    pub client_width: i32,
    /// Client area height in pixels.
    pub client_height: i32,
    /// True when this window currently holds foreground focus.
    pub is_foreground: bool,
}

#[repr(C)]
struct ModuleEntry32W {
    dw_size: u32,
    th32_module_id: u32,
    th32_process_id: u32,
    glbl_cnt_usage: u32,
    proc_cnt_usage: u32,
    mod_base_addr: *mut u8,
    mod_base_size: u32,
    h_module: Handle,
    sz_module: [u16; 256],
    sz_exe_path: [u16; 260],
}

#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<ModuleEntry32W>() == 1064);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<ModuleEntry32W>() == 1080);

#[repr(C)]
struct MemoryBasicInformation {
    base_address: usize,
    allocation_base: usize,
    allocation_protect: u32,
    #[cfg(target_pointer_width = "64")]
    partition_id: u32,
    region_size: usize,
    state: u32,
    protect: u32,
    type_: u32,
}

#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<MemoryBasicInformation>() == 28);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<MemoryBasicInformation>() == 48);

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_pels_per_meter: i32,
    bi_y_pels_per_meter: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

const _: () = assert!(size_of::<BitmapInfoHeader>() == 40);

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct BitmapInfo {
    bmi_header: BitmapInfoHeader,
    bmi_colors: [u32; 1],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateToolhelp32Snapshot(dw_flags: u32, th32_process_id: u32) -> Handle;
    fn Module32FirstW(h_snapshot: Handle, lpme: *mut ModuleEntry32W) -> i32;
    fn Module32NextW(h_snapshot: Handle, lpme: *mut ModuleEntry32W) -> i32;
    fn CloseHandle(h_object: Handle) -> i32;
    fn GetCurrentProcess() -> Handle;
    fn ReadProcessMemory(
        h_process: Handle,
        lp_base_address: *const c_void,
        lp_buffer: *mut c_void,
        n_size: usize,
        lp_number_of_bytes_read: *mut usize,
    ) -> i32;
    fn VirtualQuery(
        lp_address: *const c_void,
        lp_buffer: *mut MemoryBasicInformation,
        dw_length: usize,
    ) -> usize;
    fn GetLastError() -> u32;
}

type EnumWindowsProc = unsafe extern "system" fn(hwnd: Hwnd, lparam: isize) -> i32;

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(lp_enum_func: EnumWindowsProc, lparam: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, lpdw_process_id: *mut u32) -> u32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn GetWindowRect(hwnd: Hwnd, lp_rect: *mut Rect) -> i32;
    fn GetClientRect(hwnd: Hwnd, lp_rect: *mut Rect) -> i32;
    fn ClientToScreen(hwnd: Hwnd, lp_point: *mut Point) -> i32;
    fn GetWindowTextW(hwnd: Hwnd, lp_string: *mut u16, n_max_count: i32) -> i32;
    fn GetForegroundWindow() -> Hwnd;
    fn GetWindowDC(hwnd: Hwnd) -> Hdc;
    fn ReleaseDC(hwnd: Hwnd, hdc: Hdc) -> i32;
    fn PrintWindow(hwnd: Hwnd, hdc_blt: Hdc, n_flags: u32) -> i32;
    fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
    fn DeleteDC(hdc: Hdc) -> i32;
    fn CreateDIBSection(
        hdc: Hdc,
        pbmi: *const BitmapInfo,
        usage: u32,
        ppv_bits: *mut *mut c_void,
        h_section: Handle,
        offset: u32,
    ) -> Hbitmap;
    fn SelectObject(hdc: Hdc, h: Hgdiobj) -> Hgdiobj;
    fn DeleteObject(ho: Hgdiobj) -> i32;
    fn BitBlt(
        hdc: Hdc,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        hdc_src: Hdc,
        x1: i32,
        y1: i32,
        rop: u32,
    ) -> i32;
    fn GdiFlush() -> i32;
}

struct OwnedHandle(Handle);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 as usize != INVALID_HANDLE_VALUE_ADDR {
            // SAFETY: Valid Win32 handle owned by this guard.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

struct WindowDcGuard(Hwnd, Hdc);
impl Drop for WindowDcGuard {
    fn drop(&mut self) {
        if !self.1.is_null() {
            // SAFETY: `GetWindowDC` handle paired with its exact window handle.
            unsafe {
                let _ = ReleaseDC(self.0, self.1);
            }
        }
    }
}

struct MemDcGuard(Hdc);
impl Drop for MemDcGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: Memory DC created via `CreateCompatibleDC`.
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
}

struct GdiObjectGuard(Hgdiobj);
impl Drop for GdiObjectGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: GDI HBITMAP created via `CreateDIBSection` and unselected before drop.
            unsafe {
                let _ = DeleteObject(self.0);
            }
        }
    }
}

/// Enumerate all loaded PE modules in `pid` via Win32 Toolhelp32 snapshots.
pub fn enumerate_process_modules(pid: u32) -> Result<Vec<LiveModuleInfo>, String> {
    // SAFETY: Standard Win32 module snapshot for `pid`.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
    if raw.is_null() || raw as usize == INVALID_HANDLE_VALUE_ADDR {
        // SAFETY: Query immediately after failure.
        let err = unsafe { GetLastError() };
        return Err(format!("CreateToolhelp32Snapshot failed (win32={err})"));
    }
    let snapshot = OwnedHandle(raw);

    // SAFETY: Properly sized `ModuleEntry32W` initialized with zeroed memory + `dw_size`.
    let mut entry: ModuleEntry32W = unsafe { zeroed() };
    entry.dw_size = size_of::<ModuleEntry32W>() as u32;

    let mut modules = Vec::new();
    // SAFETY: Valid snapshot handle and `ModuleEntry32W`.
    if unsafe { Module32FirstW(snapshot.0, &mut entry) } == 0 {
        return Ok(modules);
    }

    loop {
        modules.push(LiveModuleInfo {
            name: utf16_z_to_string(&entry.sz_module),
            path: utf16_z_to_string(&entry.sz_exe_path),
            base_address: entry.mod_base_addr as usize,
            image_size: entry.mod_base_size,
        });
        // SAFETY: Advances iterator across remaining Toolhelp32 modules.
        if unsafe { Module32NextW(snapshot.0, &mut entry) } == 0 {
            break;
        }
    }
    Ok(modules)
}

/// Safely verify readable committed memory pages via `VirtualQuery` and read bytes with `ReadProcessMemory`.
pub fn read_process_memory_checked(address: usize, length: usize) -> Result<Vec<u8>, String> {
    if address == 0 {
        return Err("cannot read from null virtual address 0x0".into());
    }
    let end = address
        .checked_add(length)
        .ok_or_else(|| "virtual address + length overflows pointer width".to_string())?;

    let mut cursor = address;
    while cursor < end {
        // SAFETY: Valid output `MemoryBasicInformation` passed to `VirtualQuery`.
        let mut mbi: MemoryBasicInformation = unsafe { zeroed() };
        // SAFETY: `VirtualQuery` safely queries page state in the current process without faulting.
        let queried = unsafe {
            VirtualQuery(
                cursor as *const c_void,
                &mut mbi,
                size_of::<MemoryBasicInformation>(),
            )
        };
        if queried == 0 || mbi.region_size == 0 {
            return Err(format!("VirtualQuery failed at 0x{cursor:X}"));
        }
        if mbi.state != MEM_COMMIT || (mbi.protect & (PAGE_NOACCESS | PAGE_GUARD)) != 0 {
            return Err(format!(
                "address range around 0x{cursor:X} is uncommitted or non-readable (state={:#x}, protect={:#x})",
                mbi.state, mbi.protect
            ));
        }
        let region_end = mbi.base_address.saturating_add(mbi.region_size);
        if region_end <= cursor {
            break;
        }
        cursor = region_end;
    }

    let mut buf = vec![0u8; length];
    let mut bytes_read: usize = 0;
    // SAFETY: `buf` has capacity `length` and `ReadProcessMemory` safely copies from `address`.
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const c_void,
            buf.as_mut_ptr() as *mut c_void,
            length,
            &mut bytes_read,
        )
    };
    if ok == 0 || bytes_read != length {
        // SAFETY: Query immediately after failure.
        let err = unsafe { GetLastError() };
        return Err(format!(
            "ReadProcessMemory failed at 0x{address:X} (win32={err}, read={bytes_read}/{length})"
        ));
    }
    Ok(buf)
}

/// Enumerate visible top-level windows belonging to `target_pid`.
pub fn list_process_windows(target_pid: u32) -> Vec<LiveWindowInfo> {
    struct EnumState {
        target_pid: u32,
        fg_hwnd: usize,
        windows: Vec<LiveWindowInfo>,
    }

    unsafe extern "system" fn callback(hwnd: Hwnd, lparam: isize) -> i32 {
        // SAFETY: `lparam` points to `EnumState` on `list_process_windows` stack for the duration of `EnumWindows`.
        let state = unsafe { &mut *(lparam as *mut EnumState) };
        let mut pid: u32 = 0;
        // SAFETY: Valid `HWND` supplied by `EnumWindows`.
        unsafe {
            let _ = GetWindowThreadProcessId(hwnd, &mut pid);
        }
        if pid != state.target_pid {
            return 1;
        }
        // SAFETY: Check window visibility.
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }

        let mut wr = Rect::default();
        // SAFETY: Query outer window rect.
        if unsafe { GetWindowRect(hwnd, &mut wr) } == 0 {
            return 1;
        }
        let width = wr.right - wr.left;
        let height = wr.bottom - wr.top;
        if width <= 0 || height <= 0 {
            return 1;
        }

        let mut cr = Rect::default();
        // SAFETY: Query client rect.
        let _ = unsafe { GetClientRect(hwnd, &mut cr) };
        let mut client_pt = Point { x: 0, y: 0 };
        // SAFETY: Translate client (0,0) to screen coordinates.
        let _ = unsafe { ClientToScreen(hwnd, &mut client_pt) };

        let mut title_buf = [0u16; 256];
        // SAFETY: Read window title up to 255 UTF-16 code units.
        let title_len =
            unsafe { GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32) };
        let title = if title_len > 0 {
            String::from_utf16_lossy(&title_buf[..title_len as usize])
        } else {
            String::new()
        };

        let hwnd_val = hwnd as usize;
        state.windows.push(LiveWindowInfo {
            handle: hwnd_val,
            process_id: pid,
            title,
            left: wr.left,
            top: wr.top,
            width,
            height,
            client_left: client_pt.x,
            client_top: client_pt.y,
            client_width: (cr.right - cr.left).max(1),
            client_height: (cr.bottom - cr.top).max(1),
            is_foreground: hwnd_val == state.fg_hwnd,
        });
        1
    }

    // SAFETY: `GetForegroundWindow` returns current foreground HWND or null.
    let fg = unsafe { GetForegroundWindow() } as usize;
    let mut state = EnumState {
        target_pid,
        fg_hwnd: fg,
        windows: Vec::new(),
    };
    // SAFETY: Stack pointer `&mut state` remains valid until `EnumWindows` returns synchronously.
    unsafe {
        let _ = EnumWindows(callback, &mut state as *mut EnumState as isize);
    }
    state.windows
}

/// Capture the largest visible non-console game window belonging to `pid` into base64-encoded PNG pixels.
pub fn capture_process_window(pid: u32) -> Result<(LiveWindowInfo, String), String> {
    let target = select_primary_window(pid)?;
    let png_base64 = capture_hwnd_png(&target)?;
    Ok((target, png_base64))
}

/// Select the primary visible `SimCity` window owned by `pid` (excluding the developer console).
pub fn select_primary_window(pid: u32) -> Result<LiveWindowInfo, String> {
    let mut windows: Vec<LiveWindowInfo> = list_process_windows(pid)
        .into_iter()
        .filter(|w| !w.title.eq_ignore_ascii_case(DEV_CONSOLE_TITLE))
        .collect();
    if windows.is_empty() {
        windows = list_process_windows(pid);
    }
    if windows.is_empty() {
        return Err(format!("no visible top-level window found for pid {pid}"));
    }
    windows.sort_by_key(|w| {
        let area = (w.width.max(0) as i64) * (w.height.max(0) as i64);
        (w.is_foreground, area)
    });
    Ok(windows.pop().expect("non-empty verified"))
}

fn verify_owned_hwnd(pid: u32, hwnd_val: usize) -> Result<LiveWindowInfo, String> {
    list_process_windows(pid)
        .into_iter()
        .find(|w| w.handle == hwnd_val && !w.title.eq_ignore_ascii_case(DEV_CONSOLE_TITLE))
        .ok_or_else(|| format!("window handle {hwnd_val} is no longer active for pid {pid}"))
}

fn pack_lparam(x: i32, y: i32) -> isize {
    (((y as i16 as u16 as u32) << 16) | (x as i16 as u16 as u32)) as isize
}

fn norm_to_client_coords(win: &LiveWindowInfo, norm_x: f64, norm_y: f64) -> (i32, i32) {
    let max_x = (win.client_width - 1).max(0) as f64;
    let max_y = (win.client_height - 1).max(0) as f64;
    let cx = (norm_x.clamp(0.0, 1.0) * max_x).round() as i32;
    let cy = (norm_y.clamp(0.0, 1.0) * max_y).round() as i32;
    (cx, cy)
}

/// Post a left mouse click at `(norm_x, norm_y)` within the verified client area of `hwnd_val`.
pub fn send_client_click(
    pid: u32,
    hwnd_val: usize,
    norm_x: f64,
    norm_y: f64,
) -> Result<(i32, i32), String> {
    let win = verify_owned_hwnd(pid, hwnd_val)?;
    let (cx, cy) = norm_to_client_coords(&win, norm_x, norm_y);
    let lp = pack_lparam(cx, cy);
    let hwnd = win.handle as Hwnd;
    // SAFETY: Verified live top-level window owned by our PID.
    unsafe {
        let _ = PostMessageW(hwnd, WM_MOUSEMOVE, 0, lp);
        if PostMessageW(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, lp) == 0 {
            return Err("PostMessageW(WM_LBUTTONDOWN) failed".into());
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(40));
    // SAFETY: Paired mouse-up on verified HWND.
    unsafe {
        let _ = PostMessageW(hwnd, WM_LBUTTONUP, 0, lp);
    }
    Ok((cx, cy))
}

/// Post a smooth interpolated left-button drag across the verified client area (`steps` in `4..=40`).
#[allow(clippy::too_many_arguments)]
/// Start and end client coordinate pair for mouse drag actions.
pub type DragCoordinates = ((i32, i32), (i32, i32));

pub fn send_client_drag(
    pid: u32,
    hwnd_val: usize,
    start_x: f64,
    start_y: f64,
    end_x: f64,
    end_y: f64,
    steps: u32,
) -> Result<DragCoordinates, String> {
    let win = verify_owned_hwnd(pid, hwnd_val)?;
    let (sx, sy) = norm_to_client_coords(&win, start_x, start_y);
    let (ex, ey) = norm_to_client_coords(&win, end_x, end_y);
    let steps = steps.clamp(4, 40);
    let hwnd = win.handle as Hwnd;
    let start_lp = pack_lparam(sx, sy);

    // SAFETY: Verified live top-level window owned by our PID.
    unsafe {
        let _ = PostMessageW(hwnd, WM_MOUSEMOVE, 0, start_lp);
        std::thread::sleep(std::time::Duration::from_millis(25));
        if PostMessageW(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, start_lp) == 0 {
            return Err("PostMessageW(WM_LBUTTONDOWN) failed".into());
        }
    }

    for idx in 1..=steps {
        let t = (idx as f64) / (steps as f64);
        let mx = ((sx as f64) + ((ex - sx) as f64) * t).round() as i32;
        let my = ((sy as f64) + ((ey - sy) as f64) * t).round() as i32;
        let lp = pack_lparam(mx, my);
        // SAFETY: Interpolated drag move with `MK_LBUTTON`.
        unsafe {
            let _ = PostMessageW(hwnd, WM_MOUSEMOVE, MK_LBUTTON, lp);
        }
        std::thread::sleep(std::time::Duration::from_millis(18));
    }

    let end_lp = pack_lparam(ex, ey);
    // SAFETY: Paired mouse-up on verified HWND.
    unsafe {
        let _ = PostMessageW(hwnd, WM_LBUTTONUP, 0, end_lp);
    }
    Ok(((sx, sy), (ex, ey)))
}

/// Post a key press/release (`WM_KEYDOWN` + `WM_KEYUP`) for virtual key `vk` to the verified game window.
pub fn send_window_key(pid: u32, hwnd_val: usize, vk: u16) -> Result<(), String> {
    let win = verify_owned_hwnd(pid, hwnd_val)?;
    let hwnd = win.handle as Hwnd;
    // SAFETY: Verified live top-level window owned by our PID.
    unsafe {
        if PostMessageW(hwnd, WM_KEYDOWN, usize::from(vk), 0) == 0 {
            return Err("PostMessageW(WM_KEYDOWN) failed".into());
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(25));
    // SAFETY: Paired key-up on verified HWND.
    unsafe {
        let _ = PostMessageW(hwnd, WM_KEYUP, usize::from(vk), 0);
    }
    Ok(())
}

fn capture_hwnd_png(win: &LiveWindowInfo) -> Result<String, String> {
    let hwnd = win.handle as Hwnd;
    let w = win.width.clamp(1, 8192);
    let h = win.height.clamp(1, 8192);

    // SAFETY: Acquire window DC paired with `WindowDcGuard`.
    let hdc_win = unsafe { GetWindowDC(hwnd) };
    if hdc_win.is_null() {
        return Err("GetWindowDC returned null".into());
    }
    let _win_guard = WindowDcGuard(hwnd, hdc_win);

    // SAFETY: Create compatible memory DC paired with `MemDcGuard`.
    let hdc_mem = unsafe { CreateCompatibleDC(hdc_win) };
    if hdc_mem.is_null() {
        return Err("CreateCompatibleDC returned null".into());
    }
    let _mem_guard = MemDcGuard(hdc_mem);

    let bmi = BitmapInfo {
        bmi_header: BitmapInfoHeader {
            bi_size: size_of::<BitmapInfoHeader>() as u32,
            bi_width: w,
            bi_height: -h, // Negative height = top-down DIB origin at top-left.
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: BI_RGB,
            ..Default::default()
        },
        bmi_colors: [0],
    };

    let mut bits_ptr: *mut c_void = null_mut();
    // SAFETY: Allocates 32-bit top-down DIB buffer valid until `hbm` is deleted.
    let hbm = unsafe {
        CreateDIBSection(
            hdc_win,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            null_mut(),
            0,
        )
    };
    if hbm.is_null() || bits_ptr.is_null() {
        return Err("CreateDIBSection failed".into());
    }
    let _bm_guard = GdiObjectGuard(hbm as Hgdiobj);

    // SAFETY: Select DIB into compatible memory DC, restore old bitmap before deleting `_bm_guard`.
    let prev_obj = unsafe { SelectObject(hdc_mem, hbm as Hgdiobj) };
    // SAFETY: Attempt full rendered capture (`PW_RENDERFULLCONTENT`), then basic `PrintWindow`, then `BitBlt`.
    let printed = unsafe {
        PrintWindow(hwnd, hdc_mem, PW_RENDERFULLCONTENT) != 0
            || PrintWindow(hwnd, hdc_mem, 0) != 0
            || BitBlt(hdc_mem, 0, 0, w, h, hdc_win, 0, 0, SRCCOPY) != 0
    };
    // SAFETY: Flush GDI drawing commands and unselect our bitmap.
    unsafe {
        let _ = GdiFlush();
        let _ = SelectObject(hdc_mem, prev_obj);
    }
    if !printed {
        return Err("PrintWindow and BitBlt both failed capturing game window".into());
    }

    let pixel_count = (w as usize) * (h as usize);
    // SAFETY: `bits_ptr` points to `w * h * 4` bytes owned by live `hbm` until `_bm_guard` drops.
    let bgra = unsafe { std::slice::from_raw_parts(bits_ptr as *const u8, pixel_count * 4) };
    let mut rgb = Vec::with_capacity(pixel_count * 3);
    let (chunks, _) = bgra.as_chunks::<4>();
    for px in chunks {
        rgb.push(px[2]);
        rgb.push(px[1]);
        rgb.push(px[0]);
    }

    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut png_bytes), w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("PNG header encode error: {e}"))?;
        writer
            .write_image_data(&rgb)
            .map_err(|e| format!("PNG pixel encode error: {e}"))?;
    }

    Ok(BASE64_STANDARD.encode(&png_bytes))
}

/// Request graceful closure of a window via `WM_CLOSE`.
pub fn post_close_window(hwnd: usize) -> bool {
    if hwnd == 0 {
        return false;
    }
    // SAFETY: Posts nonblocking `WM_CLOSE` message to top-level process window.
    unsafe { PostMessageW(hwnd as Hwnd, WM_CLOSE, 0, 0) != 0 }
}

fn utf16_z_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
