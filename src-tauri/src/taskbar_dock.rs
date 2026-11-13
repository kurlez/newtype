//! Dock the indicator in the empty taskbar strip beside the system tray.
//!
//! Windows 11 has no DeskBand API, and a child parented into `Shell_TrayWnd`
//! sits behind the XAML island. The window is a topmost tool popup
//! (`WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE`) laid over the taskbar,
//! flush with its edge and exactly as thick as the bar. If that strip cannot be
//! placed, the caller keeps the floating panel. This module does not draw a tray icon.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarEdge {
    Left,
    Top,
    Right,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockFit {
    /// Place the window here, in physical pixels.
    Slot(PxRect),
    /// The taskbar is autohidden and currently collapsed.
    Collapsed,
    /// Not enough empty space beside the tray. Use the floating panel.
    NoRoom,
}

pub struct DockQuery {
    pub taskbar: PxRect,
    pub edge: BarEdge,
    /// Horizontal bars: the tray's left edge. Vertical bars: the tray's top edge.
    pub tray_start: i32,
    /// Horizontal bars: the right edge of the app buttons. Vertical: their bottom.
    pub icons_end: Option<i32>,
    pub desired: i32,
    pub minimum: i32,
    pub gap: i32,
}

impl BarEdge {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Top => "top",
            Self::Right => "right",
            Self::Bottom => "bottom",
        }
    }
}

pub fn edge_from_appbar(edge: u32) -> BarEdge {
    match edge {
        0 => BarEdge::Left,
        1 => BarEdge::Top,
        2 => BarEdge::Right,
        _ => BarEdge::Bottom,
    }
}

/// Move `outer` so its visible frame lands on `requested`.
///
/// `visible` is the on-screen frame (DWM extended bounds). It can be inset
/// from the Win32 outer rect. The returned rect is the next outer rect.
pub fn adjusted_outer(requested: PxRect, visible: PxRect, outer: PxRect) -> PxRect {
    PxRect {
        x: outer.x + (requested.x - visible.x),
        y: outer.y + (requested.y - visible.y),
        width: (outer.width + (requested.width - visible.width)).max(1),
        height: (outer.height + (requested.height - visible.height)).max(1),
    }
}

/// Pick a notification-area rect from descendant windows when `TrayNotifyWnd`
/// itself is missing. The XAML island that covers the whole bar is ignored.
pub fn tray_from_children(taskbar: PxRect, edge: BarEdge, children: &[PxRect]) -> Option<PxRect> {
    let (origin, span) = match edge {
        BarEdge::Left | BarEdge::Right => (taskbar.y, taskbar.height),
        BarEdge::Top | BarEdge::Bottom => (taskbar.x, taskbar.width),
    };
    if span <= 0 {
        return None;
    }
    let trailing = origin.saturating_add(span);
    let bar_thickness = match edge {
        BarEdge::Left | BarEdge::Right => taskbar.width,
        BarEdge::Top | BarEdge::Bottom => taskbar.height,
    };
    let mut best: Option<PxRect> = None;
    for child in children {
        if !rects_overlap(taskbar, *child) {
            continue;
        }
        let (end, length, thickness) = match edge {
            BarEdge::Left | BarEdge::Right => {
                (child.y.saturating_add(child.height), child.height, child.width)
            }
            BarEdge::Top | BarEdge::Bottom => {
                (child.x.saturating_add(child.width), child.width, child.height)
            }
        };
        if length <= 0 || thickness <= 0 || thickness > bar_thickness.saturating_add(8) {
            continue;
        }
        if (end - trailing).abs() > 8 {
            continue;
        }
        if length.saturating_mul(10) >= span.saturating_mul(7) {
            continue;
        }
        let replace = match best {
            None => true,
            Some(current) => length > long_side(edge, current),
        };
        if replace {
            best = Some(*child);
        }
    }
    best
}

