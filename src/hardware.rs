//! Hardware Auto-Profiler Engine.
//! Dynamically queries CPU core, cache, and hybrid topology via Win32 `GetLogicalProcessorInformationEx`,
//! maps PCI Display Adapters, NVMe Storage Controllers, Network Interfaces, and USB xHCI Host Controllers.

use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows_sys::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationAll, RelationCache, RelationProcessorCore,
    SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};

pub const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CpuArchClass {
    /// 6-8 Core Single-CCD AMD Processor (Unified L3 Cache)
    AmdSingleCcd,
    /// 12-16 Core Dual-CCD AMD Processor (Multi-CCD / 3D V-Cache)
    AmdDualCcd,
    /// Intel Hybrid Architecture (Performance P-Cores + Efficient E-Cores)
    IntelHybridPE,
    /// Classic Homogeneous Multi-Core Architecture (6+ Cores)
    IntelClassic,
    /// Compact Quad-Core Architecture (4 Physical Cores)
    QuadCoreBudget,
}

impl CpuArchClass {
    pub fn display_name(&self) -> &'static str {
        match self {
            CpuArchClass::AmdSingleCcd => "AMD Single-CCD (Unified L3 Cache)",
            CpuArchClass::AmdDualCcd => "AMD Dual-CCD (Multi-Die L3 Topology)",
            CpuArchClass::IntelHybridPE => "Intel Hybrid (P-Cores + E-Cores)",
            CpuArchClass::IntelClassic => "Homogeneous Multi-Core (6+ Cores)",
            CpuArchClass::QuadCoreBudget => "Compact Quad-Core (4 Cores)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreInfo {
    pub core_index: usize,
    pub mask: u64,
    pub logical_threads: Vec<usize>,
    pub efficiency_class: u8,
    pub l3_group: usize,
}

impl CoreInfo {
    /// Returns the primary hardware thread index (Thread 0 of the physical core).
    pub fn primary_thread(&self) -> usize {
        self.logical_threads.first().copied().unwrap_or(self.core_index)
    }

