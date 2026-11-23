use serde::Serialize;
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::path::Path;
use std::process::Command;
use sysinfo::{ProcessesToUpdate, System};

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_APPROVED_KEY: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

#[derive(Debug, Clone, Serialize)]
pub struct OptimizeStatus {
    pub startup_items: Vec<StartupItem>,
    pub tweaks: Vec<DebloatTweak>,
    pub temp_dir: String,
    pub temp_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartupItem {
    pub name: String,
    pub command: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebloatTweak {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub optimized: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TempCleanResult {
    pub deleted_files: usize,
    pub skipped_files: usize,
    pub freed_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KillGroupResult {
    pub name: String,
    pub killed_count: usize,
    pub failed_count: usize,
}

/// Generates a 32-character hex session token from OS-seeded RandomState.
pub fn generate_session_token() -> String {
    let mut h1 = RandomState::new().build_hasher();
    h1.write_u64(std::process::id() as u64);
    let p1 = h1.finish();

    let mut h2 = RandomState::new().build_hasher();
    h2.write_u64(p1 ^ 0x9E37_79B9_7F4A_7C15);
    let p2 = h2.finish();

    format!("{p1:016x}{p2:016x}")
}

pub fn get_status() -> OptimizeStatus {
    let (temp_dir, temp_size_bytes) = measure_temp_dir();
    OptimizeStatus {
        startup_items: list_startup_items(),
        tweaks: list_tweaks(),
        temp_dir,
        temp_size_bytes,
    }
}

pub fn list_startup_items() -> Vec<StartupItem> {
    let run_entries = reg_query_key(RUN_KEY);
    let approved_entries = reg_query_key(STARTUP_APPROVED_KEY);

    let mut approved_map: HashMap<String, bool> = HashMap::new();
    for (name, reg_type, val) in approved_entries {
        if reg_type.eq_ignore_ascii_case("REG_BINARY") {
            approved_map.insert(name.to_lowercase(), is_startup_binary_enabled(&val));
        }
    }

    let mut items: Vec<StartupItem> = run_entries
        .into_iter()
        .filter(|(_, ty, _)| {
            ty.eq_ignore_ascii_case("REG_SZ") || ty.eq_ignore_ascii_case("REG_EXPAND_SZ")
        })
        .map(|(name, _, command)| {
            let enabled = *approved_map.get(&name.to_lowercase()).unwrap_or(&true);
            StartupItem {
                name,
                command,
                enabled,
            }
        })
        .collect();

    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items
}

pub fn set_startup_enabled(name: &str, enabled: bool) -> Result<(), String> {
    let exists = list_startup_items()
        .into_iter()
        .any(|i| i.name.eq_ignore_ascii_case(name));
    if !exists {
        return Err(format!("Startup item '{name}' tidak ditemukan di HKCU Run"));
    }

    let hex = if enabled {
        "020000000000000000000000"
    } else {
        "030000000000000000000000"
    };

    let output = Command::new("reg")
        .args([
            "add",
            STARTUP_APPROVED_KEY,
            "/v",
            name,
            "/t",
            "REG_BINARY",
            "/d",
            hex,
            "/f",
        ])
        .output()
        .map_err(|e| format!("Gagal menjalankan reg.exe: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("reg add gagal: {}", err.trim()));
    }
    Ok(())
}

pub fn list_tweaks() -> Vec<DebloatTweak> {
    let widgets_val = reg_query_dword(
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
        "TaskbarDa",
    );
    let bing_val = reg_query_dword(
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Search",
        "BingSearchEnabled",
    );
    let ads_val = reg_query_dword(
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager",
        "SystemPaneSuggestionsEnabled",
    );
    let bg_val = reg_query_dword(
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications",
        "GlobalUserDisabled",
    );

    vec![
        DebloatTweak {
            id: "widgets",
            title: "Matikan Windows Widgets",
            description: "Menonaktifkan tombol & panel Widgets di taskbar (mengurangi proses msedgewebview2).",
            optimized: widgets_val == Some(0),
        },
        DebloatTweak {
            id: "bing_search",
            title: "Matikan Bing Web Search di Start Menu",
            description: "Mencegah Start Menu mencari ke internet lewat Bing (membuat SearchHost lebih ringan).",
            optimized: bing_val == Some(0),
        },
        DebloatTweak {
            id: "start_ads",
            title: "Matikan Tips, Suggestions & Iklan Start",
            description: "Menonaktifkan saran aplikasi dan konten promosi di Start Menu & Windows.",
            optimized: ads_val == Some(0),
        },
        DebloatTweak {
            id: "background_apps",
            title: "Batasi Background Apps (HKCU)",
            description: "Mengurangi aplikasi Store/UWP yang diam-diam berjalan di latar belakang.",
            optimized: bg_val == Some(1),
        },
    ]
}

pub fn apply_tweak(id: &str, optimized: bool) -> Result<DebloatTweak, String> {
    match id {
        "widgets" => {
            reg_set_dword(
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
                "TaskbarDa",
                if optimized { 0 } else { 1 },
            )?;
        }
        "bing_search" => {
            reg_set_dword(
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Search",
                "BingSearchEnabled",
                if optimized { 0 } else { 1 },
            )?;
        }
        "start_ads" => {
            let v = if optimized { 0 } else { 1 };
            let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
            for name in [
                "SystemPaneSuggestionsEnabled",
                "SubscribedContent-338388Enabled",
                "SubscribedContent-338389Enabled",
                "SoftLandingEnabled",
            ] {
                reg_set_dword(key, name, v)?;
            }
        }
        "background_apps" => {
            reg_set_dword(
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications",
                "GlobalUserDisabled",
                if optimized { 1 } else { 0 },
            )?;
        }
        _ => return Err(format!("Tweak ID '{id}' tidak dikenali")),
    }

    list_tweaks()
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| "Tweak tidak ditemukan setelah disimpan".to_string())
}

pub fn measure_temp_dir() -> (String, u64) {
    let dir = std::env::temp_dir();
    let total = dir_size_bytes(&dir);
    (dir.to_string_lossy().into_owned(), total)
}

pub fn clean_temp_dir() -> TempCleanResult {
    let dir = std::env::temp_dir();
    let mut res = TempCleanResult {
        deleted_files: 0,
        skipped_files: 0,
        freed_bytes: 0,
    };

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return res;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            res.skipped_files += 1;
            continue;
        };
        if meta.file_type().is_symlink() {
            res.skipped_files += 1;
            continue;
        }
        if meta.is_file() {
            let len = meta.len();
            if std::fs::remove_file(&path).is_ok() {
                res.deleted_files += 1;
                res.freed_bytes += len;
            } else {
                res.skipped_files += 1;
            }
        } else if meta.is_dir() {
            clean_subdir_recursive(&path, &mut res);
        }
    }

    res
}

fn clean_subdir_recursive(path: &Path, res: &mut TempCleanResult) {
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                res.skipped_files += 1;
                continue;
            };
            if meta.file_type().is_symlink() {
                res.skipped_files += 1;
                continue;
            }
            if meta.is_file() {
                let len = meta.len();
                if std::fs::remove_file(&p).is_ok() {
                    res.deleted_files += 1;
                    res.freed_bytes += len;
                } else {
                    res.skipped_files += 1;
                }
            } else if meta.is_dir() {
                clean_subdir_recursive(&p, res);
            }
        }
    }
    let _ = std::fs::remove_dir(path);
}