fn long_side(edge: BarEdge, rect: PxRect) -> i32 {
    match edge {
        BarEdge::Left | BarEdge::Right => rect.height,
        BarEdge::Top | BarEdge::Bottom => rect.width,
    }
}

fn rects_overlap(a: PxRect, b: PxRect) -> bool {
    a.width > 0
        && a.height > 0
        && b.width > 0
        && b.height > 0
        && a.x < b.x.saturating_add(b.width)
        && b.x < a.x.saturating_add(a.width)
        && a.y < b.y.saturating_add(b.height)
        && b.y < a.y.saturating_add(a.height)
}

/// Width and minimum width for a taskbar of `thickness` physical pixels.
/// 48px is the usual 100% Windows 11 taskbar.
pub fn dock_sizes(thickness: i32) -> (i32, i32) {
    let scale = thickness.max(1) as f64 / 48.0;
    let desired = (256.0 * scale).round().max(1.0) as i32;
    let minimum = ((168.0 * scale).round() as i32).clamp(96, desired);
    (desired, minimum)
}

pub fn fit_dock(query: &DockQuery) -> DockFit {
    let thickness = match query.edge {
        BarEdge::Top | BarEdge::Bottom => query.taskbar.height,
        BarEdge::Left | BarEdge::Right => query.taskbar.width,
    };
    if thickness <= 8 || query.taskbar.width <= 0 || query.taskbar.height <= 0 {
        return DockFit::Collapsed;
    }
    let gap = query.gap.max(0);
    match query.edge {
        BarEdge::Top | BarEdge::Bottom => {
            let right = query.tray_start - gap;
            let left_limit = query
                .icons_end
                .map(|end| end.saturating_add(gap))
                .unwrap_or(query.taskbar.x)
                .max(query.taskbar.x);
            let available = right.saturating_sub(left_limit);
            if available < query.minimum || right <= query.taskbar.x {
                return DockFit::NoRoom;
            }
            let width = available.min(query.desired).max(1);
            DockFit::Slot(PxRect {
                x: right - width,
                y: query.taskbar.y,
                width,
                height: query.taskbar.height,
            })
        }
        BarEdge::Left | BarEdge::Right => {
            let bottom = query.tray_start - gap;
            let top_limit = query
                .icons_end
                .map(|end| end.saturating_add(gap))
                .unwrap_or(query.taskbar.y)
                .max(query.taskbar.y);
            let available = bottom.saturating_sub(top_limit);
            if available < query.minimum || bottom <= query.taskbar.y {
                return DockFit::NoRoom;
            }
            let height = available.min(query.desired).max(1);
            DockFit::Slot(PxRect {
                x: query.taskbar.x,
                y: bottom - height,
                width: query.taskbar.width,
                height,
            })
        }
    }
}

/// Desktop and shell windows cover a monitor without being a fullscreen app.
pub fn shell_or_desktop_class(class: &str) -> bool {
    matches!(
        class,
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "NotifyIconOverflowWindow"
            | "DV2ControlHost"
            | "TopLevelWindowForOverflowXamlIsland"
    )
}

/// Fullscreen means the foreground window covers the monitor, including the
/// taskbar. A maximized window and this process's own windows do not count.
pub fn counts_as_fullscreen(class: &str, ours: bool, zoomed: bool, covers: bool) -> bool {
    covers && !ours && !zoomed && !shell_or_desktop_class(class)
}

/// A foreground window covers its monitor when each edge is within `slop`.
pub fn covers_monitor(window: PxRect, monitor: PxRect, slop: i32) -> bool {
    window.width > 0
        && window.height > 0
        && window.x <= monitor.x.saturating_add(slop)
        && window.y <= monitor.y.saturating_add(slop)
        && window.x.saturating_add(window.width)
            >= monitor.x.saturating_add(monitor.width).saturating_sub(slop)
        && window.y.saturating_add(window.height)
            >= monitor
                .y
                .saturating_add(monitor.height)
                .saturating_sub(slop)
}

