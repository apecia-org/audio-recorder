use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Browser,
    MeetingApp,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub display_name: String,
    pub exe_basename: String,
    pub bundle_id: Option<String>,
    pub icon_b64: Option<String>,
    pub category: Category,
    pub is_top_level: bool,
}

const BROWSER_BUNDLES: &[&str] = &[
    "com.google.Chrome",
    "com.apple.Safari",
    "org.mozilla.firefox",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "com.operasoftware.Opera",
    "com.vivaldi.Vivaldi",
];

const BROWSER_EXES: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "iexplore.exe",
];

const MEETING_BUNDLES: &[&str] = &[
    "com.microsoft.teams2",
    "com.microsoft.teams",
    "us.zoom.xos",
    "com.cisco.webexmeetingsapp",
    "com.cisco.webex.meetings",
    "com.tinyspeck.slackmacgap",
    "com.google.GoogleMeet",
    "com.skype.skype",
    "com.discord.Discord",
];

const MEETING_EXES: &[&str] = &[
    "Teams.exe",
    "ms-teams.exe",
    "Zoom.exe",
    "CptHost.exe",
    "Webex.exe",
    "Slack.exe",
    "Discord.exe",
    "Skype.exe",
];

fn classify(bundle: Option<&str>, exe: &str) -> Category {
    if let Some(b) = bundle {
        if BROWSER_BUNDLES.iter().any(|x| b.eq_ignore_ascii_case(x)) {
            return Category::Browser;
        }
        if MEETING_BUNDLES.iter().any(|x| b.eq_ignore_ascii_case(x)) {
            return Category::MeetingApp;
        }
    }
    if BROWSER_EXES.iter().any(|x| exe.eq_ignore_ascii_case(x)) {
        return Category::Browser;
    }
    if MEETING_EXES.iter().any(|x| exe.eq_ignore_ascii_case(x)) {
        return Category::MeetingApp;
    }
    Category::Other
}

