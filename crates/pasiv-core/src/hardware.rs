// SPDX-License-Identifier: GPL-3.0-only
//! Detect CPU/GPU + capability (ARCHITECTURE.md §3). Pearl is gated to
//! CUDA Turing+ / ≥3 GB VRAM, so on macOS the answer is always "no eligible
//! GPU" and the UI never offers GPU mining. On Windows/Linux we probe
//! `nvidia-smi` (nvml-wrapper can replace this when M3 lands for real).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct GpuInfo {
    pub name: String,
    pub vram_mb: u64,
    pub eligible: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HardwareInfo {
    pub cpu_cores: u32,
    /// Threads Full power actually launches here — `config::usable_threads`,
    /// which caps at physical cores and L3 capacity. Reported separately from
    /// `cpu_cores` because the two differ on every SMT machine, and the UI must
    /// quote the number the miner uses rather than the one the OS advertises.
    pub usable_threads: u32,
    /// The CPU's marketing name ("AMD Ryzen 5 7500F 6-Core Processor"), or None
    /// where we can't read it.
    ///
    /// WHY IT IS WORTH A FIELD. Core counts alone cannot explain a slow node.
    /// Two fleet machines both reported `cpu_cores: 12, usable_threads: 12` while
    /// mining 5489 H/s and 1871 H/s — a 3x gap that is either a tuning problem we
    /// can fix (large pages, power mode) or simply older silicon, and *nothing in
    /// the payload could tell them apart* (2026-08-15). Without the model there is
    /// no way to answer "why is this node under-earning" from the fleet view.
    ///
    /// This rides in the same opt-in Cloud `hardware` blob that already publishes
    /// GPU model and core counts, so it adds no new consent surface — and it is
    /// capability, never identity: no serial, no ID.
    pub cpu_model: Option<String>,
    pub gpus: Vec<GpuInfo>,
}

pub fn detect() -> HardwareInfo {
    HardwareInfo {
        cpu_cores: std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1),
        usable_threads: usable_threads(),
        cpu_model: cpu_model().map(str::to_owned),
        gpus: detect_gpus(),
    }
}

/// Cached: `detect()` is called on UI polls (gpu::status, the coin picker), and
/// reading the model shells out on Windows and macOS. The CPU cannot change
/// under a running process, so once is right as well as cheap.
fn cpu_model() -> Option<&'static str> {
    static MODEL: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    MODEL.get_or_init(read_cpu_model).as_deref()
}

#[cfg(target_os = "linux")]
fn read_cpu_model() -> Option<String> {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| parse_cpuinfo_model(&t))
}

/// Pure: the first `model name` value out of /proc/cpuinfo text.
#[cfg(target_os = "linux")]
pub fn parse_cpuinfo_model(text: &str) -> Option<String> {
    text.lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(target_os = "macos")]
fn read_cpu_model() -> Option<String> {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !s.is_empty()).then_some(s)
}

#[cfg(windows)]
fn read_cpu_model() -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // The registry holds the marketing name; PROCESSOR_IDENTIFIER only gives
    // "AMD64 Family 25 Model 97 …", which is not what anyone reads a fleet by.
    // CREATE_NO_WINDOW for the same reason as the nvidia-smi probe below: a GUI
    // app must not blink a console open.
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            "/v",
            "ProcessorNameString",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| parse_reg_processor_name(&String::from_utf8_lossy(&out.stdout)))
        .flatten()
}

/// Pure: the value out of `reg query … /v ProcessorNameString` output, whose
/// line is `    ProcessorNameString    REG_SZ    AMD Ryzen 5 7500F 6-Core Processor`.
#[cfg(windows)]
pub fn parse_reg_processor_name(text: &str) -> Option<String> {
    text.lines()
        .find(|l| l.contains("ProcessorNameString"))
        .and_then(|l| l.split("REG_SZ").nth(1))
        .map(|v| v.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn read_cpu_model() -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn detect_gpus() -> Vec<GpuInfo> {
    Vec::new() // no CUDA on macOS; Pearl mining is Windows/Linux only
}

#[cfg(not(target_os = "macos"))]
fn detect_gpus() -> Vec<GpuInfo> {
    // Minimal probe until nvml-wrapper lands with real M3: name + VRAM.
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,memory.total",
        "--format=csv,noheader,nounits",
    ]);
    // Without this a GUI app flashes a console window every time it probes for
    // GPUs, which on Windows happens during setup — so the user sees a black
    // box blink open on launch.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = match cmd.output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };
    parse_nvidia_smi(&out)
}