#[cfg(windows)]
mod win {
    use super::{edge_from_appbar, BarEdge, PxRect};

    pub struct Reading {
        pub taskbar: PxRect,
        pub edge: BarEdge,
        pub tray: PxRect,
        pub tray_start: i32,
        pub icons_end: Option<i32>,
        pub fullscreen: bool,
    }

    pub fn read() -> Result<Reading, &'static str> {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::UI::Shell::{SHAppBarMessage, ABM_GETTASKBARPOS, APPBARDATA};
        use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect};

        unsafe {
            let shell = FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
            if shell.is_null() {
                return Err("taskbar window not found");
            }
            let mut data = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: shell,
                ..APPBARDATA::default()
            };
            let ok = SHAppBarMessage(ABM_GETTASKBARPOS, &mut data);
            let taskbar = if ok != 0 && rect_size(&data.rc).0 > 0 && rect_size(&data.rc).1 > 0 {
                from_rect(data.rc)
            } else {
                let mut rect = RECT::default();
                if GetWindowRect(shell, &mut rect) == 0 {
                    return Err("taskbar rect not found");
                }
                from_rect(rect)
            };
            let edge = if ok != 0 {
                edge_from_appbar(data.uEdge)
            } else if taskbar.height <= taskbar.width {
                if taskbar.y <= 8 {
                    BarEdge::Top
                } else {
                    BarEdge::Bottom
                }
            } else if taskbar.x <= 8 {
                BarEdge::Left
            } else {
                BarEdge::Right
            };
            let tray = tray_rect(shell, taskbar, edge).ok_or("tray rect not found")?;
            let tray_start = match edge {
                BarEdge::Top | BarEdge::Bottom => tray.x,
                BarEdge::Left | BarEdge::Right => tray.y,
            };
            let icons_end = find_class(shell, "MSTaskListWClass").and_then(|hwnd| {
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect) == 0 {
                    return None;
                }
                Some(match edge {
                    BarEdge::Top | BarEdge::Bottom => rect.right,
                    BarEdge::Left | BarEdge::Right => rect.bottom,
                })
            });
            Ok(Reading {
                taskbar,
                edge,
                tray,
                tray_start,
                icons_end,
                fullscreen: foreground_is_fullscreen(shell),
            })
        }
    }

    /// Place a topmost tool window so its visible frame equals `slot`.
    /// Re-asserting `HWND_TOPMOST` keeps the strip above the Win11 XAML taskbar
    /// after the bar is clicked.
    pub fn place_strip(hwnd: *mut std::ffi::c_void, slot: PxRect) -> Result<PxRect, String> {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        };
        if slot.width <= 0 || slot.height <= 0 {
            return Err("strip rect is empty".into());
        }
        let hwnd = hwnd as HWND;
        if hwnd.is_null() {
            return Err("indicator window has no handle".into());
        }
        unsafe {
            apply_tool_styles(hwnd);
            let flags = SWP_NOACTIVATE | SWP_SHOWWINDOW;
            if SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                slot.x,
                slot.y,
                slot.width,
                slot.height,
                flags,
            ) == 0
            {
                return Err("SetWindowPos failed".into());
            }
            // The visible frame can be inset from the outer rect. Nudge twice;
            // the inset is stable, so the second pass lands on the taskbar.
            for _ in 0..2 {
                let visible = visible_rect(hwnd).ok_or("could not read the strip rect")?;
                let outer = window_rect(hwnd).ok_or("could not read the window rect")?;
                let adjusted = super::adjusted_outer(slot, visible, outer);
                if adjusted == outer {
                    break;
                }
                if SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    adjusted.x,
                    adjusted.y,
                    adjusted.width,
                    adjusted.height,
                    flags,
                ) == 0
                {
                    return Err("SetWindowPos failed while aligning the strip".into());
                }
            }
            // Clicking the taskbar raises that topmost window. Put the strip back.
            if SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            ) == 0
            {
                return Err("could not keep the strip above the taskbar".into());
            }
            let visible =
                visible_rect(hwnd).ok_or_else(|| "could not read the strip rect".to_string())?;
            if (visible.x - slot.x).abs() > 2
                || (visible.y - slot.y).abs() > 2
                || (visible.width - slot.width).abs() > 2
                || (visible.height - slot.height).abs() > 2
            {
                return Err(format!(
                    "strip rect {},{} {}x{} does not match taskbar slot {},{} {}x{}",
                    visible.x,
                    visible.y,
                    visible.width,
                    visible.height,
                    slot.x,
                    slot.y,
                    slot.width,
                    slot.height
                ));
            }
            Ok(visible)
        }
    }

    unsafe fn tray_rect(
        shell: windows_sys::Win32::Foundation::HWND,
        taskbar: PxRect,
        edge: BarEdge,
    ) -> Option<PxRect> {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
        if let Some(notify) = find_class(shell, "TrayNotifyWnd") {
            let mut rect = RECT::default();
            if GetWindowRect(notify, &mut rect) != 0 {
                let parsed = from_rect(rect);
                if parsed.width > 0 && parsed.height > 0 && super::rects_overlap(taskbar, parsed) {
                    return Some(parsed);
                }
            }
        }
        let mut children = Vec::new();
        collect_rects(shell, &mut children);
        super::tray_from_children(taskbar, edge, &children)
    }

    unsafe fn apply_tool_styles(hwnd: windows_sys::Win32::Foundation::HWND) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_APPWINDOW,
            WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        };
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let next = (style & !(WS_EX_APPWINDOW as isize))
            | (WS_EX_TOOLWINDOW as isize)
            | (WS_EX_NOACTIVATE as isize)
            | (WS_EX_TOPMOST as isize);
        if next == style {
            return;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }

    unsafe fn visible_rect(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<PxRect> {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
        let mut rect = RECT::default();
        let hr = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut rect as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        );
        if hr == 0 {
            let parsed = from_rect(rect);
            if parsed.width > 0 && parsed.height > 0 {
                return Some(parsed);
            }
        }
        window_rect(hwnd)
    }

    unsafe fn window_rect(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<PxRect> {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let parsed = from_rect(rect);
        if parsed.width > 0 && parsed.height > 0 {
            Some(parsed)
        } else {
            None
        }
    }

    unsafe fn collect_rects(root: windows_sys::Win32::Foundation::HWND, out: &mut Vec<PxRect>) {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindow, GetWindowRect, GW_CHILD, GW_HWNDNEXT};
        let mut child = GetWindow(root, GW_CHILD);
        while !child.is_null() {
            let mut rect = RECT::default();
            if GetWindowRect(child, &mut rect) != 0 {
                let parsed = from_rect(rect);
                if parsed.width > 0 && parsed.height > 0 {
                    out.push(parsed);
                }
            }
            collect_rects(child, out);
            child = GetWindow(child, GW_HWNDNEXT);
        }
    }

    unsafe fn foreground_is_fullscreen(taskbar: windows_sys::Win32::Foundation::HWND) -> bool {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsZoomed,
        };
        let foreground = GetForegroundWindow();
        if foreground.is_null() || foreground == taskbar {
            return false;
        }
        let class = class_of(foreground);
        if super::shell_or_desktop_class(&class) {
            return false;
        }
        let mut owner = 0u32;
        GetWindowThreadProcessId(foreground, &mut owner);
        let ours = owner != 0 && owner == GetCurrentProcessId();
        if ours || IsZoomed(foreground) != 0 {
            return false;
        }
        let mut window = RECT::default();
        if GetWindowRect(foreground, &mut window) == 0 {
            return false;
        }
        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST);
        if monitor.is_null() {
            return false;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..MONITORINFO::default()
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return false;
        }
        let window = from_rect(window);
        let screen = from_rect(info.rcMonitor);
        super::counts_as_fullscreen(&class, false, false, super::covers_monitor(window, screen, 2))
    }

    unsafe fn find_class(
        root: windows_sys::Win32::Foundation::HWND,
        class_name: &str,
    ) -> Option<windows_sys::Win32::Foundation::HWND> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindow, GW_CHILD, GW_HWNDNEXT};
        let mut child = GetWindow(root, GW_CHILD);
        while !child.is_null() {
            if class_of(child) == class_name {
                return Some(child);
            }
            if let Some(found) = find_class(child, class_name) {
                return Some(found);
            }
            child = GetWindow(child, GW_HWNDNEXT);
        }
        None
    }

    unsafe fn class_of(hwnd: windows_sys::Win32::Foundation::HWND) -> String {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
        let mut buf = [0u16; 256];
        let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_rect(rect: windows_sys::Win32::Foundation::RECT) -> PxRect {
        PxRect {
            x: rect.left,
            y: rect.top,
            width: rect.right.saturating_sub(rect.left),
            height: rect.bottom.saturating_sub(rect.top),
        }
    }

    fn rect_size(rect: &windows_sys::Win32::Foundation::RECT) -> (i32, i32) {
        (
            rect.right.saturating_sub(rect.left),
            rect.bottom.saturating_sub(rect.top),
        )
    }
}

