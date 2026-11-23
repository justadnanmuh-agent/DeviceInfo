use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use sysinfo::{
    CpuRefreshKind, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind, ProcessesToUpdate,
    System,
};

#[derive(Debug, Clone, Serialize)]
pub struct TaskManagerReport {
    pub uptime_secs: u64,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub disks: Vec<DiskMetrics>,
    pub total_disk_read_bytes_per_sec: f64,
    pub total_disk_write_bytes_per_sec: f64,
    pub networks: Vec<NetworkMetrics>,
    pub total_rx_bytes_per_sec: f64,
    pub total_tx_bytes_per_sec: f64,
    pub process_count: usize,
    pub processes: Vec<ProcessInfo>,
    pub process_groups: Vec<ProcessGroupInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CpuMetrics {
    pub usage_percent: f32,
    pub frequency_mhz: u64,
    pub logical_cores: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryMetrics {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskMetrics {
    pub mount_point: String,
    pub name: String,
    pub file_system: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
    pub read_bytes_per_sec: f64,
    pub write_bytes_per_sec: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkMetrics {
    pub name: String,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub memory_mb: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessGroupInfo {
    pub name: String,
    pub count: usize,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub memory_mb: f64,
    pub protected: bool,
}

pub struct TaskManagerState {
    sys: System,
    disks: Disks,
    networks: Networks,
    last_refresh: Instant,
    cached: Option<TaskManagerReport>,
}

impl TaskManagerState {
    fn cpu_refresh_kind() -> CpuRefreshKind {
        CpuRefreshKind::nothing().with_cpu_usage().with_frequency()
    }

    fn mem_refresh_kind() -> MemoryRefreshKind {
        MemoryRefreshKind::nothing().with_ram()
    }

    fn proc_refresh_kind() -> ProcessRefreshKind {
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
    }

    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_specifics(Self::cpu_refresh_kind());
        sys.refresh_memory_specifics(Self::mem_refresh_kind());
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, Self::proc_refresh_kind());

        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();

        Self {
            sys,
            disks,
            networks,
            last_refresh: Instant::now(),
            cached: None,
        }
    }

    pub fn snapshot(&mut self) -> TaskManagerReport {
        let now = Instant::now();
        if let Some(prev) = &self.cached {
            if now.duration_since(self.last_refresh) < Duration::from_millis(500) {
                return prev.clone();
            }
        }

        let elapsed_secs = now
            .duration_since(self.last_refresh)
            .as_secs_f64()
            .max(0.1);
        self.last_refresh = now;

        self.sys.refresh_cpu_specifics(Self::cpu_refresh_kind());
        self.sys.refresh_memory_specifics(Self::mem_refresh_kind());
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, Self::proc_refresh_kind());
        self.disks.refresh(true);
        self.networks.refresh(true);

        let cpus = self.sys.cpus();
        let logical_cores = cpus.len().max(1);
        let avg_freq = if cpus.is_empty() {
            0
        } else {
            cpus.iter().map(|c| c.frequency()).sum::<u64>() / cpus.len() as u64
        };

        let cpu = CpuMetrics {
            usage_percent: self.sys.global_cpu_usage(),
            frequency_mhz: avg_freq,
            logical_cores,
        };

        let total_mem = self.sys.total_memory();
        let used_mem = self.sys.used_memory();
        let available_mem = self.sys.available_memory();
        let mem_pct = if total_mem > 0 {
            (used_mem as f64 / total_mem as f64) * 100.0
        } else {
            0.0
        };

        let memory = MemoryMetrics {
            total_bytes: total_mem,
            used_bytes: used_mem,
            available_bytes: available_mem,
            usage_percent: mem_pct,
        };

        let mut total_disk_read_bytes_per_sec = 0.0;
        let mut total_disk_write_bytes_per_sec = 0.0;
        let disks: Vec<DiskMetrics> = self
            .disks
            .list()
            .iter()
            .map(|d| {
                let total = d.total_space();
                let avail = d.available_space();
                let used = total.saturating_sub(avail);
                let usage_pct = if total > 0 {
                    (used as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                let io = d.usage();
                let read_bps = io.read_bytes as f64 / elapsed_secs;
                let write_bps = io.written_bytes as f64 / elapsed_secs;
                total_disk_read_bytes_per_sec += read_bps;
                total_disk_write_bytes_per_sec += write_bps;
                DiskMetrics {
                    mount_point: d.mount_point().to_string_lossy().into_owned(),
                    name: d.name().to_string_lossy().into_owned(),
                    file_system: d.file_system().to_string_lossy().into_owned(),
                    total_bytes: total,
                    used_bytes: used,
                    available_bytes: avail,
                    usage_percent: usage_pct,
                    read_bytes_per_sec: read_bps,
                    write_bytes_per_sec: write_bps,
                }
            })
            .collect();

        // Fallback to aggregate process disk I/O if volume-level counters are 0
        let mut proc_disk_read = 0.0;
        let mut proc_disk_write = 0.0;

        let all_procs: Vec<ProcessInfo> = self
            .sys
            .processes()
            .values()
            .map(|p| {
                let io = p.disk_usage();
                proc_disk_read += io.read_bytes as f64 / elapsed_secs;
                proc_disk_write += io.written_bytes as f64 / elapsed_secs;
                let mem_bytes = p.memory();
                ProcessInfo {
                    pid: p.pid().as_u32(),
                    name: p.name().to_string_lossy().into_owned(),
                    cpu_percent: p.cpu_usage() / logical_cores as f32,
                    memory_bytes: mem_bytes,
                    memory_mb: mem_bytes as f64 / (1024.0 * 1024.0),
                }
            })
            .collect();

        if total_disk_read_bytes_per_sec == 0.0 && proc_disk_read > 0.0 {
            total_disk_read_bytes_per_sec = proc_disk_read;
        }
        if total_disk_write_bytes_per_sec == 0.0 && proc_disk_write > 0.0 {
            total_disk_write_bytes_per_sec = proc_disk_write;
        }

        let process_count = all_procs.len();
        let process_groups = group_processes(&all_procs, 40);
        let processes = select_top_processes(all_procs, 50);

        let mut total_rx_bytes_per_sec = 0.0;
        let mut total_tx_bytes_per_sec = 0.0;
        let mut networks: Vec<NetworkMetrics> = self
            .networks
            .iter()
            .filter_map(|(name, data)| {
                if name.to_lowercase().contains("loopback") {
                    return None;
                }
                let rx = data.received() as f64 / elapsed_secs;
                let tx = data.transmitted() as f64 / elapsed_secs;
                total_rx_bytes_per_sec += rx;
                total_tx_bytes_per_sec += tx;
                Some(NetworkMetrics {
                    name: name.clone(),
                    rx_bytes_per_sec: rx,
                    tx_bytes_per_sec: tx,
                })
            })
            .collect();
        networks.sort_by(|a, b| {
            (b.rx_bytes_per_sec + b.tx_bytes_per_sec)
                .partial_cmp(&(a.rx_bytes_per_sec + a.tx_bytes_per_sec))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let report = TaskManagerReport {
            uptime_secs: System::uptime(),
            cpu,
            memory,
            disks,
            total_disk_read_bytes_per_sec,
            total_disk_write_bytes_per_sec,
            networks,
            total_rx_bytes_per_sec,
            total_tx_bytes_per_sec,
            process_count,
            processes,
            process_groups,
        };
        self.cached = Some(report.clone());
        report
    }
}

pub fn group_processes(all: &[ProcessInfo], limit: usize) -> Vec<ProcessGroupInfo> {
    let mut map: HashMap<String, (usize, f32, u64)> = HashMap::new();
    for p in all {
        let entry = map.entry(p.name.clone()).or_insert((0, 0.0, 0));
        entry.0 += 1;
        entry.1 += p.cpu_percent;
        entry.2 = entry.2.saturating_add(p.memory_bytes);
    }

    let mut groups: Vec<ProcessGroupInfo> = map
        .into_iter()
        .map(|(name, (count, cpu_percent, memory_bytes))| {
            let protected = crate::optimize::is_protected_process(&name);
            ProcessGroupInfo {
                name,
                count,
                cpu_percent,
                memory_bytes,
                memory_mb: memory_bytes as f64 / (1024.0 * 1024.0),
                protected,
            }
        })
        .collect();

    groups.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
    groups.truncate(limit);
    groups
}

/// Returns a deduplicated union of the top `limit` processes by CPU and top `limit` by memory.
pub fn select_top_processes(all: Vec<ProcessInfo>, limit: usize) -> Vec<ProcessInfo> {
    let mut by_cpu = all.clone();
    by_cpu.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut by_mem = all;
    by_mem.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));

    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(limit * 2);

    for p in by_cpu.into_iter().take(limit).chain(by_mem.into_iter().take(limit)) {
        if seen.insert(p.pid) {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_top_processes_dedup() {
        let procs = vec![
            ProcessInfo { pid: 1, name: "a".into(), cpu_percent: 50.0, memory_bytes: 100, memory_mb: 0.1 },
            ProcessInfo { pid: 2, name: "b".into(), cpu_percent: 10.0, memory_bytes: 900, memory_mb: 0.9 },
            ProcessInfo { pid: 3, name: "c".into(), cpu_percent: 5.0, memory_bytes: 50, memory_mb: 0.05 },
        ];
        let top = select_top_processes(procs, 1);
        assert_eq!(top.len(), 2);
        assert!(top.iter().any(|p| p.pid == 1));
        assert!(top.iter().any(|p| p.pid == 2));
    }
}
