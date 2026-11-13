//! Dock the indicator in the empty taskbar strip beside the system tray.
//!
//! Windows 11 has no DeskBand API. The window is a borderless topmost panel
//! whose right edge sits against the tray and whose height matches the taskbar.
//! If that strip is missing or too narrow, the caller keeps the floating panel.
//! This module does not draw a tray icon.

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

pub fn edge_from_appbar(edge: u32) -> BarEdge {
    match edge {
        0 => BarEdge::Left,
        1 => BarEdge::Top,
        2 => BarEdge::Right,
        _ => BarEdge::Bottom,
    }
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
        pub tray_start: i32,
        pub icons_end: Option<i32>,
        pub fullscreen: bool,
    }

    pub fn read() -> Option<Reading> {
        use windows_sys::Win32::Foundation::{HWND, RECT};
        use windows_sys::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::Shell::{SHAppBarMessage, ABM_GETTASKBARPOS, APPBARDATA};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowRect,
            GetWindowThreadProcessId, GW_CHILD, GW_HWNDNEXT,
        };

        unsafe {
            let tray = FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null());
            if tray.is_null() {
                return None;
            }
            let mut data = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: tray,
                ..APPBARDATA::default()
            };
            let ok = SHAppBarMessage(ABM_GETTASKBARPOS, &mut data);
            let taskbar = if ok != 0 && rect_size(&data.rc).0 > 0 && rect_size(&data.rc).1 > 0 {
                from_rect(data.rc)
            } else {
                let mut rect = RECT::default();
                if GetWindowRect(tray, &mut rect) == 0 {
                    return None;
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
            let notify = find_class(tray, "TrayNotifyWnd")?;
            let mut notify_rect = RECT::default();
            if GetWindowRect(notify, &mut notify_rect) == 0 {
                return None;
            }
            let tray_start = match edge {
                BarEdge::Top | BarEdge::Bottom => notify_rect.left,
                BarEdge::Left | BarEdge::Right => notify_rect.top,
            };
            let icons_end = find_class(tray, "MSTaskListWClass").and_then(|hwnd| {
                let mut rect = RECT::default();
                if GetWindowRect(hwnd, &mut rect) == 0 {
                    return None;
                }
                Some(match edge {
                    BarEdge::Top | BarEdge::Bottom => rect.right,
                    BarEdge::Left | BarEdge::Right => rect.bottom,
                })
            });
            Some(Reading {
                taskbar,
                edge,
                tray_start,
                icons_end,
                fullscreen: foreground_is_fullscreen(tray),
            })
        }
    }

    unsafe fn foreground_is_fullscreen(taskbar: windows_sys::Win32::Foundation::HWND) -> bool {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
        };
        let foreground = GetForegroundWindow();
        if foreground.is_null() || foreground == taskbar {
            return false;
        }
        let mut owner = 0u32;
        GetWindowThreadProcessId(foreground, &mut owner);
        if owner == GetCurrentProcessId() {
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
        super::covers_monitor(window, screen, 2)
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
