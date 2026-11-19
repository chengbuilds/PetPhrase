//! 点短语后自动粘贴回原窗口(设置里的开关,默认关)。
//!
//! 点宠/面板会把前台抢到本进程,点击时再问「谁是前台」已经晚了。所以用前台切换钩子
//! 一直记着最近一个别家进程的前台窗口;粘贴 = 把它切回前台 + 合成 Ctrl+V。
//! 文字此前已进剪贴板,粘贴失败用户仍能手动粘。

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_CONTROL, VK_V,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, SetForegroundWindow, EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS,
};

/// 最近一个别家前台窗口的 HWND;0 = 没有可粘的目标
static TARGET: AtomicIsize = AtomicIsize::new(0);
/// 钩子装失败时粘贴报这个原因,而不是笼统的「没找到窗口」
static TRACKING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq)]
enum Seen {
    /// 普通应用窗口:记为粘贴目标
    Target,
    /// 任务栏/托盘溢出区:点托盘图标开面板会先经过它,不能顶掉真正的目标
    Pass,
    /// 桌面:用户主动离开了应用,清空目标,免得粘进更早的窗口
    Clear,
}

fn classify(class: &str) -> Seen {
    match class {
        "Shell_TrayWnd"
        | "Shell_SecondaryTrayWnd"
        | "NotifyIconOverflowWindow"
        | "TopLevelWindowForOverflowXamlIsland" => Seen::Pass,
        "Progman" | "WorkerW" => Seen::Clear,
        _ => Seen::Target,
    }
}

fn remember(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let mut buf = [0u16; 64];
    // SAFETY: buf 可写、长度如实传入
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    match classify(&String::from_utf16_lossy(&buf[..n.max(0) as usize])) {
        Seen::Target => TARGET.store(hwnd as isize, Ordering::Relaxed),
        Seen::Pass => {}
        Seen::Clear => TARGET.store(0, Ordering::Relaxed),
    }
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _obj: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    remember(hwnd);
}

/// 开始跟踪前台窗口。须在 UI 线程(有消息循环)调用:OUTOFCONTEXT 回调靠本线程消息队列投递
pub fn track_foreground() {
    // SAFETY: 回调是 'static 函数;钩子随进程存活,退出时系统回收
    unsafe {
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        if hook.is_null() {
            return;
        }
        TRACKING.store(true, Ordering::Relaxed);
        // 启动时的前台若是别家窗口也记上(钩子只报之后的切换)
        let fg = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, &mut pid);
        if pid != std::process::id() {
            remember(fg);
        }
    }
}

fn key(vk: VIRTUAL_KEY, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// 切回最近的别家前台窗口并发 Ctrl+V。调用时本进程须仍有前台权(刚处理完用户点击即满足)
pub fn paste() -> Result<(), &'static str> {
    if !TRACKING.load(Ordering::Relaxed) {
        return Err("前台窗口钩子没装上");
    }
    let hwnd = TARGET.load(Ordering::Relaxed) as HWND;
    // SAFETY: 只读查询 + 切前台 + 合成按键,hwnd 失效时各 API 返回失败而非 UB
    unsafe {
        if hwnd.is_null() || IsWindow(hwnd) == 0 || IsWindowVisible(hwnd) == 0 {
            return Err("没找到刚才的输入窗口");
        }
        if IsIconic(hwnd) != 0 {
            return Err("刚才的窗口已最小化");
        }
        if SetForegroundWindow(hwnd) == 0 {
            return Err("切不回刚才的窗口");
        }
        let inputs = [
            key(VK_CONTROL, 0),
            key(VK_V, 0),
            key(VK_V, KEYEVENTF_KEYUP),
            key(VK_CONTROL, KEYEVENTF_KEYUP),
        ];
        // 目标以管理员身份运行时 UIPI 拦截,SendInput 返回少于条数
        if SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        ) != inputs.len() as u32
        {
            return Err("对方窗口拒收按键(可能以管理员身份运行)");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_windows_do_not_become_targets() {
        assert_eq!(classify("Shell_TrayWnd"), Seen::Pass);
        assert_eq!(classify("TopLevelWindowForOverflowXamlIsland"), Seen::Pass);
        assert_eq!(classify("Progman"), Seen::Clear);
        assert_eq!(classify("WorkerW"), Seen::Clear);
        assert_eq!(classify("Notepad"), Seen::Target);
        assert_eq!(classify("Chrome_WidgetWin_1"), Seen::Target);
    }
}