fn dir_size_bytes(root: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_file() {
                total = total.saturating_add(meta.len());
            } else if meta.is_dir() {
                stack.push(p);
            }
        }
    }
    total
}

pub fn is_protected_process(name: &str) -> bool {
    let base = name
        .trim()
        .to_lowercase()
        .strip_suffix(".exe")
        .unwrap_or_else(|| name.trim())
        .to_lowercase();

    matches!(
        base.as_str(),
        "system"
            | "idle"
            | "system idle process"
            | "registry"
            | "smss"
            | "csrss"
            | "wininit"
            | "services"
            | "lsass"
            | "lsaiso"
            | "svchost"
            | "winlogon"
            | "fontdrvhost"
            | "dwm"
            | "memory compression"
            | "explorer"
            | "sihost"
            | "taskhostw"
            | "ctfmon"
            | "runtimebroker"
            | "securityhealthservice"
            | "msmpeng"
            | "nissrv"
            | "audiodg"
            | "conhost"
            | "hw-monitor"
    )
}

pub fn kill_process_group(name: &str) -> Result<KillGroupResult, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Nama proses tidak boleh kosong".to_string());
    }
    if is_protected_process(trimmed) {
        return Err(format!(
            "Proses '{trimmed}' dilindungi sistem dan tidak boleh dimatikan"
        ));
    }

    let target_base = trimmed
        .to_lowercase()
        .strip_suffix(".exe")
        .unwrap_or(&trimmed.to_lowercase())
        .to_string();

    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );

    let mut killed_count = 0usize;
    let mut failed_count = 0usize;

    for proc_item in sys.processes().values() {
        let pname = proc_item.name().to_string_lossy().to_lowercase();
        let pbase = pname.strip_suffix(".exe").unwrap_or(&pname);
        if pbase == target_base {
            if proc_item.kill() {
                killed_count += 1;
            } else {
                failed_count += 1;
            }
        }
    }

    Ok(KillGroupResult {
        name: trimmed.to_string(),
        killed_count,
        failed_count,
    })
}