#[cfg(windows)]
pub use win::place_strip;
#[cfg(windows)]
pub use win::read as read_taskbar;

#[cfg(test)]
mod tests {
    use super::*;

    fn bottom_bar() -> PxRect {
        PxRect {
            x: 0,
            y: 1040,
            width: 1920,
            height: 48,
        }
    }

    #[test]
    fn appbar_edges_map_to_the_four_sides() {
        assert_eq!(edge_from_appbar(0), BarEdge::Left);
        assert_eq!(edge_from_appbar(1), BarEdge::Top);
        assert_eq!(edge_from_appbar(2), BarEdge::Right);
        assert_eq!(edge_from_appbar(3), BarEdge::Bottom);
        assert_eq!(BarEdge::Bottom.as_str(), "bottom");
    }

    #[test]
    fn the_strip_sits_against_the_tray_and_clears_the_centered_icons() {
        let (desired, minimum) = dock_sizes(48);
        assert_eq!(desired, 256);
        assert_eq!(minimum, 168);
        let fit = fit_dock(&DockQuery {
            taskbar: bottom_bar(),
            edge: BarEdge::Bottom,
            tray_start: 1700,
            icons_end: Some(1200),
            desired,
            minimum,
            gap: 4,
        });
        assert_eq!(
            fit,
            DockFit::Slot(PxRect {
                x: 1440,
                y: 1040,
                width: 256,
                height: 48
            })
        );
    }

