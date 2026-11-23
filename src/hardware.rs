use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};
use wmi::{COMLibrary, WMIConnection};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareReport {
    pub hostname: String,
    pub scanned_at_unix: u64,
    pub scan_duration_ms: u64,
    pub warnings: Vec<String>,
    pub cpu: Vec<CpuInfo>,
    pub gpus: Vec<GpuInfo>,
    pub network_adapters: Vec<NetworkAdapterInfo>,
    pub display_outputs: DisplaySummary,
    pub usb: UsbSummary,
    pub device_manager: DeviceManagerSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    pub name: String,
    pub manufacturer: String,
    pub physical_cores: u32,
    pub logical_threads: u32,
    pub base_clock_mhz: u32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub kind: String, // "iGPU" | "dGPU" | "Virtual/Other"
    pub is_dedicated: bool,
    pub vram_bytes: u64,
    pub vram_display: String,
    pub driver_version: String,
    pub status: String,
    pub error_code: u32,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkAdapterInfo {
    pub name: String,
    pub connection_id: String,
    pub adapter_type: String, // "Ethernet/LAN" | "Wi-Fi" | "Other"
    pub mac_address: String,
    pub speed_bps: u64,
    pub speed_display: String,
    pub connection_status: String,
    pub cable_connected: bool,
    pub status_code: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplaySummary {
    pub wmi_available: bool,
    pub hdmi_connected: bool,
    pub hdmi_active_count: usize,
    pub active_output_count: usize,
    pub outputs: Vec<DisplayOutputInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayOutputInfo {
    pub instance_name: String,
    pub technology: String,
    pub technology_code: i64,
    pub is_hdmi: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbSummary {
    pub usb3_controller_count: usize,
    pub usb2_controller_count: usize,
    pub usb3_active_devices: usize,
    pub usb2_active_devices: usize,
    pub controllers: Vec<UsbControllerInfo>,
    pub devices: Vec<UsbDeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbControllerInfo {
    pub name: String,
    pub device_id: String,
    pub usb_version: String, // "USB 3.0" | "USB 2.0" | "USB 1.1"
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbDeviceInfo {
    pub name: String,
    pub device_id: String,
    pub manufacturer: String,
    pub usb_version: String,
    pub controller_name: String,
    pub status: String,
    pub ok: bool,
    pub is_hub: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceManagerSummary {
    pub total_devices: usize,
    pub ok_count: usize,
    pub error_count: usize,
    pub categories: Vec<DeviceManagerCategory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceManagerCategory {
    pub name: String,
    pub total: usize,
    pub error_count: usize,
    pub devices: Vec<DeviceManagerItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceManagerItem {
    pub name: String,
    pub manufacturer: String,
    pub device_id: String,
    pub pnp_class: String,
    pub status: String,
    pub error_code: u32,
    pub error_label: String,
    pub ok: bool,
}

// --- WMI raw deserialization structs ---

fn de_opt_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        U64(u64),
        I64(i64),
        Str(String),
    }

    let opt = Option::<NumOrStr>::deserialize(deserializer)?;
    Ok(match opt {
        Some(NumOrStr::U64(v)) => Some(v),
        Some(NumOrStr::I64(v)) if v >= 0 => Some(v as u64),
        Some(NumOrStr::Str(s)) => s.trim().parse::<u64>().ok(),
        _ => None,
    })
}

fn de_opt_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        I64(i64),
        U64(u64),
        Str(String),
    }

    let opt = Option::<NumOrStr>::deserialize(deserializer)?;
    Ok(match opt {
        Some(NumOrStr::I64(v)) => Some(v),
        Some(NumOrStr::U64(v)) => i64::try_from(v).ok(),
        Some(NumOrStr::Str(s)) => s.trim().parse::<i64>().ok(),
        None => None,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WmiProcessor {
    name: Option<String>,
    manufacturer: Option<String>,
    number_of_cores: Option<u32>,
    number_of_logical_processors: Option<u32>,
    max_clock_speed: Option<u32>,
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WmiVideoController {
    name: Option<String>,
    adapter_compatibility: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u64")]
    adapter_r_a_m: Option<u64>,
    driver_version: Option<String>,
    config_manager_error_code: Option<u32>,
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WmiNetworkAdapter {
    name: Option<String>,
    #[serde(rename = "NetConnectionID")]
    net_connection_id: Option<String>,
    #[serde(rename = "MACAddress")]
    mac_address: Option<String>,
    #[serde(default, deserialize_with = "de_opt_u64")]
    speed: Option<u64>,
    net_connection_status: Option<u16>,
    adapter_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WmiMonitorConnectionParams {
    instance_name: Option<String>,
    active: Option<bool>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    video_output_technology: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WmiUsbController {
    name: Option<String>,
    #[serde(rename = "DeviceID")]
    device_id: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WmiUsbControllerDevice {
    antecedent: Option<String>,
    dependent: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WmiPnPEntity {
    name: Option<String>,
    manufacturer: Option<String>,
    #[serde(rename = "DeviceID")]
    device_id: Option<String>,
    #[serde(rename = "PNPClass")]
    pnp_class: Option<String>,
    config_manager_error_code: Option<u32>,
    status: Option<String>,
}

/// Runs WMI scan in a short-lived child process (`--wmi-scan`) so WMI/COM
/// DLLs are unloaded immediately after scanning, keeping the main server's RAM low.
pub fn scan_hardware_isolated() -> HardwareReport {
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(out) = std::process::Command::new(exe).arg("--wmi-scan").output() {
            if out.status.success() {
                if let Ok(report) = serde_json::from_slice::<HardwareReport>(&out.stdout) {
                    return report;
                }
            }
        }
    }
    scan_hardware()
}

/// Runs a full WMI hardware & Device Manager scan in an isolated thread
/// so COM initialization never conflicts with caller threads.
pub fn scan_hardware() -> HardwareReport {
    let handle = std::thread::spawn(scan_hardware_inner);
    match handle.join() {
        Ok(report) => report,
        Err(_) => empty_report_with_warning("Hardware scan thread panicked".to_string()),
    }
}

fn empty_report_with_warning(warning: String) -> HardwareReport {
    HardwareReport {
        hostname: hostname(),
        scanned_at_unix: unix_now(),
        scan_duration_ms: 0,
        warnings: vec![warning],
        cpu: Vec::new(),
        gpus: Vec::new(),
        network_adapters: Vec::new(),
        display_outputs: DisplaySummary {
            wmi_available: false,
            hdmi_connected: false,
            hdmi_active_count: 0,
            active_output_count: 0,
            outputs: Vec::new(),
        },
        usb: UsbSummary {
            usb3_controller_count: 0,
            usb2_controller_count: 0,
            usb3_active_devices: 0,
            usb2_active_devices: 0,
            controllers: Vec::new(),
            devices: Vec::new(),
        },
        device_manager: DeviceManagerSummary {
            total_devices: 0,
            ok_count: 0,
            error_count: 0,
            categories: Vec::new(),
        },
    }
}

fn scan_hardware_inner() -> HardwareReport {
    let start = std::time::Instant::now();
    let mut warnings = Vec::new();

    let com_con = match COMLibrary::new() {
        Ok(c) => c,
        Err(e) => return empty_report_with_warning(format!("COM init failed: {e}")),
    };

    let wmi_cim = match WMIConnection::new(com_con) {
        Ok(w) => w,
        Err(e) => return empty_report_with_warning(format!("WMI CIMv2 connection failed: {e}")),
    };

    // 1. CPU
    let cpu = match wmi_cim.raw_query::<WmiProcessor>(
        "SELECT Name, Manufacturer, NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed, Status FROM Win32_Processor",
    ) {
        Ok(rows) => rows
            .into_iter()
            .map(|r| CpuInfo {
                name: r.name.unwrap_or_else(|| "Unknown CPU".to_string()).trim().to_string(),
                manufacturer: r.manufacturer.unwrap_or_else(|| "-".to_string()),
                physical_cores: r.number_of_cores.unwrap_or(0),
                logical_threads: r.number_of_logical_processors.unwrap_or(0),
                base_clock_mhz: r.max_clock_speed.unwrap_or(0),
                status: r.status.unwrap_or_else(|| "OK".to_string()),
            })
            .collect(),
        Err(e) => {
            warnings.push(format!("Win32_Processor query failed: {e}"));
            Vec::new()
        }
    };

    // 2. GPU
    let gpus = match wmi_cim.raw_query::<WmiVideoController>(
        "SELECT Name, AdapterCompatibility, AdapterRAM, DriverVersion, ConfigManagerErrorCode, Status FROM Win32_VideoController",
    ) {
        Ok(rows) => rows
            .into_iter()
            .map(|r| {
                let name = r.name.unwrap_or_else(|| "Unknown GPU".to_string()).trim().to_string();
                let vendor = r.adapter_compatibility.unwrap_or_else(|| "-".to_string());
                let vram_bytes = r.adapter_r_a_m.unwrap_or(0);
                let error_code = r.config_manager_error_code.unwrap_or(0);
                let ok = error_code == 0;
                let (kind, is_dedicated) = classify_gpu(&name, &vendor);
                let status = if ok {
                    r.status.unwrap_or_else(|| "OK".to_string())
                } else {
                    format!("Error (Code {error_code})")
                };
                GpuInfo {
                    name,
                    vendor,
                    kind: kind.to_string(),
                    is_dedicated,
                    vram_bytes,
                    vram_display: format_bytes_vram(vram_bytes),
                    driver_version: r.driver_version.unwrap_or_else(|| "-".to_string()),
                    status,
                    error_code,
                    ok,
                }
            })
            .collect(),
        Err(e) => {
            warnings.push(format!("Win32_VideoController query failed: {e}"));
            Vec::new()
        }
    };

    // 3. Network Adapters (physical LAN / Wi-Fi)
    let network_adapters = match wmi_cim.raw_query::<WmiNetworkAdapter>(
        "SELECT Name, NetConnectionID, MACAddress, Speed, NetConnectionStatus, PhysicalAdapter, AdapterType FROM Win32_NetworkAdapter WHERE PhysicalAdapter = TRUE",
    ) {
        Ok(rows) => rows
            .into_iter()
            .map(|r| {
                let name = r.name.unwrap_or_else(|| "Unknown Adapter".to_string());
                let conn_id = r.net_connection_id.unwrap_or_else(|| "-".to_string());
                let adapter_type_raw = r.adapter_type.unwrap_or_default();
                let adapter_type = classify_network_adapter(&name, &conn_id, &adapter_type_raw);
                let speed_bps = r.speed.unwrap_or(0);
                let status_code = r.net_connection_status.unwrap_or(0);
                let (connection_status, cable_connected) = map_net_status(status_code);
                NetworkAdapterInfo {
                    name,
                    connection_id: conn_id,
                    adapter_type: adapter_type.to_string(),
                    mac_address: r.mac_address.unwrap_or_else(|| "-".to_string()),
                    speed_bps,
                    speed_display: format_link_speed(speed_bps, cable_connected),
                    connection_status: connection_status.to_string(),
                    cable_connected,
                    status_code,
                }
            })
            .collect(),
        Err(e) => {
            warnings.push(format!("Win32_NetworkAdapter query failed: {e}"));
            Vec::new()
        }
    };

    // 4. Display / HDMI outputs in root\wmi (optional, safe fallback on VMs)
    let display_outputs = query_display_outputs(com_con);

    // 5. Device Manager (Win32_PnPEntity)
    let pnp_rows = match wmi_cim.raw_query::<WmiPnPEntity>(
        "SELECT Name, Manufacturer, DeviceID, PNPClass, ConfigManagerErrorCode, Status FROM Win32_PnPEntity",
    ) {
        Ok(rows) => rows,
        Err(e) => {
            warnings.push(format!("Win32_PnPEntity query failed: {e}"));
            Vec::new()
        }
    };

    // 6. USB Controllers & plugged-in devices
    let usb = query_usb_summary(&wmi_cim, &pnp_rows, &mut warnings);

    let device_manager = build_device_manager_summary(pnp_rows);

    HardwareReport {
        hostname: hostname(),
        scanned_at_unix: unix_now(),
        scan_duration_ms: start.elapsed().as_millis() as u64,
        warnings,
        cpu,
        gpus,
        network_adapters,
        display_outputs,
        usb,
        device_manager,
    }
}

fn query_display_outputs(com_con: COMLibrary) -> DisplaySummary {
    let wmi_root = match WMIConnection::with_namespace_path("root\\wmi", com_con) {
        Ok(conn) => conn,
        Err(_) => {
            return DisplaySummary {
                wmi_available: false,
                hdmi_connected: false,
                hdmi_active_count: 0,
                active_output_count: 0,
                outputs: Vec::new(),
            }
        }
    };

    let rows = match wmi_root.raw_query::<WmiMonitorConnectionParams>(
        "SELECT InstanceName, Active, VideoOutputTechnology FROM WmiMonitorConnectionParams",
    ) {
        Ok(r) => r,
        Err(_) => {
            return DisplaySummary {
                wmi_available: false,
                hdmi_connected: false,
                hdmi_active_count: 0,
                active_output_count: 0,
                outputs: Vec::new(),
            }
        }
    };

    let mut outputs = Vec::with_capacity(rows.len());
    let mut hdmi_active_count = 0usize;
    let mut active_output_count = 0usize;

    for r in rows {
        let tech_code = r.video_output_technology.unwrap_or(-1);
        let active = r.active.unwrap_or(false);
        let (tech_label, is_hdmi) = map_video_output_technology(tech_code);
        if active {
            active_output_count += 1;
            if is_hdmi {
                hdmi_active_count += 1;
            }
        }
        outputs.push(DisplayOutputInfo {
            instance_name: r.instance_name.unwrap_or_else(|| "Unknown Display".to_string()),
            technology: tech_label.to_string(),
            technology_code: tech_code,
            is_hdmi,
            active,
        });
    }

    DisplaySummary {
        wmi_available: true,
        hdmi_connected: hdmi_active_count > 0,
        hdmi_active_count,
        active_output_count,
        outputs,
    }
}

fn query_usb_summary(
    wmi_cim: &WMIConnection,
    pnp_rows: &[WmiPnPEntity],
    warnings: &mut Vec<String>,
) -> UsbSummary {
    let controllers_raw = match wmi_cim
        .raw_query::<WmiUsbController>("SELECT Name, DeviceID, Status FROM Win32_USBController")
    {
        Ok(rows) => rows,
        Err(e) => {
            warnings.push(format!("Win32_USBController query failed: {e}"));
            Vec::new()
        }
    };

    let mut controller_map: HashMap<String, (String, String)> = HashMap::new();
    let mut controllers = Vec::with_capacity(controllers_raw.len());
    let mut usb3_controller_count = 0usize;
    let mut usb2_controller_count = 0usize;

    for c in controllers_raw {
        let name = c.name.unwrap_or_else(|| "USB Controller".to_string());
        let device_id = c.device_id.unwrap_or_default();
        let usb_version = classify_usb_controller(&name, &device_id);
        match usb_version {
            "USB 3.0" => usb3_controller_count += 1,
            "USB 2.0" => usb2_controller_count += 1,
            _ => {}
        }
        if !device_id.is_empty() {
            controller_map.insert(
                device_id.to_uppercase(),
                (name.clone(), usb_version.to_string()),
            );
        }
        controllers.push(UsbControllerInfo {
            name,
            device_id,
            usb_version: usb_version.to_string(),
            status: c.status.unwrap_or_else(|| "OK".to_string()),
        });
    }

    let links = match wmi_cim
        .raw_query::<WmiUsbControllerDevice>("SELECT Antecedent, Dependent FROM Win32_USBControllerDevice")
    {
        Ok(rows) => rows,
        Err(e) => {
            warnings.push(format!("Win32_USBControllerDevice query failed: {e}"));
            Vec::new()
        }
    };

    let mut pnp_by_id: HashMap<String, &WmiPnPEntity> = HashMap::new();
    for item in pnp_rows {
        if let Some(id) = &item.device_id {
            pnp_by_id.insert(id.to_uppercase(), item);
        }
    }

    let mut seen_devices: HashMap<String, UsbDeviceInfo> = HashMap::new();
    for link in links {
        let Some(ant_raw) = link.antecedent.as_deref() else { continue };
        let Some(dep_raw) = link.dependent.as_deref() else { continue };
        let Some(ctrl_id) = extract_wmi_device_id(ant_raw) else { continue };
        let Some(dev_id) = extract_wmi_device_id(dep_raw) else { continue };

        let dev_key = dev_id.to_uppercase();
        if seen_devices.contains_key(&dev_key) {
            continue;
        }

        let (controller_name, usb_version) = controller_map
            .get(&ctrl_id.to_uppercase())
            .cloned()
            .unwrap_or_else(|| ("Unknown Controller".to_string(), "USB".to_string()));

        let pnp = pnp_by_id.get(&dev_key);
        let name = pnp
            .and_then(|p| p.name.clone())
            .unwrap_or_else(|| dev_id.clone());
        let manufacturer = pnp
            .and_then(|p| p.manufacturer.clone())
            .unwrap_or_else(|| "-".to_string());
        let error_code = pnp.and_then(|p| p.config_manager_error_code).unwrap_or(0);
        let ok = error_code == 0;
        let status = if ok {
            pnp.and_then(|p| p.status.clone())
                .unwrap_or_else(|| "OK".to_string())
        } else {
            format!("Error (Code {error_code})")
        };

        let upper_name = name.to_uppercase();
        let is_hub = upper_name.contains("ROOT HUB")
            || upper_name.contains("USB ROOT")
            || upper_name.contains("GENERIC USB HUB")
            || upper_name.contains("GENERIC SUPERSPEED USB HUB");

        seen_devices.insert(
            dev_key,
            UsbDeviceInfo {
                name,
                device_id: dev_id,
                manufacturer,
                usb_version,
                controller_name,
                status,
                ok,
                is_hub,
            },
        );
    }

    let mut devices: Vec<UsbDeviceInfo> = seen_devices.into_values().collect();
    devices.sort_by(|a, b| a.is_hub.cmp(&b.is_hub).then_with(|| a.name.cmp(&b.name)));

    let usb3_active_devices = devices
        .iter()
        .filter(|d| !d.is_hub && d.usb_version == "USB 3.0")
        .count();
    let usb2_active_devices = devices
        .iter()
        .filter(|d| !d.is_hub && d.usb_version == "USB 2.0")
        .count();

    UsbSummary {
        usb3_controller_count,
        usb2_controller_count,
        usb3_active_devices,
        usb2_active_devices,
        controllers,
        devices,
    }
}

fn build_device_manager_summary(pnp_rows: Vec<WmiPnPEntity>) -> DeviceManagerSummary {
    let mut grouped: BTreeMap<String, Vec<DeviceManagerItem>> = BTreeMap::new();
    let mut total_devices = 0usize;
    let mut ok_count = 0usize;
    let mut error_count = 0usize;

    for row in pnp_rows {
        let pnp_class = row
            .pnp_class
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Other".to_string());
        let error_code = row.config_manager_error_code.unwrap_or(0);
        let ok = error_code == 0;
        let error_label = config_manager_error_label(error_code).to_string();
        let status = if ok {
            row.status.unwrap_or_else(|| "OK".to_string())
        } else {
            format!("Error ({error_code})")
        };

        total_devices += 1;
        if ok {
            ok_count += 1;
        } else {
            error_count += 1;
        }

        grouped.entry(pnp_class.clone()).or_default().push(DeviceManagerItem {
            name: row.name.unwrap_or_else(|| "Unnamed Device".to_string()),
            manufacturer: row.manufacturer.unwrap_or_else(|| "-".to_string()),
            device_id: row.device_id.unwrap_or_else(|| "-".to_string()),
            pnp_class,
            status,
            error_code,
            error_label,
            ok,
        });
    }

    let categories = grouped
        .into_iter()
        .map(|(name, mut devices)| {
            devices.sort_by(|a, b| a.ok.cmp(&b.ok).then_with(|| a.name.cmp(&b.name)));
            let cat_err = devices.iter().filter(|d| !d.ok).count();
            DeviceManagerCategory {
                name,
                total: devices.len(),
                error_count: cat_err,
                devices,
            }
        })
        .collect();

    DeviceManagerSummary {
        total_devices,
        ok_count,
        error_count,
        categories,
    }
}

pub fn classify_gpu(name: &str, vendor: &str) -> (&'static str, bool) {
    let u = format!("{} {}", name, vendor).to_uppercase();
    if u.contains("MICROSOFT BASIC")
        || u.contains("VIRTUAL")
        || u.contains("VMWARE")
        || u.contains("VBOX")
        || u.contains("QXL")
        || u.contains("PARSEC")
        || u.contains("REMOTE")
    {
        return ("Virtual/Other", false);
    }
    if u.contains("NVIDIA")
        || u.contains("GEFORCE")
        || u.contains("RTX")
        || u.contains("GTX")
        || u.contains("QUADRO")
        || u.contains("TESLA")
        || u.contains("RADEON RX")
        || u.contains("RADEON PRO")
        || u.contains("FIREPRO")
        || u.contains("INTEL ARC")
    {
        return ("dGPU", true);
    }
    if u.contains("INTEL")
        || u.contains("UHD")
        || u.contains("IRIS")
        || u.contains("HD GRAPHICS")
        || u.contains("RADEON")
        || u.contains("VEGA")
        || u.contains("AMD")
    {
        return ("iGPU", false);
    }
    ("Virtual/Other", false)
}

pub fn classify_network_adapter(name: &str, conn_id: &str, adapter_type: &str) -> &'static str {
    let u = format!("{name} {conn_id} {adapter_type}").to_uppercase();
    if u.contains("WI-FI") || u.contains("WIFI") || u.contains("WIRELESS") || u.contains("802.11") {
        "Wi-Fi"
    } else if u.contains("ETHERNET")
        || u.contains("LAN")
        || u.contains("GIGABIT")
        || u.contains("802.3")
        || u.contains("REALTEK PCI")
        || u.contains("INTEL(R) ETHERNET")
    {
        "Ethernet/LAN"
    } else {
        "Other"
    }
}

fn map_net_status(code: u16) -> (&'static str, bool) {
    match code {
        0 => ("Disconnected", false),
        1 => ("Connecting", false),
        2 => ("Connected", true),
        3 => ("Disconnecting", false),
        4 => ("Hardware Not Present", false),
        5 => ("Hardware Disabled", false),
        6 => ("Hardware Malfunction", false),
        7 => ("Cable Disconnected", false),
        8 => ("Authenticating", false),
        9 => ("Authentication Succeeded", true),
        10 => ("Authentication Failed", false),
        11 => ("Invalid Address", false),
        12 => ("Credentials Required", false),
        _ => ("Unknown", false),
    }
}

pub fn map_video_output_technology(code: i64) -> (&'static str, bool) {
    match code {
        0 => ("VGA (HD15)", false),
        1 => ("S-Video", false),
        2 => ("Composite", false),
        3 => ("Component", false),
        4 => ("DVI", false),
        5 => ("HDMI", true),
        6 => ("LVDS (Internal)", false),
        8 => ("D-Jpn", false),
        9 => ("SDI", false),
        10 => ("DisplayPort (External)", false),
        11 => ("DisplayPort (Embedded/eDP)", false),
        12 => ("UDI (External)", false),
        13 => ("UDI (Embedded)", false),
        14 => ("SDTV Dongle", false),
        15 => ("Miracast", false),
        2147483648 => ("Internal (Laptop/Built-in)", false),
        _ => ("Other/Unknown", false),
    }
}

pub fn classify_usb_controller(name: &str, device_id: &str) -> &'static str {
    let u = format!("{name} {device_id}").to_uppercase();
    if u.contains("XHCI")
        || u.contains("EXTENSIBLE")
        || u.contains("USB 3.")
        || u.contains("USB3")
        || u.contains("3.0")
        || u.contains("3.1")
        || u.contains("3.2")
    {
        "USB 3.0"
    } else if u.contains("EHCI") || u.contains("ENHANCED") || u.contains("2.0") {
        "USB 2.0"
    } else {
        "USB 1.1"
    }
}

pub fn extract_wmi_device_id(reference: &str) -> Option<String> {
    let marker = "DeviceID=\"";
    let start = reference.find(marker)? + marker.len();
    let rest = &reference[start..];
    let end = rest.rfind('"')?;
    Some(rest[..end].replace("\\\\", "\\"))
}

fn config_manager_error_label(code: u32) -> &'static str {
    match code {
        0 => "OK",
        1 => "Device not configured correctly",
        10 => "Device cannot start",
        12 => "Not enough free resources",
        14 => "Restart required",
        18 => "Reinstall drivers",
        19 => "Registry configuration corrupted",
        21 => "Windows is removing this device",
        22 => "Device is disabled",
        24 => "Device is not present or working",
        28 => "Drivers are not installed",
        29 => "Device disabled in firmware",
        31 => "Device is not working properly",
        32 => "Driver service disabled",
        43 => "Windows stopped device (reported problems)",
        45 => "Hardware device not connected",
        47 => "Safe removal prepared",
        48 => "Software blocked from starting",
        52 => "Driver signature cannot be verified",
        _ => "Device error",
    }
}

fn format_bytes_vram(bytes: u64) -> String {
    if bytes == 0 {
        return "-".to_string();
    }
    let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    if gb >= 1.0 {
        format!("{gb:.2} GB")
    } else {
        let mb = bytes as f64 / (1024.0 * 1024.0);
        format!("{mb:.0} MB")
    }
}

fn format_link_speed(bps: u64, connected: bool) -> String {
    if !connected || bps == 0 || bps > 1_000_000_000_000 {
        return "-".to_string();
    }
    if bps >= 1_000_000_000 {
        format!("{:.1} Gbps", bps as f64 / 1_000_000_000.0)
    } else if bps >= 1_000_000 {
        format!("{:.0} Mbps", bps as f64 / 1_000_000.0)
    } else {
        format!("{:.0} Kbps", bps as f64 / 1_000.0)
    }
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "WINDOWS-PC".to_string())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_gpu() {
        assert_eq!(
            classify_gpu("NVIDIA GeForce RTX 4060 Laptop GPU", "NVIDIA"),
            ("dGPU", true)
        );
        assert_eq!(
            classify_gpu("Intel(R) UHD Graphics", "Intel Corporation"),
            ("iGPU", false)
        );
        assert_eq!(
            classify_gpu("AMD Radeon RX 7800 XT", "Advanced Micro Devices, Inc."),
            ("dGPU", true)
        );
        assert_eq!(
            classify_gpu("Microsoft Basic Display Adapter", "Microsoft"),
            ("Virtual/Other", false)
        );
    }

    #[test]
    fn test_classify_usb_controller() {
        assert_eq!(
            classify_usb_controller("Intel(R) USB 3.10 eXtensible Host Controller - 1.10 (Microsoft)", ""),
            "USB 3.0"
        );
        assert_eq!(
            classify_usb_controller("Standard Enhanced PCI to USB Host Controller", ""),
            "USB 2.0"
        );
        assert_eq!(
            classify_usb_controller("Standard OpenHCD USB Host Controller", ""),
            "USB 1.1"
        );
    }

    #[test]
    fn test_extract_wmi_device_id() {
        let raw = r#"\\DESKTOP-123\root\cimv2:Win32_PnPEntity.DeviceID="USB\\VID_046D&PID_C52B\\5&1A2B3C""#;
        assert_eq!(
            extract_wmi_device_id(raw).as_deref(),
            Some(r"USB\VID_046D&PID_C52B\5&1A2B3C")
        );
    }

    #[test]
    fn test_map_video_output_technology() {
        assert_eq!(map_video_output_technology(5), ("HDMI", true));
        assert_eq!(
            map_video_output_technology(10),
            ("DisplayPort (External)", false)
        );
        assert_eq!(
            map_video_output_technology(2147483648),
            ("Internal (Laptop/Built-in)", false)
        );
    }
}
