// SPDX-License-Identifier: GPL-3.0-only
//! Shared miner/coin types (extracted from the desktop app's `miners` module).
//! The `Miner` trait itself — process spawning and control — lives with each
//! runner (the proprietary desktop app's sidecar adapters, and `pasivd`'s own
//! XMRig driver); these are the data shapes they agree on.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MinerId {
    Xmrig,
    SrbMiner,
    /// Verus — the only IN-PROCESS miner: native arm64 VerusHash + a Rust
    /// stratum client, no vendored sidecar. It never spawns an OS process, so it
    /// writes no pidfile and the reaper never touches it.
    Verus,
}

impl MinerId {
    /// Every miner Pasiv knows about — drives the startup reaper so a
    /// crash-orphaned sidecar of any kind gets cleaned up on next launch.
    pub fn all() -> &'static [MinerId] {
        &[MinerId::Xmrig, MinerId::SrbMiner, MinerId::Verus]
    }

    /// Pidfile stem — must equal the `id` string a supervisor derives from
    /// this variant (serde snake_case), since that's what `write_pidfile` uses.
    pub fn pidfile_stem(self) -> &'static str {
        match self {
            MinerId::Xmrig => "xmrig",
            MinerId::SrbMiner => "srb_miner",
            MinerId::Verus => "verus",
        }
    }

    /// Substring expected in the OS process name, for the reaper's identity
    /// check before it kills a recorded pid. Verus has no external process; the
    /// sentinel below can never match a real `comm`, so even if a stale `verus.pid`
    /// somehow existed the reaper would refuse to kill anything.
    pub fn process_comm(self) -> &'static str {
        match self {
            MinerId::Xmrig => "xmrig",
            MinerId::SrbMiner => "srbminer",
            MinerId::Verus => "\0pasiv-in-process-verus",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coin {
    Xmr,
    /// Zephyr — RandomX (rx/0), mined via XMRig on the XMRig-family path.
    Zeph,
    /// Salvium — RandomX (rx/0), mined via XMRig; post-fork Carrot ("SC1…")
    /// payout addresses. Same XMRig-family path as XMR/ZEPH.
    Sal,
    /// Verus — VerusHash V2.2 (AES-accelerated; the coin Apple Silicon is
    /// genuinely great at). Mined in-process (not XMRig-family) via the
    /// `verus` adapter. Address is 'R'-prefixed base58check.
    Vrsc,
    Prl,
    /// Ravencoin — KawPow on the GPU, mined via SRBMiner-Multi (`-a kawpow`), the
    /// same engine as Pearl. An ASIC-*resistant* GPU coin, so a consumer card
    /// holds real network share (unlike Kaspa/Alephium, whose ASIC networks a
    /// GPU can't touch). A second GPU coin, so it's a manual pick alongside PRL —
    /// one SRBMiner sidecar, one GPU coin at a time. Address is 'R'-prefixed
    /// base58check (Bitcoin-family version byte 60, same shape as VRSC).
    Rvn,
    /// Ergo — Autolykos2 on the GPU, mined via SRBMiner-Multi (`-a autolykos2`),
    /// the same engine as Pearl/Ravencoin: a roster row, not a new sidecar.
    /// ASIC-resistant (memory-hard by design), 1% engine dev fee (half of
    /// pearlhash's 2%), ~2.5 GB working set so it fits the 3–6 GB cards that
    /// KawPow's DAG refuses. Address is a '9'-prefixed base58 P2PK.
    Erg,
    /// Ethereum Classic — Etchash (Dagger-Hashimoto) on the GPU via
    /// SRBMiner-Multi. EXPERIMENTAL and dark by default (`CoinSpec.experimental`).
    /// The first roster coin whose memory requirement is a moving target: see
    /// `crate::etchash`.
    Etc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Stalled,
    Dead,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeviceStat {
    pub name: String,
    pub hashrate: f64,
}

/// Normalized stats shape — XMRig HTTP and SRBMiner API both map here.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MinerStats {
    /// Normalized to H/s.
    pub hashrate: f64,
    pub accepted: u64,
    pub rejected: u64,
    pub hottest_c: Option<f32>,
    pub devices: Vec<DeviceStat>,
}

#[derive(Debug, Clone)]
pub struct MinerConfig {
    /// Which coin this run mines — carried through so one parameterized
    /// adapter can serve several coins, and so stats/fee events name the coin.
    pub coin: Coin,
    pub pool_host: String,
    pub pool_port: u16,
    pub payout_address: String,
    pub tls: bool,
    /// None = all cores but one (the "runs quietly" default).
    pub threads: Option<u32>,
    /// Miner algorithm flag (e.g. XMRig `-a rx/wow`); None = the miner's
    /// default (RandomX `rx/0` for XMRig).
    pub algo: Option<String>,
}

pub type Result<T> = std::result::Result<T, MinerError>;

#[derive(Debug, thiserror::Error)]
pub enum MinerError {
    #[error("could not launch the miner: {0}")]
    Spawn(String),
    #[error("stats unavailable: {0}")]
    Stats(String),
    #[error("control failed: {0}")]
    Control(String),
    #[error("{0} not supported by this miner")]
    Unsupported(&'static str),
}