    #[test]
    fn a_narrow_gap_falls_back_instead_of_covering_the_icons() {
        let fit = fit_dock(&DockQuery {
            taskbar: bottom_bar(),
            edge: BarEdge::Bottom,
            tray_start: 1400,
            icons_end: Some(1300),
            desired: 256,
            minimum: 168,
            gap: 4,
        });
        assert_eq!(fit, DockFit::NoRoom);
    }

    #[test]
    fn a_collapsed_taskbar_is_not_a_slot() {
        let fit = fit_dock(&DockQuery {
            taskbar: PxRect {
                x: 0,
                y: 1078,
                width: 1920,
                height: 2,
            },
            edge: BarEdge::Bottom,
            tray_start: 1700,
            icons_end: Some(1200),
            desired: 256,
            minimum: 168,
            gap: 4,
        });
        assert_eq!(fit, DockFit::Collapsed);
    }

    #[test]
    fn a_vertical_taskbar_docks_just_above_the_tray() {
        let fit = fit_dock(&DockQuery {
            taskbar: PxRect {
                x: 1872,
                y: 0,
                width: 48,
                height: 1080,
            },
            edge: BarEdge::Right,
            tray_start: 980,
            icons_end: Some(400),
            desired: 256,
            minimum: 168,
            gap: 4,
        });
        assert_eq!(
            fit,
            DockFit::Slot(PxRect {
                x: 1872,
                y: 720,
                width: 48,
                height: 256
            })
        );
    }

