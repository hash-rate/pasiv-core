# Pasiv — Architecture & Roadmap

*Cross-platform background miner. CPU → RandomX coins (Monero/Zephyr/Salvium) + in-process Verus, all via XMRig/native; GPU → Pearl/Ravencoin (SRBMiner). Data-driven coin roster (`coins/mod.rs`). Reown wallet sign-in, Supabase accounts, headless `pasivd` nodes. Tauri 2.*

---

## 1. The one-paragraph shape

Pasiv is a **thin Rust control plane wrapping proven miner binaries as sidecars**, with a webview frontend that is a pure view of state the Rust core owns. You never write hashing code. The core spawns/monitors miners, reads their local APIs, runs a resource governor, and drives a single explicit state machine that both the window and the tray render. Wallets are for *identity + payout addresses only* — the app never custodies funds, so mining works fully offline and the backend stays tiny.

```
┌──────────────────────────── Tauri App (one binary) ────────────────────────────┐
│                                                                                  │
│   WEBVIEW (frontend)                        RUST CORE (owns all state)           │
│   ┌────────────────────┐   commands  ┌──────────────────────────────────────┐   │
│   │ Reown AppKit (WC)   │ ──────────▶ │  commands ──▶ Core State Machine     │   │
│   │ Screens (UX flow)   │             │                 │  ▲                  │   │
│   │ Live view of state  │ ◀────────── │  events ◀───────┘  │                  │   │
│   └────────────────────┘   events     │            ┌───────┴────────┐         │   │
│                                        │  Supervisor │  Governor      │         │   │
│                                        │  (spawn/    │ (battery/temp/ │         │   │
│                                        │   backoff/  │  fullscreen)   │         │   │
│                                        │   watchdog) └───────┬────────┘         │   │
│   TRAY (native)                        │        │           │  Hardware detect  │   │
│   ┌────────────────────┐  commands     │        ▼           │  (nvml/cpu)       │   │
│   │ state + hashrate    │ ────────────▶ │   Miner adapters (trait objects)     │   │
│   │ start/stop/pause    │ ◀──────────── │   ┌──────────┐  ┌──────────────┐     │   │
│   └────────────────────┘  events        │   │ XMRig    │  │ SRBMiner     │ …   │   │
│                                          │   │ (CPU/XMR)│  │ (GPU/PRL)    │     │   │
│                                          └───┴────┬─────┴──┴──────┬───────┴─────┘   │
└────────────────────────────────────────────────┼───────────────┼──────────────────┘
             stats via localhost HTTP/JSON API ───┘               │  stratum+TLS
                                                                  ▼
                                            Pools: MoneroOcean (XMR) · HeroMiners (PRL)
   ┌───────────────────────┐
   │ Supabase (thin)       │  SIWX verify (edge fn) · user↔address map · optional cache
   └───────────────────────┘
```

---

## 2. Design principles (non-negotiable)

1. **Don't reinvent the miner.** XMRig and SRBMiner are sidecars, versioned and vendored per target triple. Your value is the shell, not the hash loop.
2. **Rust owns state; the UI is a projection.** No mining truth lives in JS. This is what prevents a fake "Mining" label — the tray and window read the same state enum.
3. **One trait, many miners.** Every miner conforms to a `Miner` interface. Adding a coin = adding an adapter, with zero supervisor changes.
4. **Motion is proof.** The Luster Ring animates only while shares are accepted. State is honest by construction.
5. **The backend is optional.** Mining requires no network to Supabase. Auth and earnings-cache are additive, never load-bearing.

---

## 3. Rust core modules (`src-tauri/src/`)

| Module | Owns | Notes |
|---|---|---|
| `core/` | The **state machine** — single source of truth | `Idle · Starting · Mining · Paused{reason} · Error{kind,retry}` |
| `miners/` | The `Miner` **trait** + adapters | `xmrig.rs`, `srbminer.rs`; each knows its args, API port, stats shape |
| `supervisor/` | Process lifecycle | Spawn, **bounded-backoff restart**, watchdog (0 H/s for 30 s → restart) |
| `stats/` | Poll miner local APIs → `MinerStats` | Normalize XMRig HTTP + SRBMiner API into one shape; emit on interval |
| `governor/` | Auto-pause rules | Battery, thermal threshold, fullscreen/GPU-busy detection → feeds state machine |
| `hardware/` | Detect CPU/GPU + capability | `nvml-wrapper` (or `nvidia-smi`); gate Pearl to Turing+ / ≥3 GB VRAM |
| `config/` | Persisted settings | Pools, payout addresses, threads, power rules, eco/full; `tauri-plugin-store` or SQLite |
| `fee/` | L1 hashrate-fee engine | Pasiv's 4%, time-sliced per MONETISATION.md §2; only in `Mining`; local ledger |
| `elevation/` | One-time hugepages/MSR helper | Highest-risk surface — keep minimal, signed, audited (see §8) |
| `tray/` | Native tray icon + menu | Renders state; sends commands; `TrayIconBuilder` |
| `deeplink/` | Wallet sign-in return | `tauri-plugin-deep-link` custom scheme for the SIWX callback |
| `commands/` | Tauri IPC handlers | `start`, `stop`, `apply_config`, `set_payout`, `wallet_callback` |

