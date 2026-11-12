//! Foreground application name, used only as polish context.

pub fn front_app_name() -> String {
    let name = read_name();
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("newtype") {
        String::new()
    } else {
        format!("前台应用：{trimmed}")
    }
}

fn read_name() -> String {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("osascript")
            .args([
                "-e",
                "tell application \"System Events\" to get name of first application process whose frontmost is true",
            ])
            .output();
        return match output {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            }
            _ => String::new(),
        };
    }
    #[cfg(target_os = "windows")]
    {
        return windows_foreground();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        String::new()
    }
}

#[cfg(target_os = "windows")]
fn windows_foreground() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 {
            return String::new();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return String::new();
        }
        let handle = OpenProcess(0x1000, 0, pid);
        if handle == 0 {
            return String::new();
        }
        let mut buffer = [0u16; 512];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(handle);
        if ok == 0 {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        path.rsplit(['\\', '/'])
            .next()
            .unwrap_or("")
            .trim_end_matches(".exe")
            .trim_end_matches(".EXE")
            .to_string()
    }
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
extern "system" {
    fn GetForegroundWindow() -> isize;
    fn GetWindowThreadProcessId(hwnd: isize, process_id: *mut u32) -> u32;
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    fn CloseHandle(handle: isize) -> i32;
    fn QueryFullProcessImageNameW(
        handle: isize,
        flags: u32,
        name: *mut u16,
        size: *mut u32,
    ) -> i32;
}
