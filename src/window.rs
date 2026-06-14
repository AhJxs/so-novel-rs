//! OS 窗口原生化定制（Windows 11 DWM 圆角 + 暗色标题栏）。
//!
//! 其它平台走 stub：什么都不做；egui 用默认外观。
//! 圆角 + 暗色标题栏是 Windows 11 才支持的属性，旧版本 Windows 上调用是 no-op。
//!
//! 本模块是 crate 内唯一允许 unsafe 的位置（DWM 是 FFI）——
#![allow(unsafe_code)]

#[cfg(target_os = "windows")]
pub mod platform {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND,
    };

    /// 把 OS 窗口设为 Windows 11 圆角 + 沉浸式暗色标题栏。
    ///
    /// - `hwnd`：从 `frame.window_handle()` 拿到的 Win32 HWND（isize）。
    /// - `dark_mode`：当前主题是否暗色（影响标题栏配色）。
    ///
    /// 调用失败时（如旧版 Windows 不支持）静默忽略，不影响应用运行。
    pub fn apply_windows11_chrome(hwnd: isize, dark_mode: bool) {
        let hwnd_ptr = hwnd as HWND;

        // 1. 窗口圆角（DWMWCP_ROUND = 系统默认圆角半径，i32）
        let corner: i32 = DWMWCP_ROUND;
        let hr = unsafe {
            DwmSetWindowAttribute(
                hwnd_ptr,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                &corner as *const i32 as *const core::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            )
        };
        if hr < 0 {
            tracing::debug!("DWM 圆角设置失败（旧 Windows 不支持？）: hr=0x{hr:x}");
        }

        // 2. 沉浸式暗色标题栏（Win 11 22621+ 支持，旧版忽略）
        let dark: i32 = if dark_mode { 1 } else { 0 };
        let hr = unsafe {
            DwmSetWindowAttribute(
                hwnd_ptr,
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                &dark as *const i32 as *const core::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            )
        };
        if hr < 0 {
            tracing::debug!("DWM 沉浸式暗色标题栏设置失败: hr=0x{hr:x}");
        }
    }

    /// 从 eframe::Frame 取 HWND（Windows）。
    pub fn extract_hwnd(frame: &eframe::Frame) -> Option<isize> {
        use raw_window_handle::HasWindowHandle;
        let handle = frame.window_handle().ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
            _ => None,
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub mod platform {
    /// 非 Windows 平台：no-op。
    pub fn apply_windows11_chrome(_hwnd: isize, _dark_mode: bool) {}
    pub fn extract_hwnd(_frame: &eframe::Frame) -> Option<isize> {
        None
    }
}