fn reg_query_key(key: &str) -> Vec<(String, String, String)> {
    let Ok(output) = Command::new("reg").args(["query", key]).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_reg_query_lines(&text)
}

fn parse_reg_query_lines(text: &str) -> Vec<(String, String, String)> {
    let mut results = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("HKEY_") || trimmed.starts_with("End of search")
        {
            continue;
        }
        // reg query formats value lines with 4-space separators:
        // <Name>    <REG_TYPE>    <Data>
        let parts: Vec<&str> = trimmed.split("    ").filter(|s| !s.is_empty()).collect();
        if parts.len() >= 3 && parts[1].starts_with("REG_") {
            results.push((
                parts[0].trim().to_string(),
                parts[1].trim().to_string(),
                parts[2..].join("    ").trim().to_string(),
            ));
        }
    }
    results
}

fn is_startup_binary_enabled(hex: &str) -> bool {
    let clean = hex.trim();
    if clean.len() < 2 {
        return true;
    }
    matches!(&clean[..2], "02" | "06")
}

fn reg_query_dword(key: &str, value_name: &str) -> Option<u32> {
    let output = Command::new("reg")
        .args(["query", key, "/v", value_name])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for (_, ty, data) in parse_reg_query_lines(&text) {
        if ty.eq_ignore_ascii_case("REG_DWORD") {
            let trimmed = data.trim();
            if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
                return u32::from_str_radix(hex, 16).ok();
            }
            return trimmed.parse::<u32>().ok();
        }
    }
    None
}

fn reg_set_dword(key: &str, value_name: &str, data: u32) -> Result<(), String> {
    let data_str = data.to_string();
    let output = Command::new("reg")
        .args(["add", key, "/v", value_name, "/t", "REG_DWORD", "/d", &data_str, "/f"])
        .output()
        .map_err(|e| format!("Gagal menjalankan reg.exe: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("reg add gagal: {}", err.trim()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_reg_query_lines() {
        let sample = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    OneDrive    REG_SZ    \"C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe\" /background\r\n    Teams    REG_EXPAND_SZ    \"C:\\Teams\\ms-teams.exe\"\r\n";
        let parsed = parse_reg_query_lines(sample);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "OneDrive");
        assert_eq!(parsed[0].1, "REG_SZ");
        assert_eq!(parsed[1].0, "Teams");
        assert_eq!(parsed[1].1, "REG_EXPAND_SZ");
    }

    #[test]
    fn test_is_startup_binary_enabled() {
        assert!(is_startup_binary_enabled("020000000000000000000000"));
        assert!(is_startup_binary_enabled("060000000000000000000000"));
        assert!(!is_startup_binary_enabled("030000000000000000000000"));
    }

    #[test]
    fn test_is_protected_process() {
        assert!(is_protected_process("explorer.exe"));
        assert!(is_protected_process("EXPLORER"));
        assert!(is_protected_process("svchost.exe"));
        assert!(is_protected_process("hw-monitor.exe"));
        assert!(!is_protected_process("chrome.exe"));
        assert!(!is_protected_process("ms-teams.exe"));
    }
}