---

## 4. The keystone: the `Miner` trait

Everything hangs off this contract. Get it right first — the rest is plumbing against it.

```rust
#[async_trait]
pub trait Miner: Send + Sync {
    fn id(&self) -> MinerId;                 // Xmrig | SrbMiner | …
    fn coin(&self) -> Coin;                  // Xmr | Prl
    fn device_class(&self) -> DeviceClass;   // Cpu | Gpu

    /// Build args from config and spawn the sidecar (via Tauri shell/command).
    async fn start(&self, cfg: &MinerConfig) -> Result<MinerHandle>;

    /// Read the miner's local HTTP/JSON API and normalize.
    async fn stats(&self) -> Result<MinerStats>;

    /// Interpret stats into health for the watchdog.
    fn health(&self, s: &MinerStats) -> Health;   // Healthy | Stalled | Dead

    async fn stop(&self, handle: &mut MinerHandle) -> Result<()>;
}

pub struct MinerStats {
    pub hashrate: f64,          // normalized to H/s
    pub accepted: u64,
    pub rejected: u64,
    pub hottest_c: Option<f32>,
    pub devices: Vec<DeviceStat>,
}
```

- **XmrigAdapter** — spawns with `--http-enabled --http-port`, reads structured JSON from `localhost`. RandomX, all-but-N cores.
- **SrbMinerAdapter** — spawns SRBMiner per GPU coin (Pearl → LuckyPool `pearl-eu2.luckypool.io`, Ravencoin → HeroMiners), reads SRBMiner's HTTP API. Auto-detects CUDA devices; gated to NVIDIA Turing+ with the coin's VRAM floor.
- **Adding profit-switching or a new coin later** = a new adapter. The supervisor, governor, and UI never change.

---

## 5. The state machine (the honesty engine)

```
                 user:start
   ┌────┐  ─────────────────▶ ┌──────────┐  supervisor:first_share  ┌────────┐
   │Idle│                     │ Starting │ ───────────────────────▶ │ Mining │
   └────┘ ◀───────────────    └──────────┘                          └────────┘
      ▲     user:stop / all miners exited        governor:pause │ ▲ governor:resume
      │                                                         ▼ │
      │                                                    ┌──────────────┐
      └──────────────────── user:stop ────────────────────│ Paused{reason}│
                                                           └──────────────┘
   any state ── miner:crash (backoff exhausted) / pool:unreachable ──▶ ┌───────────────┐
                                                                       │ Error{kind}   │
   Error ── retry succeeds ──▶ Starting                                └───────────────┘
```

- Transitions are driven by **four inputs only**: user commands, supervisor events, governor signals, watchdog verdicts.
- `Paused{reason}` and `Error{kind}` carry their cause so the UI can *name the reason and the exit* (per the UX rules).
- **Partial failure is first-class:** GPU pool down → `Error` for that miner while CPU stays `Mining`. State is per-miner, rolled up for display.

---

## 6. Frontend & backend

**Frontend (webview):** React is the pragmatic pick — Reown AppKit + `wagmi` + the Solana adapter have their most mature bindings there, which de-risks *your stated hard problem* (wallets). SvelteKit is the lean alternative if you'd rather ship less JS; you'd trade some wallet-SDK maturity for it.

- Reown AppKit modal, QR-first (no `window.ethereum` in a webview), MetaMask + Phantom featured, extension listings disabled.
- Subscribes to Rust state events; renders the 9 screens. Zero business logic.
- CSP in `tauri.conf.json` must allow the WalletConnect relay (`wss://relay.walletconnect.org`) and Reown origins, or connections silently fail.

