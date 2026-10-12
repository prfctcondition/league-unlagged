//! CPU Scheduling, PowerCfg Short Thread Policy, Process Lasso Configuration Engine,
//! IFEO Kernel Priority Manager, and 0.505ms Kernel Timer Resolution Controller.

use crate::hardware::{CpuAffinityPlan, CREATE_NO_WINDOW};
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

#[repr(C)]
struct ProcessPowerThrottlingState {
    version: u32,
    control_mask: u32,
    state_mask: u32,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryTimerResolution(
        minimum_resolution: *mut u32,
        maximum_resolution: *mut u32,
        current_resolution: *mut u32,
    ) -> i32;
    fn NtSetTimerResolution(
        desired_resolution: u32,
        set_resolution: u8,
        current_resolution: *mut u32,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn SetProcessInformation(
        h_process: *mut std::ffi::c_void,
        process_information_class: i32,
        process_information: *const std::ffi::c_void,
        process_information_size: u32,
    ) -> i32;
}

const SET_TIMER_RESOLUTION_SERVICE_CS: &str = r#"using System;
using System.Runtime.InteropServices;
using System.ServiceProcess;

public class SetTimerResolutionService : ServiceBase
{
    [DllImport("ntdll.dll", SetLastError = true)]
    private static extern int NtQueryTimerResolution(out uint minimumResolution, out uint maximumResolution, out uint currentResolution);

    [DllImport("ntdll.dll", SetLastError = true)]
    private static extern int NtSetTimerResolution(uint desiredResolution, bool setResolution, out uint currentResolution);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetProcessInformation(IntPtr hProcess, int processInformationClass, ref PROCESS_POWER_THROTTLING_STATE processInformation, uint processInformationSize);

    [StructLayout(LayoutKind.Sequential)]
    private struct PROCESS_POWER_THROTTLING_STATE
    {
        public uint Version;
        public uint ControlMask;
        public uint StateMask;
    }

    private uint targetResolution = 5050;

    public SetTimerResolutionService(uint res)
    {
        this.ServiceName = "SetTimerResolutionService";
        this.CanStop = true;
        this.CanPauseAndContinue = false;
        this.AutoLog = false;
        this.targetResolution = res;
    }

    private static void DisablePowerThrottling()
    {
        try
        {
            PROCESS_POWER_THROTTLING_STATE state = new PROCESS_POWER_THROTTLING_STATE();
            state.Version = 1;
            state.ControlMask = 0x5;
            state.StateMask = 0;
            SetProcessInformation(GetCurrentProcess(), 4, ref state, (uint)Marshal.SizeOf(state));
        }
        catch { }
    }

    protected override void OnStart(string[] args)
    {
        DisablePowerThrottling();
        uint cur;
        NtSetTimerResolution(this.targetResolution, true, out cur);
    }

    protected override void OnStop()
    {
        uint cur;
        NtSetTimerResolution(this.targetResolution, false, out cur);
    }

