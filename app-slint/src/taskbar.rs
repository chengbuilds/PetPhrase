//! 窗口进不进任务栏。
//!
//! winit 给每个无 owner 窗口恒打 `WS_EX_APPWINDOW`,且每次显隐都按自己的标志位整体重写
//! `GWL_EXSTYLE`;它的 `set_skip_taskbar` 只是一次性 `ITaskbarList::DeleteTab`,窗口一重显
//! 按钮就回来(宠物/面板误进任务栏的根因)。这里子类化窗口过程拦 `WM_STYLECHANGING`,
//! 把扩展样式钉成策略要的样子:winit 任何路径重写样式都绕不过去。

use slint::winit_030::winit;
use windows_sys::core::w;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GetPropW, GetWindowLongPtrW, RemovePropW, SetPropW, SetWindowLongPtrW,
    GWLP_WNDPROC, GWL_EXSTYLE, STYLESTRUCT, WM_NCDESTROY, WM_STYLECHANGING, WNDPROC,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

const PROP_ORIG_PROC: windows_sys::core::PCWSTR = w!("PetPhrase.OrigWndProc");
const PROP_TOOL: windows_sys::core::PCWSTR = w!("PetPhrase.ToolWindow");

/// 工具窗口:不进任务栏、不进 Alt+Tab;否则强制有任务栏按钮
fn policy(ex: u32, tool: bool) -> u32 {
    if tool {
        (ex | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW
    } else {
        (ex | WS_EX_APPWINDOW) & !WS_EX_TOOLWINDOW
    }
}

unsafe extern "system" fn policy_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let orig = GetPropW(hwnd, PROP_ORIG_PROC);
    if msg == WM_STYLECHANGING && wp as i32 == GWL_EXSTYLE {
        // SAFETY: WM_STYLECHANGING 的 lParam 按约定指向可写 STYLESTRUCT
        let ss = &mut *(lp as *mut STYLESTRUCT);
        ss.styleNew = policy(ss.styleNew, !GetPropW(hwnd, PROP_TOOL).is_null());
    } else if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, orig as isize);
        RemovePropW(hwnd, PROP_ORIG_PROC);
        RemovePropW(hwnd, PROP_TOOL);
    }
    // SAFETY: orig 是子类化前的窗口过程,与 WNDPROC 同布局
    let orig: WNDPROC = std::mem::transmute(orig);
    CallWindowProcW(orig, hwnd, msg, wp, lp)
}

/// 设定窗口是否有任务栏按钮;可反复调用(首次调用时装上样式拦截)
pub fn set_on_taskbar(w: &winit::window::Window, on: bool) {
    use winit::platform::windows::WindowExtWindows;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = w.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    let hwnd = h.hwnd.get() as HWND;
    // SAFETY: hwnd 来自存活中的 winit 窗口,且只在 UI 线程调用
    unsafe {
        if GetPropW(hwnd, PROP_ORIG_PROC).is_null() {
            let proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = policy_proc;
            let orig = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, proc as usize as isize);
            SetPropW(hwnd, PROP_ORIG_PROC, orig as _);
        }
        SetPropW(
            hwnd,
            PROP_TOOL,
            if on { std::ptr::null_mut() } else { 1 as _ },
        );
        // 原样写回,经 WM_STYLECHANGING 套上新策略
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, GetWindowLongPtrW(hwnd, GWL_EXSTYLE));
    }
    // 样式只管之后的显隐;当前已显示的按钮靠 ITaskbarList 立即增删
    w.set_skip_taskbar(!on);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_tool_strips_appwindow_and_back() {
        let winit_default = WS_EX_APPWINDOW | 0x108; // winit 常态:APPWINDOW|WINDOWEDGE|TOPMOST
        let tool = policy(winit_default, true);
        assert_eq!(tool & WS_EX_APPWINDOW, 0);
        assert_ne!(tool & WS_EX_TOOLWINDOW, 0);
        assert_eq!(tool & 0x108, 0x108, "其它位不动");
        let app = policy(tool, false);
        assert_eq!(app, winit_default);
    }
}
