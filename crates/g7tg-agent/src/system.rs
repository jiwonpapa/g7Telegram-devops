//! 저비용 시스템 상태 collector입니다.

use g7tg_core::{DiskSnapshot, SystemSnapshot};
use sysinfo::{Disks, System};

/// 현재 OS와 실제 disk snapshot을 수집합니다.
#[must_use]
pub fn collect(server_name: &str) -> SystemSnapshot {
    let mut system = System::new_all();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    system.refresh_cpu_usage();
    let disks = Disks::new_with_refreshed_list()
        .iter()
        .filter(|disk| disk.total_space() > 0)
        .map(|disk| DiskSnapshot {
            mount_point: disk.mount_point().to_string_lossy().into_owned(),
            total_bytes: disk.total_space(),
            available_bytes: disk.available_space(),
        })
        .collect();
    let load = System::load_average();
    SystemSnapshot {
        server_name: server_name.to_owned(),
        hostname: System::host_name().unwrap_or_else(|| "unknown".to_owned()),
        os_name: System::long_os_version().unwrap_or_else(|| "unknown".to_owned()),
        kernel_version: System::kernel_version().unwrap_or_else(|| "unknown".to_owned()),
        uptime_seconds: System::uptime(),
        cpu_usage_percent: system.global_cpu_usage(),
        logical_cpu_count: u32::try_from(system.cpus().len())
            .unwrap_or(u32::MAX)
            .max(1),
        load_one: load.one,
        memory_total_bytes: system.total_memory(),
        memory_used_bytes: system.used_memory(),
        swap_total_bytes: system.total_swap(),
        swap_used_bytes: system.used_swap(),
        disks,
    }
}