    #[test]
    fn a_48px_taskbar_on_a_1920x1200_screen_is_flush_with_the_bar() {
        let taskbar = PxRect {
            x: 0,
            y: 1152,
            width: 1920,
            height: 48,
        };
        let (desired, minimum) = dock_sizes(taskbar.height);
        let fit = fit_dock(&DockQuery {
            taskbar,
            edge: BarEdge::Bottom,
            tray_start: 1680,
            icons_end: Some(1100),
            desired,
            minimum,
            gap: 4,
        });
        assert_eq!(
            fit,
            DockFit::Slot(PxRect {
                x: 1420,
                y: 1152,
                width: 256,
                height: 48
            })
        );
    }

    #[test]
    fn the_outer_window_shifts_so_the_visible_frame_matches_the_taskbar() {
        let requested = PxRect {
            x: 1420,
            y: 1152,
            width: 256,
            height: 48,
        };
        let visible = PxRect {
            x: 1420,
            y: 1153,
            width: 256,
            height: 46,
        };
        assert_eq!(
            adjusted_outer(requested, visible, requested),
            PxRect {
                x: 1420,
                y: 1151,
                width: 256,
                height: 50
            }
        );
        assert_eq!(adjusted_outer(requested, requested, requested), requested);
    }

    #[test]
    fn a_missing_tray_window_is_derived_from_the_notify_area() {
        let taskbar = PxRect {
            x: 0,
            y: 1152,
            width: 1920,
            height: 48,
        };
        let island = taskbar;
        let clock = PxRect {
            x: 1840,
            y: 1152,
            width: 80,
            height: 48,
        };
        let tray = PxRect {
            x: 1680,
            y: 1152,
            width: 240,
            height: 48,
        };
        assert_eq!(
            tray_from_children(taskbar, BarEdge::Bottom, &[island, clock, tray]),
            Some(tray)
        );
        assert_eq!(tray_from_children(taskbar, BarEdge::Bottom, &[island]), None);
    }

    #[test]
    fn a_maximized_or_shell_window_is_not_fullscreen() {
        let screen = PxRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1200,
        };
        let inflated = PxRect {
            x: -8,
            y: -8,
            width: 1936,
            height: 1216,
        };
        assert!(covers_monitor(inflated, screen, 2));
        assert!(!counts_as_fullscreen("Chrome_WidgetWin_1", false, true, true));
        assert!(!counts_as_fullscreen("Progman", false, false, true));
        assert!(!counts_as_fullscreen("WorkerW", false, false, true));
        assert!(!counts_as_fullscreen("Shell_TrayWnd", false, false, true));
        assert!(!counts_as_fullscreen("Chrome_WidgetWin_1", true, false, true));
        assert!(counts_as_fullscreen("ApplicationFrameWindow", false, false, true));
        let work = PxRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1152,
        };
        assert!(!covers_monitor(work, screen, 2));
        assert!(!counts_as_fullscreen(
            "Chrome_WidgetWin_1",
            false,
            false,
            covers_monitor(work, screen, 2)
        ));
    }

    #[test]
    fn a_fullscreen_window_covers_its_monitor() {
        let screen = PxRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert!(covers_monitor(screen, screen, 2));
        assert!(!covers_monitor(
            PxRect {
                x: 20,
                y: 20,
                width: 800,
                height: 600
            },
            screen,
            2
        ));
    }
}