/// Parse `nvidia-smi --query-gpu=name,memory.total` output. Pearl gating:
/// Turing+ (RTX 20xx onward, GTX 16xx onward, Tesla T4) with ≥3 GB VRAM.
#[allow(dead_code)] // used on non-macOS; unit-tested on all platforms
fn parse_nvidia_smi(out: &str) -> Vec<GpuInfo> {
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let (name, mem) = line.rsplit_once(',')?;
            let name = name.trim().to_string();
            let vram_mb: u64 = mem.trim().parse().ok()?;
            let eligible = vram_mb >= 3072
                && (name.contains("RTX") || name.contains("GTX 16") || name.contains("Tesla T"));
            Some(GpuInfo {
                name,
                vram_mb,
                eligible,
            })
        })
        .collect()
}

/// The largest eligible GPU's VRAM (MB) — what the per-coin `min_vram_mb` gate
/// compares against. 0 when no eligible GPU is present, which fails every GPU
/// coin's gate for the honest reason (no GPU) rather than a VRAM number.
pub fn max_eligible_vram_mb(gpus: &[GpuInfo]) -> u64 {
    gpus.iter()
        .filter(|g| g.eligible)
        .map(|g| g.vram_mb)
        .max()
        .unwrap_or(0)
}

// ── Thread-count decision (RandomX topology limits) ──────────────────────

/// This machine's useful thread cap — `decide_threads` fed the live probes.
///
/// Public because the UI has to quote the same number the miner runs. Settings
/// says "Eco stays quiet on about half your cores (N of M); Full uses all M",
/// and it built M from `available_parallelism` — so on a 6c/12t chip it
/// promised "Full uses all 12" while the miner launched 6. A number the user
/// can check against their own task manager has to come from one place.
pub fn usable_threads() -> u32 {
    decide_threads(
        std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1),
        physical_cores(),
        l3_bytes(),
    )
}

/// RandomX gives every mining thread its own 2 MB scratchpad.
const RANDOMX_SCRATCHPAD_BYTES: u64 = 2 * 1024 * 1024;

/// Pure decision: how many threads "Full power" should actually launch.
///
/// RandomX is bottlenecked on memory latency, not arithmetic, so two limits cap
/// useful thread count independently of how many logical CPUs the OS reports:
///
/// - **L3 capacity.** Each thread holds a 2 MB scratchpad. Once the live set
///   exceeds L3 the threads evict each other's scratchpads and *every* thread
///   slows down — so past this point more threads means less total hashrate.
/// - **Physical cores.** SMT siblings share one core's L1/L2 and its load/store
///   path. A second thread on the same core contends for the resource that is
///   already the bottleneck rather than adding throughput.
///
/// Measured on an i5-12400F (6 physical / 12 logical, 18 MB L3) against
/// MoneroOcean: **6 threads sustained 5124 H/s, 12 threads 5009 H/s.** So the
/// old "Full = every logical core" was not merely wasteful — it was *slower
/// than Eco* while taking the machine from usable to unusable. That is the
/// worst possible shape for a toggle labelled "Full power".
///
/// Unknown topology abstains rather than guessing: a `None` leaves that limit
/// unapplied, so a platform with no probe keeps the previous all-logical-cores
/// behaviour instead of silently mining on one thread.
pub fn decide_threads(logical: u32, physical: Option<u32>, l3_bytes: Option<u64>) -> u32 {
    let mut cap = logical;
    if let Some(p) = physical.filter(|p| *p > 0) {
        cap = cap.min(p);
    }
    if let Some(fits) = l3_bytes.map(|b| (b / RANDOMX_SCRATCHPAD_BYTES) as u32) {
        if fits > 0 {
            cap = cap.min(fits);
        }
    }
    cap.max(1)
}

/// Pure: parse a sysfs cache size (`"18432K"`, `"512K"`, `"32M"`) into bytes.
///
/// Only `l3_bytes` calls it, and only Linux has a `l3_bytes` that reads
/// anything — so on macOS and Windows this is dead code and `-D warnings`
/// rejects the build. Kept compiled everywhere rather than `#[cfg]`-ed to
/// Linux so its tests run on every platform: the parser is pure string
/// handling, and a sysfs format change should fail the suite wherever it runs.
/// Same treatment as `parse_meminfo` in the desktop's largepages module, for the same reason.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_cache_size(raw: &str) -> Option<u64> {
    let s = raw.trim();
    let (digits, mult) = match s.as_bytes().last()? {
        b'K' | b'k' => (&s[..s.len() - 1], 1024),
        b'M' | b'm' => (&s[..s.len() - 1], 1024 * 1024),
        b'0'..=b'9' => (s, 1),
        _ => return None,
    };
    digits.trim().parse::<u64>().ok().map(|n| n * mult)
}