    public static void Main(string[] args)
    {
        uint res = 5050;
        if (args != null && args.Length > 0)
        {
            uint parsed;
            if (uint.TryParse(args[0], out parsed) && parsed >= 5000 && parsed <= 156250)
            {
                res = parsed;
            }
        }
        ServiceBase.Run(new SetTimerResolutionService(res));
    }
}
"#;

pub fn disable_timer_resolution_throttling() {
    unsafe {
        let state = ProcessPowerThrottlingState {
            version: 1,
            control_mask: 0x1 | 0x4,
            state_mask: 0,
        };
        let _ = SetProcessInformation(
            GetCurrentProcess(),
            4,
            &state as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<ProcessPowerThrottlingState>() as u32,
        );
    }
}

/// Checks if the standalone Windows Service `SetTimerResolutionService` is installed and running.
pub fn is_timer_service_running() -> bool {
    if let Ok(out) = Command::new("sc.exe")
        .args(["query", "SetTimerResolutionService"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        return stdout.contains("RUNNING") || stdout.contains(": 4 ");
    }
    false
}

#[derive(Debug, Clone)]
pub struct SchedulerState {
    pub current_short_thread_policy: u32,
    pub short_thread_policy_optimal: bool,
    pub power_saving_disabled: bool,
    pub ifeo_priorities_applied: bool,
    pub process_lasso_installed: bool,
    pub process_lasso_rules_applied: bool,
    pub current_timer_resolution_100ns: u32,
}

impl SchedulerState {
    pub fn refresh(plan: &CpuAffinityPlan) -> Self {
        let current_short_thread_policy = query_short_thread_policy().unwrap_or(5);
        let short_thread_policy_optimal =
            current_short_thread_policy == plan.short_thread_policy_index;
        let power_saving_disabled = query_usb_selective_suspend_disabled();
        let ifeo_priorities_applied = check_ifeo_priorities_applied();
        let process_lasso_installed = is_process_lasso_installed();
        let process_lasso_rules_applied = check_process_lasso_rules_applied(plan);
        let current_timer_resolution_100ns = query_current_timer_resolution_100ns();

        Self {
            current_short_thread_policy,
            short_thread_policy_optimal,
            power_saving_disabled,
            ifeo_priorities_applied,
            process_lasso_installed,
            process_lasso_rules_applied,
            current_timer_resolution_100ns,
        }
    }
}

pub fn query_current_timer_resolution_100ns() -> u32 {
    unsafe {
        let mut min_res = 0u32;
        let mut max_res = 0u32;
        let mut cur_res = 156250u32;
        let _ = NtQueryTimerResolution(&mut min_res, &mut max_res, &mut cur_res);
        cur_res
    }
}

/// Installs and starts (or stops and removes) the standalone Windows Service `SetTimerResolutionService`
/// (`C:\ProgramData\LeagueUnlagged\TimerService\SetTimerResolutionService.exe`) so that the 0.505 ms
/// kernel timer resolution remains active across reboots at the OS service level without needing
/// League Unlagged running or installed.
pub fn set_kernel_timer_resolution(enable: bool, desired_100ns: u32) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges are required to install or remove SetTimerResolutionService."
                .to_string(),
        );
    }

    disable_timer_resolution_throttling();

    if enable {
        let _ = crate::tweaks::windows_reg::set_global_timer_resolution_requests(true);

        let dir = PathBuf::from(r"C:\ProgramData\LeagueUnlagged\TimerService");
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create TimerService directory: {e}"))?;
        let cs_path = dir.join("SetTimerResolutionService.cs");
        let exe_path = dir.join("SetTimerResolutionService.exe");

        let _ = Command::new("sc.exe")
            .args(["stop", "SetTimerResolutionService"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let _ = Command::new("sc.exe")
            .args(["delete", "SetTimerResolutionService"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        if !exe_path.exists() {
            fs::write(&cs_path, SET_TIMER_RESOLUTION_SERVICE_CS)
                .map_err(|e| format!("Failed to write SetTimerResolutionService.cs: {e}"))?;
            let csc = r"C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe";
            let out_arg = format!("/out:{}", exe_path.display());
            let compile = Command::new(csc)
                .args([
                    "/nologo",
                    "/optimize+",
                    "/target:exe",
                    "/r:System.ServiceProcess.dll",
                    &out_arg,
                    &cs_path.display().to_string(),
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("Failed to invoke csc.exe to build SetTimerResolutionService.exe: {e}"))?;
            if !compile.status.success() || !exe_path.exists() {
                return Err(format!(
                    "Failed to build SetTimerResolutionService.exe: {}",
                    String::from_utf8_lossy(&compile.stdout).trim()
                ));
            }
            let _ = fs::remove_file(&cs_path);
        }

        let bin_path = format!("{} {}", exe_path.display(), desired_100ns);
        let _ = Command::new("sc.exe")
            .args([
                "create",
                "SetTimerResolutionService",
                "binPath=",
                &bin_path,
                "DisplayName=",
                "Set Timer Resolution Service",
                "start=",
                "auto",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let _ = Command::new("sc.exe")
            .args(["start", "SetTimerResolutionService"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        unsafe {
            let mut cur = 0u32;
            let _ = NtSetTimerResolution(desired_100ns, 1, &mut cur);
        }

        let effective_cur = query_current_timer_resolution_100ns();
        Ok(format!(
            "SetTimerResolutionService (Windows Service start=auto) installed & started -> {:.3} ms ({} * 100ns)",
            effective_cur as f64 / 10_000.0,
            effective_cur
        ))
    } else {
        let _ = Command::new("sc.exe")
            .args(["stop", "SetTimerResolutionService"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let _ = Command::new("sc.exe")
            .args(["delete", "SetTimerResolutionService"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        unsafe {
            let mut cur = 0u32;
            let _ = NtSetTimerResolution(desired_100ns, 0, &mut cur);
        }

        let effective_cur = query_current_timer_resolution_100ns();
        Ok(format!(
            "SetTimerResolutionService stopped & removed -> Restored to OS Default ({:.3} ms)",
            effective_cur as f64 / 10_000.0
        ))
    }
}

/// Locale-independent parser for `powercfg /qh` output.
/// `powercfg /qh` always prints hex lines ending with:
/// - second-to-last `: 0x...`: Current AC Power Setting Index
/// - last `: 0x...`: Current DC Power Setting Index
pub fn parse_powercfg_ac_index(stdout: &[u8]) -> Option<u32> {
    let text = String::from_utf8_lossy(stdout);
    let mut hex_values = Vec::new();
    for line in text.lines() {
        if let Some(idx) = line.find(": 0x") {
            let hex_part = line[idx + 4..].trim();
            let clean_hex: String = hex_part
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            if let Ok(val) = u32::from_str_radix(&clean_hex, 16) {
                hex_values.push(val);
            }
        }
    }
    if hex_values.len() >= 2 {
        Some(hex_values[hex_values.len() - 2])
    } else {
        hex_values.last().copied()
    }
}

pub fn query_short_thread_policy() -> Option<u32> {
    let out = Command::new("powercfg")
        .args([
            "/qh",
            "SCHEME_CURRENT",
            "54533251-82be-4824-96c1-47b60b740d00",
            "bae08b81-2d5e-4688-ad6a-13243356654b",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    parse_powercfg_ac_index(&out.stdout)
}

pub fn set_short_thread_policy(index: u32) -> Result<String, String> {
    let idx_str = index.to_string();
    let _ = Command::new("powercfg")
        .args([
            "-attributes",
            "54533251-82be-4824-96c1-47b60b740d00",
            "bae08b81-2d5e-4688-ad6a-13243356654b",
            "-ATTRIB_HIDE",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    for flag in ["-setacvalueindex", "-setdcvalueindex"] {
        let out = Command::new("powercfg")
            .args([
                flag,
                "SCHEME_CURRENT",
                "54533251-82be-4824-96c1-47b60b740d00",
                "bae08b81-2d5e-4688-ad6a-13243356654b",
                &idx_str,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to execute powercfg {flag}: {e}"))?;
        if !out.status.success() {
            return Err(format!("powercfg {flag} returned a non-zero exit code"));
        }
    }

    let _ = Command::new("powercfg")
        .args(["-setactive", "SCHEME_CURRENT"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let name = match index {
        0 => "0 (All Processors)",
        1 => "1 (Performant Processors)",
        2 => "2 (Prefer Performant Processors)",
        5 => "5 (Automatic — Windows Default)",
        _ => "Custom",
    };
    Ok(format!(
        "Heterogeneous Short Running Thread Scheduling Policy -> {name}"
    ))
}

fn query_usb_selective_suspend_disabled() -> bool {
    if let Ok(out) = Command::new("powercfg")
        .args([
            "/qh",
            "SCHEME_CURRENT",
            "2a737441-1930-4402-8d77-b2bebba308a3",
            "48e6b7a6-50f5-4782-a5d4-53bb8f07e226",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        return parse_powercfg_ac_index(&out.stdout) == Some(0);
    }
    false
}

/// Disables Windows Power Saving throttles (USB Selective Suspend, PCIe ASPM, CPU Core Parking)
/// while keeping Processor C-States (`IDLEDISABLE = 0`) enabled for optimal single-core boost clocks.
pub fn set_power_saving_features_disabled(disable_power_saving: bool) -> Result<String, String> {
    let usb_val = if disable_power_saving { "0" } else { "1" };
    let pcie_val = if disable_power_saving { "0" } else { "1" };
    let unpark_val = if disable_power_saving { "100" } else { "5" };

    for flag in ["-setacvalueindex", "-setdcvalueindex"] {
        // 1. USB Selective Suspend -> 0 (Disabled)
        let _ = Command::new("powercfg")
            .args([
                flag,
                "SCHEME_CURRENT",
                "2a737441-1930-4402-8d77-b2bebba308a3",
                "48e6b7a6-50f5-4782-a5d4-53bb8f07e226",
                usb_val,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        // 2. PCIe Link State Power Management (ASPM) -> 0 (Off)
        let _ = Command::new("powercfg")
            .args([
                flag,
                "SCHEME_CURRENT",
                "501a4d13-42af-4429-9fd1-a8218c268e20",
                "ee12f906-d277-404b-b6da-e5fa1a576df5",
                pcie_val,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        // 3. CPU Core Parking Min Cores -> 100%
        let _ = Command::new("powercfg")
            .args([
                flag,
                "SCHEME_CURRENT",
                "54533251-82be-4824-96c1-47b60b740d00",
                "0cc5b647-c1df-4637-891a-dec35c318583",
                unpark_val,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }

    let _ = Command::new("powercfg")
        .args(["-setactive", "SCHEME_CURRENT"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    Ok(format!(
        "Bus Power Management (USB Selective Suspend, PCIe ASPM, Core Parking) -> {}",
        if disable_power_saving {
            "Disabled (Processor C-States Preserved)"
        } else {
            "Windows Default"
        }
    ))
}

fn check_ifeo_priorities_applied() -> bool {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(key) = hklm.open_subkey_with_flags(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\League of Legends.exe\PerfOptions",
        KEY_READ,
    ) {
        let prio: u32 = key.get_value("CpuPriorityClass").unwrap_or(0);
        return prio == 3;
    }
    false
}

/// Applies persistent Windows Kernel `Image File Execution Options\<exe>\PerfOptions` priorities.
pub fn set_ifeo_priorities(enable: bool) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges are required to modify Image File Execution Options (IFEO)."
                .to_string(),
        );
    }
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let base = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";

    let targets = [
        ("League of Legends.exe", 3u32, 3u32), // High CPU (3), High IO (3)
        ("LeagueClient.exe", 5u32, 1u32),      // Below Normal CPU (5), Very Low IO (1)
        ("LeagueClientUx.exe", 5u32, 1u32),
        ("LeagueClientUxRender.exe", 5u32, 1u32),
        ("LeagueCrashHandler64.exe", 5u32, 1u32),
        ("RiotClientServices.exe", 5u32, 1u32),
        ("Riot Client.exe", 5u32, 1u32),
        ("RiotClientUx.exe", 5u32, 1u32),
        ("RiotClientUxRender.exe", 5u32, 1u32),
        ("RiotClientCrashHandler.exe", 5u32, 1u32),
        ("vgc.exe", 5u32, 2u32),    // Vanguard Below Normal CPU (5)
        ("vgtray.exe", 5u32, 1u32), // Vanguard Tray Below Normal CPU (5)
    ];

    for (exe, cpu_prio, io_prio) in targets {
        let path = format!(r"{base}\{exe}\PerfOptions");
        if enable {
            let (k, _) = hklm
                .create_subkey(&path)
                .map_err(|e| format!("Failed to create IFEO key for {exe}: {e}"))?;
            k.set_value("CpuPriorityClass", &cpu_prio)
                .map_err(|e| format!("Failed to write CpuPriorityClass for {exe}: {e}"))?;
            k.set_value("IoPriority", &io_prio)
                .map_err(|e| format!("Failed to write IoPriority for {exe}: {e}"))?;
        } else {
            let _ = hklm.delete_subkey_all(&path);
        }
    }

    Ok(format!(
        "Kernel IFEO PerfOptions Priorities (League of Legends.exe, LeagueClient*, RiotClient*, vgc.exe, vgtray.exe) -> {}",
        if enable {
            "Status: Active"
        } else {
            "Status: Restored to Default"
        }
    ))
}

pub fn is_process_lasso_installed() -> bool {
    std::path::Path::new(r"C:\Program Files\Process Lasso\ProcessLasso.exe").exists()
        || std::path::Path::new(r"C:\Program Files\Process Lasso\ProcessGovernor.exe").exists()
}

pub fn install_process_lasso_via_winget() -> Result<String, String> {
    let out = Command::new("winget")
        .args([
            "install",
            "--id",
            "Bitsum.ProcessLasso",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--silent",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to launch winget: {e}"))?;

    if out.status.success() || is_process_lasso_installed() {
        Ok("Process Lasso installed successfully.".to_string())
    } else {
        Err(format!(
            "Failed to install Process Lasso via winget: {}",
            String::from_utf8_lossy(&out.stdout)
        ))
    }
}

pub fn get_process_lasso_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let progdata = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
    paths.push(
        PathBuf::from(progdata)
            .join("ProcessLasso")
            .join("config")
            .join("prolasso.ini"),
    );
    if let Ok(appdata) = std::env::var("APPDATA") {
        let p = PathBuf::from(appdata)
            .join("ProcessLasso")
            .join("config")
            .join("prolasso.ini");
        if p.exists() && !paths.contains(&p) {
            paths.push(p);
        }
    }
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        let p = PathBuf::from(localappdata)
            .join("ProcessLasso")
            .join("config")
            .join("prolasso.ini");
        if p.exists() && !paths.contains(&p) {
            paths.push(p);
        }
    }
    paths
}

/// Reads Process Lasso's `prolasso.ini` (stored in UTF-16LE with BOM `0xFF, 0xFE`).
pub fn read_prolasso_ini(path: &std::path::Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return Some(String::from_utf16_lossy(&u16s));
    }
    if bytes.len() >= 4 && bytes.len() % 2 == 0 && bytes[1] == 0 && bytes[3] == 0 {
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return Some(String::from_utf16_lossy(&u16s));
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Writes Process Lasso's `prolasso.ini` in UTF-16LE with BOM (`0xFF, 0xFE`) as required by `ProcessGovernor.exe`.
pub fn write_prolasso_ini(path: &std::path::Path, content: &str) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(2 + content.len() * 2);
    bytes.push(0xFF);
    bytes.push(0xFE);
    for unit in content.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    if fs::write(path, &bytes).is_ok() {
        return Ok(());
    }

    let tmp_path = std::env::temp_dir().join("league_unlagged_prolasso.ini");
    fs::write(&tmp_path, &bytes)
        .map_err(|e| format!("Failed to write temporary file {}: {e}", tmp_path.display()))?;

    let src = tmp_path.display().to_string().replace('\'', "''");
    let dst = path.display().to_string().replace('\'', "''");
    let inner_cmd = format!(
        "Copy-Item -LiteralPath '{src}' -Destination '{dst}' -Force; icacls '{dst}' /grant '*S-1-5-32-545:(M)' | Out-Null; Restart-Service -Name 'ProcessGovernor' -Force -ErrorAction SilentlyContinue"
    );
    let outer_cmd = format!(
        "Start-Process powershell -Verb RunAs -WindowStyle Hidden -Wait -ArgumentList '-NoProfile','-NonInteractive','-Command',\"{}\"",
        inner_cmd.replace('"', "\\\"")
    );

    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &outer_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    if fs::write(path, &bytes).is_ok() {
        Ok(())
    } else {
        if let Some(read_back) = read_prolasso_ini(path) {
            if read_back.contains("league of legends.exe")
                || content.contains("DefaultAffinitiesEx=\r\n")
            {
                return Ok(());
            }
        }
        Err(format!(
            "Access denied to {}. Approve the UAC prompt or click 'Run as Admin' in the top bar.",
            path.display()
        ))
    }
}

/// Converts comma-separated CPU core list (e.g. `"2,4,6,8,10"`) to Process Lasso `DefaultAffinitiesEx`
/// semicolon-separated core list (`"2;4;6;8;10"`).
pub fn to_prolasso_core_spec(cpus_csv: &str) -> String {
    cpus_csv
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(";")
}

pub fn check_process_lasso_rules_applied(plan: &CpuAffinityPlan) -> bool {
    let expected_game_spec = format!(
        "league of legends.exe,0,{}",
        to_prolasso_core_spec(&plan.game_cpus_lasso)
    );
    for path in get_process_lasso_config_paths() {
        if let Some(content) = read_prolasso_ini(&path) {
            let lower = content.to_lowercase();
            if lower.contains(&expected_game_spec.to_lowercase())
                && lower.contains("vgc.exe,0,")
                && lower.contains("oocon=false")
                && lower.contains("oocexclusions=league of legends.exe")
            {
                return true;
            }
        }
    }
    false
}

/// Builds the Process Lasso `prolasso.ini` content with:
/// - `OocOn=false` and `OocExclusions=...` under `[OutOfControlProcessRestraint]` (disables ProBalance globally and per-process)
/// - `DefaultAffinitiesEx=` under `[ProcessDefaults]` with semicolon-delimited core masks (`proc.exe,0,2;4;6;8;10`)
/// - `DefaultPriorities=` under `[ProcessDefaults]` (`league of legends.exe,high`, clients & Vanguard `below normal`)
pub fn build_process_lasso_ini(existing_content: &str, plan: &CpuAffinityPlan) -> String {
    let game_spec = to_prolasso_core_spec(&plan.game_cpus_lasso);
    let client_spec = to_prolasso_core_spec(&plan.client_cpus_lasso);
    let vanguard_spec = to_prolasso_core_spec(&plan.vanguard_cpus_lasso);

    let ooc_exclusions = [
        "league of legends.exe",
        "leagueclient.exe",
        "leagueclientux.exe",
        "leagueclientuxrender.exe",
        "leaguecrashhandler64.exe",
        "leaguecrashhandler.exe",
        "riotclientservices.exe",
        "riot client.exe",
        "riotclientux.exe",
        "riotclientuxrender.exe",
        "riotclientcrashhandler.exe",
        "vgc.exe",
        "vgtray.exe",
    ]
    .join(",");

    let affinities_ex = [
        format!("league of legends.exe,0,{game_spec}"),
        format!("leagueclient.exe,0,{client_spec}"),
        format!("leagueclientux.exe,0,{client_spec}"),
        format!("leagueclientuxrender.exe,0,{client_spec}"),
        format!("leaguecrashhandler64.exe,0,{client_spec}"),
        format!("leaguecrashhandler.exe,0,{client_spec}"),
        format!("riotclientservices.exe,0,{client_spec}"),
        format!("riot client.exe,0,{client_spec}"),
        format!("riotclientux.exe,0,{client_spec}"),
        format!("riotclientuxrender.exe,0,{client_spec}"),
        format!("riotclientcrashhandler.exe,0,{client_spec}"),
        format!("vgc.exe,0,{vanguard_spec}"),
        format!("vgtray.exe,0,{vanguard_spec}"),
    ]
    .join(",");

    let priorities = [
        "league of legends.exe,high",
        "leagueclient.exe,below normal",
        "leagueclientux.exe,below normal",
        "leagueclientuxrender.exe,below normal",
        "leaguecrashhandler64.exe,below normal",
        "leaguecrashhandler.exe,below normal",
        "riotclientservices.exe,below normal",
        "riot client.exe,below normal",
        "riotclientux.exe,below normal",
        "riotclientuxrender.exe,below normal",
        "riotclientcrashhandler.exe,below normal",
        "vgc.exe,below normal",
        "vgtray.exe,below normal",
    ]
    .join(",");

    if existing_content.trim().is_empty() {
        return format!(
            "[OutOfControlProcessRestraint]\r\n\
             OocOn=false\r\n\
             OocExclusions={ooc_exclusions}\r\n\
             [ProcessDefaults]\r\n\
             DefaultPriorities={priorities}\r\n\
             DefaultAffinitiesEx={affinities_ex}\r\n"
        );
    }

    let mut lines: Vec<String> = existing_content.lines().map(|s| s.to_string()).collect();
    let mut updated_ooc = false;
    let mut updated_ooc_excl = false;
    let mut updated_affinities_ex = false;
    let mut updated_priorities = false;

    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.starts_with("OocOn=") {
            *line = "OocOn=false".to_string();
            updated_ooc = true;
        } else if trimmed.starts_with("OocExclusions=") {
            *line = format!("OocExclusions={ooc_exclusions}");
            updated_ooc_excl = true;
        } else if trimmed.starts_with("DefaultAffinitiesEx=") {
            *line = format!("DefaultAffinitiesEx={affinities_ex}");
            updated_affinities_ex = true;
        } else if trimmed.starts_with("DefaultPriorities=") {
            *line = format!("DefaultPriorities={priorities}");
            updated_priorities = true;
        } else if trimmed.starts_with("DefaultAffinities=") {
            *line = "DefaultAffinities=".to_string();
        }
    }

    if !updated_ooc || !updated_ooc_excl {
        if let Some(pos) = lines
            .iter()
            .position(|l| l.trim().eq_ignore_ascii_case("[OutOfControlProcessRestraint]"))
        {
            if !updated_ooc_excl {
                lines.insert(pos + 1, format!("OocExclusions={ooc_exclusions}"));
            }
            if !updated_ooc {
                lines.insert(pos + 1, "OocOn=false".to_string());
            }
        } else {
            lines.insert(0, "[OutOfControlProcessRestraint]".to_string());
            lines.insert(1, "OocOn=false".to_string());
            lines.insert(2, format!("OocExclusions={ooc_exclusions}"));
        }
    }

    if !updated_affinities_ex || !updated_priorities {
        if let Some(pos) = lines
            .iter()
            .position(|l| l.trim().eq_ignore_ascii_case("[ProcessDefaults]"))
        {
            if !updated_affinities_ex {
                lines.insert(pos + 1, format!("DefaultAffinitiesEx={affinities_ex}"));
            }
            if !updated_priorities {
                lines.insert(pos + 1, format!("DefaultPriorities={priorities}"));
            }
        } else {
            lines.push("[ProcessDefaults]".to_string());
            if !updated_priorities {
                lines.push(format!("DefaultPriorities={priorities}"));
            }
            if !updated_affinities_ex {
                lines.push(format!("DefaultAffinitiesEx={affinities_ex}"));
            }
        }
    }

    lines.join("\r\n") + "\r\n"
}

fn stop_process_lasso_gui_if_running() -> bool {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "if (Get-Process -Name 'ProcessLasso' -ErrorAction SilentlyContinue) { Stop-Process -Name 'ProcessLasso' -Force -ErrorAction SilentlyContinue; Write-Output 'STOPPED' }",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(out) = output {
        let s = String::from_utf8_lossy(&out.stdout);
        return s.contains("STOPPED");
    }
    false
}

/// Signals Process Lasso Governor (`ProcessGovernor.exe`) and restarts `ProcessLasso.exe` GUI if it was open.
pub fn notify_and_reload_process_lasso(relaunch_gui: bool) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent};

    let event_names = [
        "Lasso777\0",
        "Lasso888_2\0",
        "{49da4c63-c475-4b6c-83f3-c78a9fe813c2}\0",
        "Global\\Lasso777\0",
        "Global\\Lasso888_2\0",
        "Global\\{49da4c63-c475-4b6c-83f3-c78a9fe813c2}\0",
    ];

    unsafe {
        for name in event_names {
            let wide: Vec<u16> = name.encode_utf16().collect();
            let h = CreateEventW(std::ptr::null(), 0, 0, wide.as_ptr());
            if !h.is_null() {
                let _ = SetEvent(h);
                let _ = CloseHandle(h);
            }
        }
    }

    let script = if relaunch_gui {
        "Restart-Service -Name 'ProcessGovernor' -Force -ErrorAction SilentlyContinue; if (Test-Path 'C:\\Program Files\\Process Lasso\\ProcessLasso.exe') { Start-Process 'C:\\Program Files\\Process Lasso\\ProcessLasso.exe' }"
    } else {
        "Restart-Service -Name 'ProcessGovernor' -Force -ErrorAction SilentlyContinue"
    };

    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

pub fn apply_process_lasso_rules(plan: &CpuAffinityPlan) -> Result<String, String> {
    let paths = get_process_lasso_config_paths();
    if paths.is_empty() {
        return Err(
            "Unable to resolve path to C:\\ProgramData\\ProcessLasso\\config\\prolasso.ini."
                .to_string(),
        );
    }

    let gui_was_running = stop_process_lasso_gui_if_running();

    let mut written_count = 0;
    for path in &paths {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let existing = read_prolasso_ini(path).unwrap_or_default();
        let new_ini = build_process_lasso_ini(&existing, plan);
        if write_prolasso_ini(path, &new_ini).is_ok() {
            written_count += 1;
        }
    }

    if written_count > 0 {
        notify_and_reload_process_lasso(gui_was_running);
        Ok(format!(
            "Process Lasso profile (prolasso.ini) applied: ProBalance Disabled (OocOn=false & OocExclusions), League of Legends.exe -> CPUs [{}], Client/Riot -> CPUs [{}], Vanguard -> CPUs [{}].",
            to_prolasso_core_spec(&plan.game_cpus_lasso),
            to_prolasso_core_spec(&plan.client_cpus_lasso),
            to_prolasso_core_spec(&plan.vanguard_cpus_lasso)
        ))
    } else {
        if gui_was_running {
            notify_and_reload_process_lasso(true);
        }
        Err(
            "Failed to write C:\\ProgramData\\ProcessLasso\\config\\prolasso.ini (Run as Administrator)."
                .to_string(),
        )
    }
}

pub fn remove_process_lasso_rules() -> Result<String, String> {
    let paths = get_process_lasso_config_paths();
    let gui_was_running = stop_process_lasso_gui_if_running();
    let mut updated = 0;
    for path in &paths {
        if let Some(existing) = read_prolasso_ini(path) {
            let mut lines: Vec<String> = existing.lines().map(|s| s.to_string()).collect();
            for line in &mut lines {
                let trimmed = line.trim();
                if trimmed.starts_with("DefaultAffinitiesEx=") {
                    *line = "DefaultAffinitiesEx=".to_string();
                } else if trimmed.starts_with("DefaultPriorities=") {
                    *line = "DefaultPriorities=".to_string();
                } else if trimmed.starts_with("DefaultAffinities=") {
                    *line = "DefaultAffinities=".to_string();
                } else if trimmed.starts_with("OocExclusions=") {
                    *line = "OocExclusions=".to_string();
                }
            }
            let out_str = lines.join("\r\n") + "\r\n";
            if write_prolasso_ini(path, &out_str).is_ok() {
                updated += 1;
            }
        }
    }
    if updated > 0 {
        notify_and_reload_process_lasso(gui_was_running);
        Ok(
            "Process Lasso CPU affinity and priority rules cleared; ProcessGovernor reloaded."
                .to_string(),
        )
    } else {
        if gui_was_running {
            notify_and_reload_process_lasso(true);
        }
        Ok("Process Lasso rules reset to default.".to_string())
    }
}

/// Checks if the current process has Administrator (elevated) write access to `HKLM`.
pub fn is_running_as_admin() -> bool {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    hklm.open_subkey_with_flags(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options",
        KEY_WRITE,
    )
    .is_ok()
}

/// Relaunches the current executable with Administrator privileges via UAC (`RunAs`).
pub fn relaunch_as_admin() -> Result<String, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("Failed to resolve current executable path: {e}"))?;
    let script = format!(
        "Start-Process -FilePath '{}' -Verb RunAs",
        exe.display().to_string().replace('\'', "''")
    );
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Failed to relaunch as Administrator: {e}"))?;
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{compute_optimal_cpu_plan, CoreInfo};

    #[test]
    fn test_process_lasso_ini_includes_vanguard_and_disables_probalance() {
        let mut cores = Vec::new();
        for i in 0..6 {
            cores.push(CoreInfo {
                core_index: i,
                mask: (1u64 << (i * 2)) | (1u64 << (i * 2 + 1)),
                logical_threads: vec![i * 2, i * 2 + 1],
                efficiency_class: 0,
                l3_group: 0,
            });
        }
        let plan = compute_optimal_cpu_plan(&cores, "AMD Ryzen 5 5600");
        let ini = build_process_lasso_ini("", &plan);

        assert!(ini.contains("OocOn=false"));
        assert!(ini.contains("OocExclusions=league of legends.exe,leagueclient.exe"));
        assert!(ini.contains("DefaultAffinitiesEx="));
        assert!(ini.contains("league of legends.exe,0,2;4;6;8;10"));
        assert!(ini.contains("riotclientservices.exe,0,2;3;4;5;6;7;8;9"));
        assert!(ini.contains("vgc.exe,0,2;3;4;5;6;7;8;9"));
        assert!(ini.contains("vgtray.exe,0,2;3;4;5;6;7;8;9"));
    }

    #[test]
    fn test_parse_powercfg_ac_index_locale_independent() {
        let localized_output = "\
Minimum Possible Setting: 0x00000000\r\n\
Maximum Possible Setting: 0x00000005\r\n\
Possible Settings increment: 0x00000001\r\n\
AC Setting Index: 0x00000000\r\n\
DC Setting Index: 0x00000005\r\n";
        assert_eq!(parse_powercfg_ac_index(localized_output.as_bytes()), Some(0));

        let en_output = "\
Current AC Power Setting Index: 0x00000001\r\n\
Current DC Power Setting Index: 0x00000005\r\n";
        assert_eq!(parse_powercfg_ac_index(en_output.as_bytes()), Some(1));
    }
}