    pub fn primary_thread_mask(&self) -> u64 {
        1u64 << self.primary_thread()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreIsolationMode {
    /// Dedicated Primary Core + Shared Auxiliary Workers:
    /// - `Core 0`: Reserved for OS Kernel / System ISR & DPC execution
    /// - `dedicated_lol_core_idx`: 100% exclusive to the primary game render thread (Client & Vanguard excluded)
    /// - Remaining non-zero cores: Primary physical threads assigned to game auxiliary workers; background threads handle Client & Vanguard
    #[serde(alias = "RedditSharedWorkers")]
    DedicatedPrimaryCore,
    /// Strict Zero-Overlap Partitioning:
    /// - `Core 0`: Reserved for OS Kernel / System ISR & DPC execution
    /// - Dedicated Game Cores: 100% exclusive to `League of Legends.exe` (physical threads only, SMT disabled, 0% Client/Vanguard overlap)
    /// - Dedicated Background Cores: 100% exclusive to `LeagueClient` & `Vanguard` processes (0% overlap with Game)
    StrictZeroOverlap,
}

impl CoreIsolationMode {
    pub fn label(self) -> &'static str {
        match self {
            CoreIsolationMode::DedicatedPrimaryCore => {
                "Dedicated Primary Core + Shared Workers"
            }
            CoreIsolationMode::StrictZeroOverlap => {
                "Strict Core Partitioning (Zero Overlap)"
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuAffinityPlan {
    pub arch_class: CpuArchClass,
    pub isolation_mode: CoreIsolationMode,
    pub dedicated_lol_core_idx: usize,
    pub game_mask: u64,
    pub game_cpus_lasso: String,
    pub client_mask: u64,
    pub client_cpus_lasso: String,
    pub vanguard_mask: u64,
    pub vanguard_cpus_lasso: String,
    pub gpu_irq_core_idx: usize,
    pub gpu_irq_cpu: usize,
    pub gpu_irq_mask: u64,
    pub usb_primary_irq_core_idx: usize,
    pub usb_primary_irq_cpu: usize,
    pub usb_primary_irq_mask: u64,
    pub usb_secondary_irq_core_idx: usize,
    pub usb_secondary_irq_cpu: usize,
    pub usb_secondary_irq_mask: u64,
    pub nic_irq_core_idx: usize,
    pub nic_irq_cpu: usize,
    pub nic_irq_mask: u64,
    pub short_thread_policy_index: u32,
    pub explanation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    Amd,
    Nvidia,
    Intel,
    Unknown,
}

impl GpuVendor {
    pub fn as_str(&self) -> &'static str {
        match self {
            GpuVendor::Amd => "AMD Radeon",
            GpuVendor::Nvidia => "NVIDIA GeForce",
            GpuVendor::Intel => "Intel Arc / Xe",
            GpuVendor::Unknown => "Standard Display Adapter",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: GpuVendor,
    pub pnp_device_id: String,
    pub driver_class_subkey: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbControllerInfo {
    pub name: String,
    pub pnp_device_id: String,
    pub is_cpu_soc_direct: bool,
    pub connected_peripherals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvmeControllerInfo {
    pub name: String,
    pub pnp_device_id: String,
    pub service: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NicInfo {
    pub name: String,
    pub description: String,
    pub pnp_device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHardwareReport {
    pub os_caption: String,
    pub os_build: u32,
    pub is_windows_11: bool,
    pub cpu_name: String,
    pub physical_cores: usize,
    pub logical_threads: usize,
    pub cores: Vec<CoreInfo>,
    pub cpu_plan: CpuAffinityPlan,
    pub gpu: GpuInfo,
    pub usb_controllers: Vec<UsbControllerInfo>,
    pub nvme_controllers: Vec<NvmeControllerInfo>,
    pub nic: NicInfo,
    pub league_exe_paths: Vec<String>,
}

pub fn mask_to_cpu_list_string(mask: u64) -> String {
    let mut cpus = Vec::new();
    for i in 0..64 {
        if (mask & (1u64 << i)) != 0 {
            cpus.push(i.to_string());
        }
    }
    cpus.join(",")
}

/// Queries Win32 `GetLogicalProcessorInformationEx` for physical cores, efficiency classes, and L3 cache groups.
pub fn query_cpu_topology_win32() -> Vec<CoreInfo> {
    unsafe {
        let mut len: u32 = 0;
        let _ = GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            std::ptr::null_mut(),
            &mut len,
        );
        if len == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; len as usize];
        if GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
            &mut len,
        ) == 0
        {
            return Vec::new();
        }

        // Query RelationCache to detect L3 Cache groups (CCDs on multi-die processors)
        let mut cache_len: u32 = 0;
        let _ = GetLogicalProcessorInformationEx(
            RelationCache,
            std::ptr::null_mut(),
            &mut cache_len,
        );
        let mut l3_domains: Vec<(u64, u32)> = Vec::new();
        if cache_len > 0 {
            let mut cache_buf = vec![0u8; cache_len as usize];
            if GetLogicalProcessorInformationEx(
                RelationCache,
                cache_buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
                &mut cache_len,
            ) != 0
            {
                let mut offset = 0usize;
                while offset < cache_len as usize {
                    let info = &*(cache_buf.as_ptr().add(offset)
                        as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX);
                    if info.Relationship == RelationCache {
                        let cache = &info.Anonymous.Cache;
                        if cache.Level == 3 {
                            let m = cache.Anonymous.GroupMask.Mask as u64;
                            let sz = cache.CacheSize;
                            if m != 0 && !l3_domains.iter().any(|&(existing_m, _)| existing_m == m) {
                                l3_domains.push((m, sz));
                            }
                        }
                    }
                    if info.Size == 0 {
                        break;
                    }
                    offset += info.Size as usize;
                }
            }
        }
        // Sort L3 domains by cache size descending so CCD0 (l3_group = 0) is always the 3D V-Cache CCD on asymmetric X3D chips
        l3_domains.sort_by(|a, b| b.1.cmp(&a.1));
        let l3_masks: Vec<u64> = l3_domains.into_iter().map(|(m, _)| m).collect();

        let mut cores = Vec::new();
        let mut offset = 0usize;
        let mut core_idx = 0usize;
        while offset < len as usize {
            let info =
                &*(buf.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX);
            if info.Relationship == RelationProcessorCore {
                let proc = &info.Anonymous.Processor;
                let mask = proc.GroupMask[0].Mask as u64;
                let mut threads = Vec::new();
                for bit in 0..64 {
                    if (mask & (1u64 << bit)) != 0 {
                        threads.push(bit);
                    }
                }
                let l3_group = l3_masks
                    .iter()
                    .position(|&l3m| (l3m & mask) != 0)
                    .unwrap_or(0);

                cores.push(CoreInfo {
                    core_index: core_idx,
                    mask,
                    logical_threads: threads,
                    efficiency_class: proc.EfficiencyClass,
                    l3_group,
                });
                core_idx += 1;
            }
            if info.Size == 0 {
                break;
            }
            offset += info.Size as usize;
        }
        let _ = RelationAll;
        cores
    }
}

/// Computes the optimal CPU Affinity & IRQ distribution plan for any processor topology.
pub fn compute_optimal_cpu_plan(cores: &[CoreInfo], cpu_name: &str) -> CpuAffinityPlan {
    compute_optimal_cpu_plan_with_mode(
        cores,
        cpu_name,
        None,
        CoreIsolationMode::DedicatedPrimaryCore,
    )
}

/// Computes the optimal CPU Affinity & IRQ plan while isolating a specified physical core index
/// for the primary game thread.
pub fn compute_optimal_cpu_plan_with_preferred_core(
    cores: &[CoreInfo],
    cpu_name: &str,
    preferred_lol_core_idx: Option<usize>,
) -> CpuAffinityPlan {
    compute_optimal_cpu_plan_with_mode(
        cores,
        cpu_name,
        preferred_lol_core_idx,
        CoreIsolationMode::DedicatedPrimaryCore,
    )
}

pub fn compute_optimal_cpu_plan_with_mode(
    cores: &[CoreInfo],
    cpu_name: &str,
    preferred_lol_core_idx: Option<usize>,
    isolation_mode: CoreIsolationMode,
) -> CpuAffinityPlan {
    let name_upper = cpu_name.to_uppercase();
    let is_amd = name_upper.contains("AMD") || name_upper.contains("RYZEN");
    let num_cores = cores.len().max(1);

    let has_hybrid_efficiency = cores.iter().any(|c| c.efficiency_class > 0)
        && cores.iter().any(|c| c.efficiency_class == 0);
    let max_l3_group = cores.iter().map(|c| c.l3_group).max().unwrap_or(0);
    let is_dual_ccd = is_amd && num_cores >= 10 && (max_l3_group >= 1 || num_cores >= 12);

    let arch_class = if num_cores <= 4 {
        CpuArchClass::QuadCoreBudget
    } else if has_hybrid_efficiency {
        CpuArchClass::IntelHybridPE
    } else if is_dual_ccd {
        CpuArchClass::AmdDualCcd
    } else if is_amd {
        CpuArchClass::AmdSingleCcd
    } else {
        CpuArchClass::IntelClassic
    };

    let mut game_mask: u64 = 0;
    let mut client_mask: u64 = 0;
    let short_thread_policy_index: u32;
    let explanation: String;
    let mut dedicated_lol_core_idx = preferred_lol_core_idx
        .filter(|&idx| idx > 0 && idx < num_cores)
        .unwrap_or_else(|| num_cores.saturating_sub(1));

    match arch_class {
        CpuArchClass::IntelHybridPE => {
            let max_eff = cores.iter().map(|c| c.efficiency_class).max().unwrap_or(1);
            let p_cores: Vec<&CoreInfo> =
                cores.iter().filter(|c| c.efficiency_class == max_eff).collect();
            let e_cores: Vec<&CoreInfo> =
                cores.iter().filter(|c| c.efficiency_class < max_eff).collect();

            if let Some(last_p) = p_cores.last() {
                if preferred_lol_core_idx.is_none() {
                    dedicated_lol_core_idx = last_p.core_index;
                }
            }

            for (idx, pc) in p_cores.iter().enumerate() {
                if p_cores.len() > 4 && idx == 0 {
                    continue;
                }
                game_mask |= pc.primary_thread_mask();
            }
            for ec in &e_cores {
                client_mask |= ec.mask;
            }
            if client_mask == 0 {
                for pc in p_cores.iter().skip(1) {
                    if pc.core_index != dedicated_lol_core_idx {
                        client_mask |= pc.mask;
                    }
                }
            }
            short_thread_policy_index = 1;
            explanation = format!(
                "Intel Hybrid topology detected ({} P-Cores + {} E-Cores). \
                 League of Legends.exe is pinned to physical P-Core threads excluding Core 0 (Mask 0x{:04X}), \
                 while client and Vanguard background processes are isolated onto E-Cores (Mask 0x{:04X}).",
                p_cores.len(),
                e_cores.len(),
                game_mask,
                client_mask
            );
        }
        CpuArchClass::AmdDualCcd => {
            let ccd0: Vec<&CoreInfo> = if max_l3_group >= 1 {
                cores.iter().filter(|c| c.l3_group == 0).collect()
            } else {
                cores.iter().take(num_cores / 2).collect()
            };
            let ccd1: Vec<&CoreInfo> = if max_l3_group >= 1 {
                cores.iter().filter(|c| c.l3_group > 0).collect()
            } else {
                cores.iter().skip(num_cores / 2).collect()
            };

            if let Some(last_c0) = ccd0.last() {
                if preferred_lol_core_idx.is_none() {
                    dedicated_lol_core_idx = last_c0.core_index;
                }
            }

            for (idx, c) in ccd0.iter().enumerate() {
                if ccd0.len() >= 6 && idx == 0 {
                    continue;
                }
                game_mask |= c.primary_thread_mask();
            }
            for c in &ccd1 {
                client_mask |= c.mask;
            }
            short_thread_policy_index = if name_upper.contains("X3D") { 1 } else { 0 };
            explanation = format!(
                "Dual-CCD processor topology detected ({} cores, 2 independent L3 cache complexes). \
                 To eliminate cross-CCD interconnect latency, League of Legends.exe is confined to CCD0 physical cores (Mask 0x{:04X}), \
                 while client and Vanguard processes are isolated onto CCD1 (Mask 0x{:04X}).",
                num_cores, game_mask, client_mask
            );
        }
        CpuArchClass::QuadCoreBudget => {
            for c in cores {
                game_mask |= c.primary_thread_mask();
            }
            for c in cores.iter().skip(1) {
                if c.core_index != dedicated_lol_core_idx {
                    client_mask |= c.mask;
                }
            }
            if client_mask == 0 {
                client_mask = game_mask;
            }
            short_thread_policy_index = 0;
            explanation = format!(
                "Quad-core topology detected ({} physical cores). \
                 League of Legends.exe uses all 4 physical threads with SMT disabled (Mask 0x{:04X}), \
                 while client and Vanguard processes are restricted to secondary threads (Mask 0x{:04X}).",
                num_cores, game_mask, client_mask
            );
        }
        CpuArchClass::AmdSingleCcd | CpuArchClass::IntelClassic => {
            match isolation_mode {
                CoreIsolationMode::DedicatedPrimaryCore => {
                    for (idx, c) in cores.iter().enumerate() {
                        if idx == 0 {
                            continue;
                        }
                        game_mask |= c.primary_thread_mask();
                        if c.core_index != dedicated_lol_core_idx {
                            client_mask |= c.mask;
                        }
                    }
                    short_thread_policy_index = 0;
                    explanation = format!(
                        "Dedicated Primary Core Mode: Core #{} is reserved exclusively for the primary game thread (0% client/Vanguard contention). Remaining non-zero physical threads assist auxiliary game workers, while client and Vanguard processes are excluded from Core 0 and Core #{}.",
                        dedicated_lol_core_idx, dedicated_lol_core_idx
                    );
                }
                CoreIsolationMode::StrictZeroOverlap => {
                    let target_game_cores_count = ((num_cores.saturating_sub(1) + 1) / 2).max(2);
                    let mut game_core_indices = vec![dedicated_lol_core_idx];
                    for c in cores.iter().skip(1) {
                        if game_core_indices.len() >= target_game_cores_count {
                            break;
                        }
                        if !game_core_indices.contains(&c.core_index) {
                            game_core_indices.push(c.core_index);
                        }
                    }

                    for c in cores.iter().skip(1) {
                        if game_core_indices.contains(&c.core_index) {
                            game_mask |= c.primary_thread_mask();
                        } else {
                            client_mask |= c.mask;
                        }
                    }
                    short_thread_policy_index = 0;
                    explanation = format!(
                        "Strict Core Partitioning (Zero Overlap): League of Legends.exe is isolated onto physical cores {:?} (CPUs [{}], SMT disabled), while client and Vanguard processes are confined to separate background cores (CPUs [{}]).",
                        game_core_indices,
                        mask_to_cpu_list_string(game_mask),
                        mask_to_cpu_list_string(client_mask)
                    );
                }
            }
        }
    }

    let candidate_irq_cores: Vec<&CoreInfo> = cores
        .iter()
        .filter(|c| c.core_index > 0 && c.core_index != dedicated_lol_core_idx)
        .collect();

    let gpu_core = candidate_irq_cores
        .first()
        .copied()
        .or_else(|| cores.get(1))
        .or_else(|| cores.first())
        .unwrap();
    let usb1_core = candidate_irq_cores
        .get(1)
        .copied()
        .or_else(|| candidate_irq_cores.first().copied())
        .or_else(|| cores.get(2))
        .or_else(|| cores.first())
        .unwrap();
    let usb2_core = candidate_irq_cores
        .get(2)
        .copied()
        .or_else(|| candidate_irq_cores.get(1).copied())
        .or_else(|| cores.get(3))
        .or_else(|| cores.first())
        .unwrap();
    let nic_core = candidate_irq_cores
        .get(2)
        .copied()
        .or_else(|| candidate_irq_cores.last().copied())
        .unwrap_or(usb2_core);

    CpuAffinityPlan {
        arch_class,
        isolation_mode,
        dedicated_lol_core_idx,
        game_mask,
        game_cpus_lasso: mask_to_cpu_list_string(game_mask),
        client_mask,
        client_cpus_lasso: mask_to_cpu_list_string(client_mask),
        vanguard_mask: client_mask,
        vanguard_cpus_lasso: mask_to_cpu_list_string(client_mask),
        gpu_irq_core_idx: gpu_core.core_index,
        gpu_irq_cpu: gpu_core.primary_thread(),
        gpu_irq_mask: gpu_core.primary_thread_mask(),
        usb_primary_irq_core_idx: usb1_core.core_index,
        usb_primary_irq_cpu: usb1_core.primary_thread(),
        usb_primary_irq_mask: usb1_core.primary_thread_mask(),
        usb_secondary_irq_core_idx: usb2_core.core_index,
        usb_secondary_irq_cpu: usb2_core.primary_thread(),
        usb_secondary_irq_mask: usb2_core.primary_thread_mask(),
        nic_irq_core_idx: nic_core.core_index,
        nic_irq_cpu: nic_core.primary_thread(),
        nic_irq_mask: nic_core.primary_thread_mask(),
        short_thread_policy_index,
        explanation,
    }
}

impl SystemHardwareReport {
    pub fn scan_system() -> Self {
        let mut cores = query_cpu_topology_win32();
        if cores.is_empty() {
            for i in 0..6 {
                let t0 = i * 2;
                let t1 = i * 2 + 1;
                cores.push(CoreInfo {
                    core_index: i,
                    mask: (1u64 << t0) | (1u64 << t1),
                    logical_threads: vec![t0, t1],
                    efficiency_class: 0,
                    l3_group: 0,
                });
            }
        }

        let ps_script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption, BuildNumber
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1 Name
$gpu = Get-CimInstance Win32_VideoController | Where-Object { $_.PNPDeviceID -like 'PCI\*' } | Select-Object -First 1 Name, PNPDeviceID
$nic = Get-NetAdapter | Where-Object Status -eq 'Up' | Select-Object -First 1 Name, InterfaceDescription, PnPDeviceId
$usbs = @(Get-CimInstance Win32_USBController | Where-Object { $_.PNPDeviceID -like 'PCI\*' } | Select-Object Name, PNPDeviceID)
$nvmes = @(Get-CimInstance Win32_PnPEntity | Where-Object { $_.PNPDeviceID -like 'PCI\*' -and ($_.Service -eq 'stornvme' -or $_.Service -eq 'nvme' -or $_.Name -match 'NVMe|NVM Express') } | Select-Object Name, PNPDeviceID, Service)

$peripherals = @{}
Get-PnpDevice -Class Mouse, Keyboard -Status OK | ForEach-Object {
    $dev = $_
    $cur = $dev.InstanceId
    $busReported = $null
    while ($true) {
        $parent = (Get-PnpDeviceProperty -InstanceId $cur -KeyName 'DEVPKEY_Device_Parent').Data
        if (-not $parent) { break }
        if (-not $busReported -and $parent -like 'USB\*') {
            $busReported = (Get-PnpDeviceProperty -InstanceId $parent -KeyName 'DEVPKEY_Device_BusReportedDeviceDesc').Data
        }
        if ($parent -like 'PCI\VEN_*') {
            $key = $parent.ToUpper()
            if (-not $peripherals.ContainsKey($key)) { $peripherals[$key] = @() }
            $prodName = if ($busReported) { $busReported.Trim() } else { $dev.FriendlyName }
            if ($peripherals[$key] -notcontains $prodName) {
                $peripherals[$key] += $prodName
            }
            break
        }
        $cur = $parent
    }
}

$usbList = @()
foreach ($u in $usbs) {
    $idUpper = $u.PNPDeviceID.ToUpper()
    $attached = if ($peripherals.ContainsKey($idUpper)) { $peripherals[$idUpper] } else { @() }
    $usbList += @{
        name = [string]$u.Name
        pnp_id = [string]$u.PNPDeviceID
        peripherals = @($attached)
    }
}

$nvmeList = @()
foreach ($n in $nvmes) {
    $nvmeList += @{
        name = [string]$n.Name
        pnp_id = [string]$n.PNPDeviceID
        service = [string]$n.Service
    }
}

[PSCustomObject]@{
    os_caption = [string]$os.Caption
    os_build = [int]$os.BuildNumber
    cpu_name = [string]$cpu.Name
    gpu_name = [string]$gpu.Name
    gpu_pnp = [string]$gpu.PNPDeviceID
    nic_name = [string]$nic.Name
    nic_desc = [string]$nic.InterfaceDescription
    nic_pnp = [string]$nic.PnPDeviceId
    usb_controllers = $usbList
    nvme_controllers = $nvmeList
} | ConvertTo-Json -Depth 4 -Compress
"#;

        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let mut os_caption = "Microsoft Windows".to_string();
        let mut os_build = 26100u32;
        let mut cpu_name = "Multi-Core Processor".to_string();
        let mut gpu_name = "PCI Express Display Adapter".to_string();
        let mut gpu_pnp = String::new();
        let mut nic_name = "Ethernet".to_string();
        let mut nic_desc = "Network Interface Controller".to_string();
        let mut nic_pnp = String::new();
        let mut usb_controllers = Vec::new();
        let mut nvme_controllers = Vec::new();

        if let Ok(out) = output {
            if let Ok(text) = String::from_utf8(out.stdout) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(text.trim()) {
                    if let Some(s) = val.get("os_caption").and_then(|v| v.as_str()) {
                        if !s.is_empty() {
                            os_caption = s.trim().to_string();
                        }
                    }
                    if let Some(b) = val.get("os_build").and_then(|v| v.as_u64()) {
                        if b > 0 {
                            os_build = b as u32;
                        }
                    }
                    if let Some(s) = val.get("cpu_name").and_then(|v| v.as_str()) {
                        if !s.is_empty() {
                            cpu_name = s.trim().to_string();
                        }
                    }
                    if let Some(s) = val.get("gpu_name").and_then(|v| v.as_str()) {
                        if !s.is_empty() {
                            gpu_name = s.trim().to_string();
                        }
                    }
                    if let Some(s) = val.get("gpu_pnp").and_then(|v| v.as_str()) {
                        gpu_pnp = s.trim().to_string();
                    }
                    if let Some(s) = val.get("nic_name").and_then(|v| v.as_str()) {
                        if !s.is_empty() {
                            nic_name = s.trim().to_string();
                        }
                    }
                    if let Some(s) = val.get("nic_desc").and_then(|v| v.as_str()) {
                        if !s.is_empty() {
                            nic_desc = s.trim().to_string();
                        }
                    }
                    if let Some(s) = val.get("nic_pnp").and_then(|v| v.as_str()) {
                        nic_pnp = s.trim().to_string();
                    }
                    if let Some(arr) = val.get("usb_controllers").and_then(|v| v.as_array()) {
                        for item in arr {
                            let uname = item
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("USB xHCI Host Controller")
                                .to_string();
                            let upnp = item
                                .get("pnp_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let mut periphs = Vec::new();
                            if let Some(pa) = item.get("peripherals").and_then(|v| v.as_array()) {
                                for p in pa {
                                    if let Some(ps) = p.as_str() {
                                        periphs.push(ps.to_string());
                                    }
                                }
                            }
                            let is_cpu_soc = detect_usb_soc_direct(&upnp, &uname);
                            usb_controllers.push(UsbControllerInfo {
                                name: uname,
                                pnp_device_id: upnp,
                                is_cpu_soc_direct: is_cpu_soc,
                                connected_peripherals: periphs,
                            });
                        }
                    }
                    if let Some(arr) = val.get("nvme_controllers").and_then(|v| v.as_array()) {
                        for item in arr {
                            let nname = item
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Standard NVM Express Controller")
                                .to_string();
                            let npnp = item
                                .get("pnp_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let nsvc = item
                                .get("service")
                                .and_then(|v| v.as_str())
                                .unwrap_or("stornvme")
                                .to_string();
                            if !npnp.is_empty() {
                                nvme_controllers.push(NvmeControllerInfo {
                                    name: nname,
                                    pnp_device_id: npnp,
                                    service: nsvc,
                                });
                            }
                        }
                    }
                }
            }
        }

        usb_controllers.sort_by_key(|u| {
            let has_periph = !u.connected_peripherals.is_empty();
            (!has_periph, !u.is_cpu_soc_direct)
        });

        let vendor = detect_gpu_vendor_dynamic(&gpu_pnp, &gpu_name);
        let driver_class_subkey = find_active_gpu_class_subkey(&gpu_name, &gpu_pnp);
        let physical_cores = cores.len();
        let logical_threads = cores.iter().map(|c| c.logical_threads.len()).sum();
        let default_preferred_core = if physical_cores == 6 {
            Some(3)
        } else {
            None
        };
        let cpu_plan =
            compute_optimal_cpu_plan_with_preferred_core(&cores, &cpu_name, default_preferred_core);

        let league_exe_paths = detect_league_exe_paths();

        SystemHardwareReport {
            os_caption,
            os_build,
            is_windows_11: os_build >= 22000,
            cpu_name,
            physical_cores,
            logical_threads,
            cores,
            cpu_plan,
            gpu: GpuInfo {
                name: gpu_name,
                vendor,
                pnp_device_id: gpu_pnp,
                driver_class_subkey,
            },
            usb_controllers,
            nvme_controllers,
            nic: NicInfo {
                name: nic_name,
                description: nic_desc,
                pnp_device_id: nic_pnp,
            },
            league_exe_paths,
        }
    }
}

/// Dynamically classifies GPU vendor from PCI hardware IDs or driver description strings.
pub fn detect_gpu_vendor_dynamic(pnp_id: &str, gpu_name: &str) -> GpuVendor {
    let pnp_u = pnp_id.to_uppercase();
    let name_u = gpu_name.to_uppercase();
    if pnp_u.contains("VEN_1002")
        || name_u.contains("AMD")
        || name_u.contains("RADEON")
        || name_u.contains("ADVANCED MICRO DEVICES")
    {
        GpuVendor::Amd
    } else if pnp_u.contains("VEN_10DE")
        || name_u.contains("NVIDIA")
        || name_u.contains("GEFORCE")
        || name_u.contains("QUADRO")
        || name_u.contains("RTX")
        || name_u.contains("GTX")
    {
        GpuVendor::Nvidia
    } else if pnp_u.contains("VEN_8086")
        || name_u.contains("INTEL")
        || name_u.contains("ARC")
        || name_u.contains("UHD")
        || name_u.contains("IRIS")
    {
        GpuVendor::Intel
    } else {
        GpuVendor::Unknown
    }
}

/// Heuristic to distinguish CPU-integrated SoC USB xHCI controllers from chipset-routed controllers.
fn detect_usb_soc_direct(pnp_id: &str, name: &str) -> bool {
    let pnp_u = pnp_id.to_uppercase();
    let name_u = name.to_uppercase();
    // Chipset USB controllers commonly expose chipset series identifiers or DEV_43xx on AMD PCH
    if pnp_u.contains("DEV_43") || name_u.contains("CHIPSET") {
        return false;
    }
    pnp_u.starts_with("PCI\\")
}

/// Dynamically locates the active display adapter driver instance (`000X`) under
/// `HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}`
/// by matching `MatchingDeviceId` against the PCI `VEN_xxxx&DEV_xxxx` token or `DriverDesc`.
pub fn find_active_gpu_class_subkey(gpu_name: &str, gpu_pnp: &str) -> String {
    use winreg::enums::*;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let class_path = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

    // Extract `VEN_xxxx&DEV_xxxx` from `PCI\VEN_1002&DEV_731F&SUBSYS_...`
    let ven_dev_token = gpu_pnp
        .split('\\')
        .nth(1)
        .and_then(|s| {
            let parts: Vec<&str> = s.split('&').collect();
            if parts.len() >= 2 {
                Some(format!("{}&{}", parts[0], parts[1]).to_lowercase())
            } else {
                None
            }
        })
        .unwrap_or_default();

    if let Ok(class_key) = hklm.open_subkey_with_flags(class_path, KEY_READ) {
        let mut fallback_match: Option<String> = None;
        for sub in class_key.enum_keys().flatten() {
            if sub.len() != 4 || !sub.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if let Ok(sk) = class_key.open_subkey_with_flags(&sub, KEY_READ) {
                let matching_id: String = sk
                    .get_value::<String, _>("MatchingDeviceId")
                    .unwrap_or_default()
                    .to_lowercase();
                let desc: String = sk
                    .get_value::<String, _>("DriverDesc")
                    .unwrap_or_default()
                    .to_lowercase();

                if !ven_dev_token.is_empty() && matching_id.contains(&ven_dev_token) {
                    return sub;
                }
                if !gpu_name.is_empty() && desc.contains(&gpu_name.to_lowercase()) {
                    fallback_match = Some(sub);
                }
            }
        }
        if let Some(found) = fallback_match {
            return found;
        }
    }
    "0000".to_string()
}

fn detect_league_exe_paths() -> Vec<String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let mut paths = Vec::new();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) =
        hkcu.open_subkey_with_flags(r"Software\Microsoft\DirectX\UserGpuPreferences", KEY_READ)
    {
        for (name, _) in key.enum_values().flatten() {
            if name.to_lowercase().ends_with("league of legends.exe") {
                paths.push(name);
            }
        }
    }
    for candidate in [
        r"D:\Riot Games\League of Legends\Game\League of Legends.exe",
        r"C:\Riot Games\League of Legends\Game\League of Legends.exe",
    ] {
        if std::path::Path::new(candidate).exists()
            && !paths.iter().any(|p| p.eq_ignore_ascii_case(candidate))
        {
            paths.push(candidate.to_string());
        }
    }
    if paths.is_empty() {
        paths.push(r"C:\Riot Games\League of Legends\Game\League of Legends.exe".to_string());
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_smt_cores(count: usize, l3_groups: usize) -> Vec<CoreInfo> {
        let mut v = Vec::new();
        let cores_per_group = count / l3_groups.max(1);
        for i in 0..count {
            let t0 = i * 2;
            let t1 = i * 2 + 1;
            v.push(CoreInfo {
                core_index: i,
                mask: (1u64 << t0) | (1u64 << t1),
                logical_threads: vec![t0, t1],
                efficiency_class: 0,
                l3_group: i / cores_per_group.max(1),
            });
        }
        v
    }

    #[test]
    fn test_ryzen_5_5600_6c_12t_plan() {
        let cores = make_smt_cores(6, 1);
        let plan = compute_optimal_cpu_plan(&cores, "AMD Ryzen 5 5600 6-Core Processor");
        assert_eq!(plan.arch_class, CpuArchClass::AmdSingleCcd);
        assert_eq!(plan.game_mask, 0x0554);
        assert_eq!(plan.game_cpus_lasso, "2,4,6,8,10");
        assert_eq!(plan.client_mask, 0x03FC);
        assert_eq!(plan.vanguard_mask, 0x03FC);
        assert_eq!(plan.short_thread_policy_index, 0);
    }

    #[test]
    fn test_ryzen_5_5600_preferred_core_3_isolation() {
        let cores = make_smt_cores(6, 1);
        let plan = compute_optimal_cpu_plan_with_preferred_core(
            &cores,
            "AMD Ryzen 5 5600 6-Core Processor",
            Some(3),
        );
        assert_eq!(plan.dedicated_lol_core_idx, 3);
        assert_eq!(plan.game_mask, 0x0554);
        assert_eq!(plan.client_mask, 0x0F3C);
        assert_eq!(plan.vanguard_mask, 0x0F3C);
        assert_eq!(plan.gpu_irq_core_idx, 1);
        assert_eq!(plan.usb_primary_irq_core_idx, 2);
        assert_eq!(plan.nic_irq_core_idx, 4);
    }

    #[test]
    fn test_ryzen_5_5600_strict_zero_overlap_mode() {
        let cores = make_smt_cores(6, 1);
        let plan = compute_optimal_cpu_plan_with_mode(
            &cores,
            "AMD Ryzen 5 5600 6-Core Processor",
            Some(3),
            CoreIsolationMode::StrictZeroOverlap,
        );
        assert_eq!(plan.isolation_mode, CoreIsolationMode::StrictZeroOverlap);
        assert_eq!(plan.dedicated_lol_core_idx, 3);
        assert_eq!(plan.game_mask, 0x0054);
        assert_eq!(plan.client_mask, 0x0F00);
        assert_eq!(plan.vanguard_mask, 0x0F00);
        assert_eq!(plan.game_mask & plan.client_mask, 0);
    }

    #[test]
    fn test_ryzen_7_5800x3d_8c_16t_plan() {
        let cores = make_smt_cores(8, 1);
        let plan = compute_optimal_cpu_plan(&cores, "AMD Ryzen 7 5800X3D 8-Core Processor");
        assert_eq!(plan.arch_class, CpuArchClass::AmdSingleCcd);
        assert_eq!(plan.game_mask, 0x5554);
        assert_eq!(plan.client_mask, 0x3FFC);
    }

    #[test]
    fn test_ryzen_9_7950x3d_dual_ccd_plan() {
        let cores = make_smt_cores(16, 2);
        let plan = compute_optimal_cpu_plan(&cores, "AMD Ryzen 9 7950X3D 16-Core Processor");
        assert_eq!(plan.arch_class, CpuArchClass::AmdDualCcd);
        assert_eq!(plan.game_mask, 0x5554);
        assert_eq!(plan.client_mask, 0xFFFF_0000);
        assert_eq!(plan.short_thread_policy_index, 1);
    }

    #[test]
    fn test_intel_hybrid_i5_13600k_pe_plan() {
        let mut cores = Vec::new();
        for i in 0..6 {
            let t0 = i * 2;
            let t1 = i * 2 + 1;
            cores.push(CoreInfo {
                core_index: i,
                mask: (1u64 << t0) | (1u64 << t1),
                logical_threads: vec![t0, t1],
                efficiency_class: 1,
                l3_group: 0,
            });
        }
        for e in 0..8 {
            let t = 12 + e;
            cores.push(CoreInfo {
                core_index: 6 + e,
                mask: 1u64 << t,
                logical_threads: vec![t],
                efficiency_class: 0,
                l3_group: 0,
            });
        }
        let plan = compute_optimal_cpu_plan(&cores, "13th Gen Intel(R) Core(TM) i5-13600K");
        assert_eq!(plan.arch_class, CpuArchClass::IntelHybridPE);
        assert_eq!(plan.game_mask, 0x0554);
        assert_eq!(plan.client_mask, 0xFF000);
        assert_eq!(plan.vanguard_mask, 0xFF000);
        assert_eq!(plan.short_thread_policy_index, 1);
    }

    #[test]
    fn test_quad_core_i3_12100f_plan() {
        let cores = make_smt_cores(4, 1);
        let plan = compute_optimal_cpu_plan(&cores, "12th Gen Intel(R) Core(TM) i3-12100F");
        assert_eq!(plan.arch_class, CpuArchClass::QuadCoreBudget);
        assert_eq!(plan.game_mask, 0x0055);
    }
}