/// Distinct (package, core) pairs under `/sys/devices/system/cpu/*/topology/`.
/// That is the count of real cores; `available_parallelism` counts SMT siblings.
#[cfg(target_os = "linux")]
fn physical_cores() -> Option<u32> {
    let mut seen = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir("/sys/devices/system/cpu").ok()?.flatten() {
        let path = entry.path();
        let topo = path.join("topology");
        let (Ok(core), Ok(pkg)) = (
            std::fs::read_to_string(topo.join("core_id")),
            std::fs::read_to_string(topo.join("physical_package_id")),
        ) else {
            continue;
        };
        seen.insert((pkg.trim().to_string(), core.trim().to_string()));
    }
    (!seen.is_empty()).then_some(seen.len() as u32)
}

/// Largest last-level cache reported for cpu0. Reads every `index*` rather than
/// assuming `index3` is L3 — that mapping is not guaranteed, and on machines
/// with an L4 or a split LLC the highest level present is the one that matters.
#[cfg(target_os = "linux")]
fn l3_bytes() -> Option<u64> {
    let mut best: Option<(u32, u64)> = None;
    for entry in std::fs::read_dir("/sys/devices/system/cpu/cpu0/cache")
        .ok()?
        .flatten()
    {
        let dir = entry.path();
        let (Ok(level), Ok(size)) = (
            std::fs::read_to_string(dir.join("level")),
            std::fs::read_to_string(dir.join("size")),
        ) else {
            continue;
        };
        let (Ok(level), Some(bytes)) = (level.trim().parse::<u32>(), parse_cache_size(&size))
        else {
            continue;
        };
        if level >= 3 && best.is_none_or(|(bl, _)| level > bl) {
            best = Some((level, bytes));
        }
    }
    best.map(|(_, bytes)| bytes)
}

// Other platforms abstain until they grow a topology probe; `decide_threads`
// treats `None` as "limit unknown", preserving the previous behaviour there.
#[cfg(not(target_os = "linux"))]
fn physical_cores() -> Option<u32> {
    None
}