**Backend (Supabase, keep it thin):**
- **SIWX verify** edge function: signature in → session out → upsert `user ↔ address`.
- Optional: cache pool earnings for the "≈ $" row so it survives restarts. Never the source of truth — "verify on pool ↗" always links out.
- Web3 sign-in via Supabase Auth means no separate auth bill on top of what Wedding Sherpa already uses.

---

## 7. Tech stack

| Layer | Choice | Why |
|---|---|---|
| Shell | **Tauri 2** | Small binary, Rust core, native tray, sidecars, updater, deep-link |
| CPU miner | **XMRig** (sidecar) | Reference RandomX miner; HTTP API for clean stats |
| GPU miner | **SRBMiner** (sidecar) | Mature Pearl support; single binary; API |
| Frontend | **React** (+ Vite) | Best Reown/wagmi/Solana support |
| Wallet connect | **Reown AppKit** (free) | EVM + Solana in one SDK; QR desktop flow |
| Accounts | **Supabase** (SIWX + Postgres) | You already run it; thin, offline-tolerant |
| GPU detect | **nvml-wrapper** | Capability gating + temps |
| Persistence | `tauri-plugin-store` → SQLite if it grows | Config, addresses, rules |
| Updates | **Tauri updater** (signed) | Silent, safe auto-update |

---

## 8. Cross-cutting risks (treat as first-class)

- **Antivirus / SmartScreen / Gatekeeper.** This turned out subtler than "bundled XMRig will be flagged." Phase 0 on a real Windows box (docs/WINDOWS-PORT-PHASE0-RESULT.md) showed Defender leaving the Windows XMRig untouched and instead quarantining the **unsigned `pasiv.exe`** as a generic ML/reputation false positive. So the block is the unsigned, zero-reputation *bundle*, not the miner. **macOS: notarization is a hard gate — already done** (Developer ID, verified). **Windows: the cheapest fix is free** — ship `perMachine` + statically-linked CRT, submit the ML false positive to Microsoft, and let users click past SmartScreen ("Run anyway"); code-signing (Azure Trusted Signing ~$10/mo) removes that last click but is the *upgrade*, not the gate. See docs/WINDOWS-PORT.md.
  - **Revised at 0.3.8 (2026-07-27), on new evidence.** Two parts of the above did not survive contact with the GPU sidecar. (a) *"the block is the bundle, not the miner"* — no longer true: `srbminer.exe` is quarantined as `Trojan:Win32/Kepavll!rfn` while `pasiv.exe` and `xmrig.exe` are left alone, and Defender flags the **pristine upstream SRBMiner archive** too, so the detection is on unmodified third-party software. (b) *"ship `perMachine`"* as AV mitigation — the 0.3.8 quarantine happened to a `Program Files` install, so location bought nothing. Keep `perMachine` on its own merits, but it is not AV mitigation, and note it is what forces every update through a UAC prompt (`src/platform.ts::updateNeedsElevation`). Also: upstream SRBMiner is itself unsigned, so signing Pasiv cannot inoculate the sidecar — only a Microsoft FP determination clears the quarantine. Evidence and checksums: docs/SIDECAR-PROVENANCE.md, docs/WINDOWS-FALSE-POSITIVE.md.
- **The elevation helper is your biggest attack surface.** Hugepages/MSR needs admin once. Keep the privileged component tiny, signed, and doing exactly one thing. On macOS its scope is limited — set expectations that Full-power is mainly a Windows/Linux win.
- **Updates must be signed** end-to-end (Tauri updater keys), or you've built a malware delivery channel.
- **Telemetry is opt-in only.** This audience distrusts closed miners by default; any silent phone-home undoes the whole trust layer. Disclose the dev fee in-app (Pro), as the UX already does.

---


## 9. The fee-enforcement policy (where the private supervisor meets this core)

The proprietary desktop app's supervisor drives the fee slice against the
constants and schedule in `crates/pasiv-core/src/fee.rs`, with the same policy
`pasivd` implements in the open: the payout is hot-swapped to the fee address
only during an actual `Mining` slice (XMRig config hot-reload — the RandomX
dataset derives from the chain seed, not the login, so a swap costs nothing);
restarts always respawn on the user's address; and after **3 consecutive
failures to swap back to the user's address, mining stops entirely** rather
than continue on the fee address. Failing toward "not mining" instead of
"still mining to us" is the invariant; `pasivd/src/main.rs` is the runnable
reference of it.