pub fn list_processes() -> Vec<ProcessInfo> {
    #[cfg(target_os = "macos")]
    {
        macos::list()
    }
    #[cfg(target_os = "windows")]
    {
        windows::list()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

/// Resolve a possibly-child PID to its top-level browser/meeting parent PID.
/// On macOS, NSWorkspace already returns app-level PIDs, so this is a no-op.
pub fn resolve_top_level_pid(pid: u32) -> u32 {
    #[cfg(target_os = "windows")]
    {
        windows::resolve_top_level(pid)
    }
    #[cfg(not(target_os = "windows"))]
    {
        pid
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{classify, Category, ProcessInfo};
    use objc2_app_kit::NSWorkspace;

    pub fn list() -> Vec<ProcessInfo> {
        let workspace = unsafe { NSWorkspace::sharedWorkspace() };
        let apps = unsafe { workspace.runningApplications() };
        let mut out = Vec::with_capacity(apps.len());

        for app in apps.iter() {
            let pid = unsafe { app.processIdentifier() };
            if pid <= 0 {
                continue;
            }
            let name = unsafe { app.localizedName() }
                .map(|s| s.to_string())
                .unwrap_or_default();
            let bundle = unsafe { app.bundleIdentifier() }.map(|s| s.to_string());
            let exe_basename = unsafe { app.bundleURL() }
                .and_then(|u| unsafe { u.lastPathComponent() })
                .map(|s| s.to_string())
                .unwrap_or_else(|| name.clone());

            let category = classify(bundle.as_deref(), &exe_basename);

            out.push(ProcessInfo {
                pid: pid as u32,
                display_name: if name.is_empty() {
                    exe_basename.clone()
                } else {
                    name
                },
                exe_basename,
                bundle_id: bundle,
                icon_b64: None,
                category,
                is_top_level: true,
            });
        }

        out.sort_by(|a, b| {
            category_order(a.category)
                .cmp(&category_order(b.category))
                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
        });
        out
    }

    fn category_order(c: Category) -> u8 {
        match c {
            Category::MeetingApp => 0,
            Category::Browser => 1,
            Category::Other => 2,
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{classify, Category, ProcessInfo};
    use std::collections::{HashMap, HashSet};
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use sysinfo::{ProcessesToUpdate, System};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible,
    };

    pub fn list() -> Vec<ProcessInfo> {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);

        let visible_pids = collect_visible_window_pids();
        let titles = collect_window_titles();

        let by_pid: HashMap<u32, &sysinfo::Process> = sys
            .processes()
            .iter()
            .map(|(pid, p)| (pid.as_u32(), p))
            .collect();

        let mut out = Vec::new();
        let mut seen_top = HashSet::new();

        for (pid_u, proc) in by_pid.iter() {
            let exe = proc
                .exe()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if exe.is_empty() {
                continue;
            }

            let category = classify(None, &exe);
            let is_visible = visible_pids.contains(pid_u);

            let top_pid = resolve_top_level_with(*pid_u, &by_pid, &exe);
            let is_top = top_pid == *pid_u;
            if !is_top && category == Category::Other {
                continue;
            }
            if category == Category::Other && !is_visible {
                continue;
            }
            if !seen_top.insert(top_pid) {
                continue;
            }

            let title = titles
                .get(&top_pid)
                .cloned()
                .unwrap_or_else(|| exe.clone());

            out.push(ProcessInfo {
                pid: top_pid,
                display_name: title,
                exe_basename: exe,
                bundle_id: None,
                icon_b64: None,
                category,
                is_top_level: true,
            });
        }

        out.sort_by(|a, b| {
            category_order(a.category)
                .cmp(&category_order(b.category))
                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
        });
        out
    }

    fn category_order(c: Category) -> u8 {
        match c {
            Category::MeetingApp => 0,
            Category::Browser => 1,
            Category::Other => 2,
        }
    }

    pub fn resolve_top_level(pid: u32) -> u32 {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let by_pid: HashMap<u32, &sysinfo::Process> = sys
            .processes()
            .iter()
            .map(|(p, proc)| (p.as_u32(), proc))
            .collect();
        let exe = by_pid
            .get(&pid)
            .and_then(|p| p.exe())
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        resolve_top_level_with(pid, &by_pid, &exe)
    }

    fn resolve_top_level_with(
        pid: u32,
        by_pid: &HashMap<u32, &sysinfo::Process>,
        exe: &str,
    ) -> u32 {
        let mut current = pid;
        for _ in 0..16 {
            let Some(proc) = by_pid.get(&current) else {
                return current;
            };
            let Some(parent_pid) = proc.parent() else {
                return current;
            };
            let parent_exe = by_pid
                .get(&parent_pid.as_u32())
                .and_then(|p| p.exe())
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if parent_exe.eq_ignore_ascii_case(exe) {
                current = parent_pid.as_u32();
                continue;
            }
            return current;
        }
        current
    }

    fn collect_visible_window_pids() -> HashSet<u32> {
        let mut pids: HashSet<u32> = HashSet::new();
        let pids_ptr = &mut pids as *mut HashSet<u32> as isize;
        unsafe {
            let _ = EnumWindows(Some(enum_visible_pid_proc), LPARAM(pids_ptr));
        }
        pids
    }

    fn collect_window_titles() -> HashMap<u32, String> {
        let mut map: HashMap<u32, String> = HashMap::new();
        let map_ptr = &mut map as *mut HashMap<u32, String> as isize;
        unsafe {
            let _ = EnumWindows(Some(enum_title_proc), LPARAM(map_ptr));
        }
        map
    }

    unsafe extern "system" fn enum_visible_pid_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            let pids = &mut *(lparam.0 as *mut HashSet<u32>);
            pids.insert(pid);
        }
        BOOL(1)
    }

    unsafe extern "system" fn enum_title_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return BOOL(1);
        }
        let mut buf = vec![0u16; (len as usize) + 1];
        let copied = GetWindowTextW(hwnd, &mut buf);
        if copied <= 0 {
            return BOOL(1);
        }
        let title = OsString::from_wide(&buf[..copied as usize])
            .to_string_lossy()
            .to_string();
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 && !title.is_empty() {
            let map = &mut *(lparam.0 as *mut HashMap<u32, String>);
            map.entry(pid).or_insert(title);
        }
        BOOL(1)
    }
}