#[cfg(not(target_os = "linux"))]
fn l3_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The VRAM figure that feeds the per-coin gate comes from these fixtures —
    /// they are also the documented way to fake a small GPU when testing the
    /// gate (front a fake `nvidia-smi` on PATH echoing one of these lines).
    #[test]
    fn parse_nvidia_smi_vram_feeds_the_gate() {
        // A 4 GB RTX is eligible (≥3 GB floor) but must report 4096 to the gate.
        let one = parse_nvidia_smi("NVIDIA GeForce RTX 3050, 4096\n");
        assert_eq!(max_eligible_vram_mb(&one), 4096);
        // Max over ELIGIBLE cards only: the 2 GB GTX 1050 is ineligible and the
        // 12 GB RTX 3060 wins.
        let two =
            parse_nvidia_smi("NVIDIA GeForce RTX 3060, 12288\nNVIDIA GeForce GTX 1050, 2048\n");
        assert_eq!(max_eligible_vram_mb(&two), 12288);
        // No GPUs at all → 0, which fails every GPU coin's floor.
        assert_eq!(max_eligible_vram_mb(&[]), 0);
    }

    #[test]
    fn detect_reports_real_cores() {
        let hw = detect();
        assert!(hw.cpu_cores >= 1);
        // The cap is a *restriction* on the logical count, never an invention:
        // a UI quoting more threads than the machine has would be worse than
        // the over-promise this field exists to fix.
        assert!(hw.usable_threads >= 1);
        assert!(hw.usable_threads <= hw.cpu_cores);
        #[cfg(target_os = "macos")]
        assert!(hw.gpus.is_empty(), "no CUDA gating path on macOS");
    }

    /// The model is the field that makes a slow node diagnosable, so it has to
    /// actually arrive on the platforms we ship. It is allowed to be None (a
    /// locked-down box, an unreadable probe) — but it must never be an empty
    /// string, which would read as "we know it, and it's blank".
    #[test]
    fn cpu_model_is_absent_or_meaningful_never_empty() {
        let hw = detect();
        if let Some(m) = &hw.cpu_model {
            assert!(!m.trim().is_empty(), "empty model is worse than None");
            assert_eq!(m, m.trim(), "model must arrive trimmed");
        }
        // Cached, so a second read is identical and costs no probe.
        assert_eq!(hw.cpu_model, detect().cpu_model);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cpuinfo_model_is_read_from_the_first_processor_block() {
        let text = "processor\t: 0\n\
                    vendor_id\t: AuthenticAMD\n\
                    model name\t: AMD Ryzen 5 7500F 6-Core Processor\n\
                    siblings\t: 12\n\
                    processor\t: 1\n\
                    model name\t: AMD Ryzen 5 7500F 6-Core Processor\n";
        assert_eq!(
            parse_cpuinfo_model(text).as_deref(),
            Some("AMD Ryzen 5 7500F 6-Core Processor")
        );
        // A kernel that doesn't publish it (some ARM boards) yields None, not "".
        assert_eq!(parse_cpuinfo_model("processor\t: 0\n"), None);
        assert_eq!(parse_cpuinfo_model("model name\t:   \n"), None);
    }

    #[cfg(windows)]
    #[test]
    fn reg_query_processor_name_is_parsed() {
        // Real `reg query` shape: leading indent, tab-ish spacing, REG_SZ, value.
        let out =
            "\r\nHKEY_LOCAL_MACHINE\\HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0\r\n    \
                   ProcessorNameString    REG_SZ    AMD Ryzen 5 7500F 6-Core Processor\r\n\r\n";
        assert_eq!(
            parse_reg_processor_name(out).as_deref(),
            Some("AMD Ryzen 5 7500F 6-Core Processor")
        );
        assert_eq!(parse_reg_processor_name("ERROR: unable to find\r\n"), None);
    }

    #[test]
    fn nvidia_parse_gates_on_turing_and_vram() {
        let out = "NVIDIA GeForce RTX 3080, 10240\n\
                   NVIDIA GeForce GTX 1650, 4096\n\
                   NVIDIA GeForce GTX 1060, 6144\n\
                   Tesla T4, 15360\n\
                   NVIDIA GeForce RTX 3050, 2048\n";
        let gpus = parse_nvidia_smi(out);
        assert_eq!(gpus.len(), 5);
        let by = |n: &str| gpus.iter().find(|g| g.name.contains(n)).unwrap();
        assert!(by("RTX 3080").eligible);
        assert!(by("GTX 1650").eligible); // Turing 16-series, 4 GB
        assert!(!by("GTX 1060").eligible); // Pascal, too old
        assert!(by("Tesla T").eligible);
        assert!(!by("RTX 3050").eligible); // eligible arch but < 3 GB
    }

    #[test]
    fn nvidia_parse_tolerates_empty() {
        assert!(parse_nvidia_smi("").is_empty());
        assert!(parse_nvidia_smi("\n\n").is_empty());
    }

    /// The exact machine the regression was measured on: 6 physical / 12
    /// logical, 18 MB L3. Both limits agree the answer is 6, and 12 threads
    /// benchmarked *slower* than 6 (5009 vs 5124 H/s) — so if this ever returns
    /// 12 again, "Full power" has gone back to costing hashrate.
    #[test]
    fn full_power_does_not_oversubscribe_an_smt_cpu() {
        assert_eq!(decide_threads(12, Some(6), Some(18 * 1024 * 1024)), 6);
    }

    /// L3 binds before core count on a many-core chip with a small cache: 16
    /// physical cores but only 8 MB L3 means 4 scratchpads fit, so 4 threads.
    #[test]
    fn l3_capacity_can_bind_tighter_than_core_count() {
        assert_eq!(decide_threads(32, Some(16), Some(8 * 1024 * 1024)), 4);
    }

    /// Unknown topology must abstain, not guess — a missing probe leaves the
    /// previous all-logical-cores behaviour rather than collapsing to 1 thread.
    #[test]
    fn unknown_topology_falls_back_to_logical_cores() {
        assert_eq!(decide_threads(8, None, None), 8);
        assert_eq!(decide_threads(8, Some(4), None), 4);
        assert_eq!(decide_threads(8, None, Some(4 * 1024 * 1024)), 2);
    }

    /// Never zero, and a nonsense probe abstains rather than starving the
    /// miner: a reported 0 cores / 0 bytes is not evidence for "one thread",
    /// it is evidence the probe failed, so the logical count still stands.
    #[test]
    fn degenerate_probes_abstain_and_never_return_zero() {
        assert_eq!(decide_threads(1, Some(1), Some(1024 * 1024)), 1);
        assert_eq!(decide_threads(4, Some(0), Some(0)), 4);
        assert_eq!(decide_threads(0, None, None), 1);
    }

    #[test]
    fn parses_sysfs_cache_sizes() {
        assert_eq!(parse_cache_size("18432K"), Some(18 * 1024 * 1024));
        assert_eq!(parse_cache_size(" 512K\n"), Some(512 * 1024));
        assert_eq!(parse_cache_size("32M"), Some(32 * 1024 * 1024));
        assert_eq!(parse_cache_size("4096"), Some(4096));
        assert_eq!(parse_cache_size(""), None);
        assert_eq!(parse_cache_size("banana"), None);
    }
}
