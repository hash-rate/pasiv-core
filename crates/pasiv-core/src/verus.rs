// SPDX-License-Identifier: GPL-3.0-only
//! The Verus (VRSC) mining ENGINE — native arm64 VerusHash V2.2
//! (`verushash-rs`, vendored under `vendor/`) driven by a small Rust stratum
//! client and a CPU worker pool. This is a complete in-process money path:
//! pool handshake, job assembly, hashing, share submission — open because a
//! miner's share submission is exactly the code a user shouldn't have to take
//! on faith. The proprietary desktop app wraps this engine in a thin adapter
//! (its `Miner` trait); the engine itself has no UI coupling.
//!
//! The exact LuckPool PBaaS byte layout here is not a guess: this logic landed
//! a real accepted share on live `na.luckpool.net:3956` (docs/VERUS.md §10).
//! VerusHash is AES-accelerated, so this is the one coin Apple Silicon is
//! genuinely great at. macOS-only for now: the vendored C++ is patched for
//! arm64/clang and does not build on Linux/GCC.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use verushash_rs::verus_hash_v2_2;

use crate::types::MinerConfig;

// --- The proven header/solution layout (docs/verus/VERUS.md §10) -------------
//
// Hashed block = 140B header (version‖prevhash‖merkleroot‖finalsaplinghash‖time‖
// bits‖nNonce) ‖ nSolution (compactsize `fd4005`=1344 ‖ 1344B solution). In the
// PBaaS "merged" path (solution version ≥7, solution[5]>0 — LuckPool's live
// mode) the mining nonce lives in the SOLUTION's last 15 bytes, not the header
// nNonce, and several header/solution fields are zeroed before hashing.
const HEADER_LEN: usize = 1487;
const SOLUTION_LEN: usize = 1344;
/// Solution start within the 1487B buffer: 108 prefix + 32 nNonce + 3 compactsize.
const SOL_OFF: usize = 143;
/// Within the 1344B solution: the 15-byte nonceSpace = [pool nonce(4)][round(4)]
/// [thrd id(1)][pad(2)][counting nonce(4)] at solution[1329..1344].
const NS_POOL_NONCE: usize = 1329;
const NS_THRD: usize = 1337;
const NS_COUNTING: usize = 1340;
/// The same fields addressed inside the 1487B hashing buffer.
const COUNTING_IN_HEADER: usize = SOL_OFF + NS_COUNTING; // 1483
const THRD_IN_HEADER: usize = SOL_OFF + NS_THRD; // 1480
/// nNonce[4..32] — the 28 bytes submitted as `noncestr` (pool re-prepends its
/// extranonce). In merged mode these are zero (the nonce is in the solution).
const NNONCE_TAIL: std::ops::Range<usize> = 112..140;

/// How many hashes a worker runs between job/shutdown checks — amortises the
/// lock/atomic traffic without letting a new job or a stop wait more than a few ms.
const CHUNK: u32 = 2048;

/// Assemble the fixed part of the 1487B hashing buffer and the 1344B submit
/// solution for a job. Pure, so it's unit-tested against the proven offsets. The
/// caller stamps the 4-byte counting nonce at `COUNTING_IN_HEADER` each iteration
/// (and into the solution at `NS_COUNTING` on a hit), plus the 1-byte thread id.
/// Returns `(header, solution, merged)`.
pub fn assemble(prefix: &[u8], extranonce1: &[u8], template: &[u8]) -> (Vec<u8>, Vec<u8>, bool) {
    let mut solution = vec![0u8; SOLUTION_LEN];
    let n = template.len().min(SOLUTION_LEN);
    solution[..n].copy_from_slice(&template[..n]);
    let version = solution[0];
    let merged = version >= 7 && solution[5] > 0;
    if extranonce1.len() == 4 {
        solution[NS_POOL_NONCE..NS_POOL_NONCE + 4].copy_from_slice(extranonce1);
    }

    let mut header = vec![0u8; HEADER_LEN];
    let pn = prefix.len().min(108);
    header[..pn].copy_from_slice(&prefix[..pn]);
    if extranonce1.len() == 4 {
        header[108..112].copy_from_slice(extranonce1); // nNonce[0..4]
    }
    header[140..143].copy_from_slice(&[0xfd, 0x40, 0x05]); // compactsize(1344)
    header[SOL_OFF..].copy_from_slice(&solution);

    if merged {
        // Canonicalisation the pool also performs before hashing.
        header[4..100].iter_mut().for_each(|b| *b = 0); // prevhash+merkle+finalsapling
        header[104..140].iter_mut().for_each(|b| *b = 0); // nBits + nNonce
        header[151..215].iter_mut().for_each(|b| *b = 0); // solution[8..72]
    }
    (header, solution, merged)
}

#[derive(Clone, Default)]
struct Job {
    id: String,
    /// version‖prevhash‖merkleroot‖finalsaplinghash‖time‖bits (108B).
    prefix: Vec<u8>,
    /// The 125B PBaaS solution template.
    template: Vec<u8>,
    time: String,
}

/// A found share, handed from a worker to the stratum writer to submit.
pub struct Solve {
    job_id: String,
    time: String,
    noncestr: String,
    solhex: String,
}

/// Everything a running Verus miner shares between its stratum thread and worker
/// threads. One is created per `start()` and torn down on `stop()`.
pub struct Engine {
    pub shutdown: AtomicBool,
    pub paused: AtomicBool,
    /// Set by the reader when the socket drops, so the writer loop reconnects.
    link_down: AtomicBool,
    /// Bumped on every job/target/extranonce change; workers re-snapshot when it moves.
    gen: AtomicU64,
    job: Mutex<Option<Job>>,
    target: Mutex<[u8; 32]>,
    extranonce1: Mutex<Vec<u8>>,
    submit_tx: mpsc::Sender<Solve>,
    pub hashes: AtomicU64,
    pub accepted: AtomicU64,
    pub rejected: AtomicU64,
    /// (instant, cumulative hashes) at the previous `stats()` call — for the rate.
    pub rate_prev: Mutex<(Instant, u64)>,
    pub handles: Mutex<Vec<JoinHandle<()>>>,
}

impl Engine {
    pub fn new(submit_tx: mpsc::Sender<Solve>) -> Self {
        Self {
            shutdown: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            link_down: AtomicBool::new(false),
            gen: AtomicU64::new(0),
            job: Mutex::new(None),
            target: Mutex::new([0xff; 32]),
            extranonce1: Mutex::new(Vec::new()),
            submit_tx,
            hashes: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            rate_prev: Mutex::new((Instant::now(), 0)),
            handles: Mutex::new(Vec::new()),
        }
    }

    pub fn stopping(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }
}

fn hexd(s: &str) -> Vec<u8> {
    hex::decode(s).unwrap_or_default()
}

fn send_line(w: &mut impl Write, v: &Value) -> io::Result<()> {
    let mut s = v.to_string();
    s.push('\n');
    w.write_all(s.as_bytes())
}

fn read_loop(rd: TcpStream, engine: Arc<Engine>) {
    let reader = BufReader::new(rd);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if engine.stopping() {
            break;
        }
        let Ok(m) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        // Subscribe reply carries the pool's extranonce1 (the "pool nonce").
        if m.get("id") == Some(&json!(1)) {
            if let Some(e) = m["result"].get(1).and_then(|v| v.as_str()) {
                *engine.extranonce1.lock().unwrap() = hexd(e);
                engine.gen.fetch_add(1, Ordering::Release);
            }
            continue;
        }
        // Submit acknowledgements (our ids start at 100).
        if let Some(rid) = m.get("id").and_then(|v| v.as_u64()) {
            if rid >= 100 {
                if m["result"].as_bool() == Some(true) {
                    engine.accepted.fetch_add(1, Ordering::Relaxed);
                } else {
                    engine.rejected.fetch_add(1, Ordering::Relaxed);
                }
                continue;
            }
        }
        match m["method"].as_str() {
            Some("mining.set_target") => {
                if let Some(t) = m["params"][0].as_str() {
                    let b = hexd(t);
                    if b.len() == 32 {
                        engine.target.lock().unwrap().copy_from_slice(&b);
                        engine.gen.fetch_add(1, Ordering::Release);
                    }
                }
            }
            Some("mining.notify") => {
                let p = &m["params"];
                let g = |i: usize| p[i].as_str().unwrap_or("").to_string();
                let mut prefix = Vec::with_capacity(108);
                for i in 1..=6 {
                    prefix.extend(hexd(&g(i)));
                }
                *engine.job.lock().unwrap() = Some(Job {
                    id: g(0),
                    prefix,
                    template: hexd(&g(8)),
                    time: g(5),
                });
                engine.gen.fetch_add(1, Ordering::Release);
            }
            _ => {}
        }
    }
    engine.link_down.store(true, Ordering::Release);
}

/// The stratum thread: connect → session → reconnect with backoff, until stop.
/// Owns the submit `Receiver` (single consumer).
pub fn stratum_thread(
    engine: Arc<Engine>,
    cfg: MinerConfig,
    rx: mpsc::Receiver<Solve>,
    worker: String,
) {
    while !engine.stopping() {
        if run_session(&engine, &cfg, &rx, &worker).is_err() {
            engine.link_down.store(true, Ordering::Release);
        }
        if engine.stopping() {
            break;
        }
        // Reconnect backoff (~2s), but stay responsive to stop.
        for _ in 0..20 {
            if engine.stopping() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// One connected session: handshake, spawn the reader, then drain submits from
/// `rx` until the socket drops or we're told to stop.
fn run_session(
    engine: &Arc<Engine>,
    cfg: &MinerConfig,
    rx: &mpsc::Receiver<Solve>,
    worker: &str,
) -> io::Result<()> {
    let stream = TcpStream::connect((cfg.pool_host.as_str(), cfg.pool_port))?;
    stream.set_nodelay(true).ok();
    engine.link_down.store(false, Ordering::Release);

    let mut wr = stream.try_clone()?;
    let agent = concat!("pasiv-core/", env!("CARGO_PKG_VERSION"));
    send_line(
        &mut wr,
        &json!({"id":1,"method":"mining.subscribe","params":[agent]}),
    )?;
    send_line(
        &mut wr,
        &json!({"id":2,"method":"mining.extranonce.subscribe","params":[]}),
    )?;
    send_line(
        &mut wr,
        &json!({"id":3,"method":"mining.authorize","params":[worker,"x"]}),
    )?;

    let rd = stream.try_clone()?;
    let reader_engine = engine.clone();
    let reader = std::thread::spawn(move || read_loop(rd, reader_engine));

    let mut id = 100u64;
    let result = loop {
        if engine.stopping() || engine.link_down.load(Ordering::Acquire) {
            break Ok(());
        }
        match rx.recv_timeout(Duration::from_millis(400)) {
            Ok(s) => {
                let line = json!({
                    "id": id,
                    "method": "mining.submit",
                    "params": [worker, s.job_id, s.time, s.noncestr, s.solhex],
                });
                id += 1;
                if let Err(e) = send_line(&mut wr, &line) {
                    break Err(e);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break Ok(()),
        }
    };

    let _ = stream.shutdown(Shutdown::Both);
    let _ = reader.join();
    result
}

/// A CPU worker: hash the current job over its slice of the counting-nonce space
/// and submit any share at or below target.
pub fn worker_thread(engine: Arc<Engine>, wid: u8, stride: u32) {
    let mut local_gen = u64::MAX;
    let mut header: Vec<u8> = Vec::new();
    let mut solution: Vec<u8> = Vec::new();
    let mut job_id = String::new();
    let mut job_time = String::new();
    let mut target = [0xffu8; 32];
    let mut ctr: u32 = wid as u32;

    loop {
        if engine.stopping() {
            return;
        }
        if engine.paused.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(120));
            continue;
        }

        let gen = engine.gen.load(Ordering::Acquire);
        if gen != local_gen {
            let job = engine.job.lock().unwrap().clone();
            let e1 = engine.extranonce1.lock().unwrap().clone();
            target = *engine.target.lock().unwrap();
            match job {
                Some(j) if j.prefix.len() >= 108 && e1.len() == 4 => {
                    let (h, s, _merged) = assemble(&j.prefix, &e1, &j.template);
                    header = h;
                    solution = s;
                    header[THRD_IN_HEADER] = wid;
                    solution[NS_THRD] = wid;
                    job_id = j.id;
                    job_time = j.time;
                    local_gen = gen;
                    ctr = wid as u32; // fresh nonce space per job
                }
                _ => {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            }
        }

        for _ in 0..CHUNK {
            header[COUNTING_IN_HEADER..COUNTING_IN_HEADER + 4].copy_from_slice(&ctr.to_le_bytes());
            let mut hr = verus_hash_v2_2(&header);
            hr.reverse(); // little-endian hash → big-endian for the target compare
            if hr <= target {
                solution[NS_COUNTING..NS_COUNTING + 4].copy_from_slice(&ctr.to_le_bytes());
                let noncestr = hex::encode(&header[NNONCE_TAIL]);
                let solhex = format!("fd4005{}", hex::encode(&solution));
                let _ = engine.submit_tx.send(Solve {
                    job_id: job_id.clone(),
                    time: job_time.clone(),
                    noncestr,
                    solhex,
                });
            }
            ctr = ctr.wrapping_add(stride);
        }
        engine.hashes.fetch_add(CHUNK as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PBaaS-v8 solution template: version 8, byte[5]>0 ⇒ merged path.
    fn v8_template() -> Vec<u8> {
        let mut t = vec![0u8; 125];
        t[0] = 8; // solution version
        t[5] = 1; // merged flag
                  // some non-zero bytes in solution[8..72] to prove they get canonicalised
        for (i, b) in t.iter_mut().enumerate().take(72).skip(8) {
            *b = (i as u8).wrapping_add(1);
        }
        t
    }

    #[test]
    fn assemble_produces_the_proven_1487b_layout() {
        let mut prefix = vec![0u8; 108];
        // Put a recognisable value in the time field (bytes 100..104) — merged
        // canonicalisation must KEEP it (only prevhash/merkle/finalsapling/bits/
        // nNonce are zeroed).
        prefix[100..104].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
        prefix[4] = 0x11; // inside prevhash — must be zeroed by merged
        let e1 = [0x6d, 0x86, 0x6d, 0x33];
        let (header, solution, merged) = assemble(&prefix, &e1, &v8_template());

        assert!(merged, "version 8 + solution[5]>0 is the merged path");
        assert_eq!(header.len(), HEADER_LEN);
        assert_eq!(solution.len(), SOLUTION_LEN);

        // compactsize marker + pool nonce in the solution tail.
        assert_eq!(&header[140..143], &[0xfd, 0x40, 0x05]);
        assert_eq!(&solution[NS_POOL_NONCE..NS_POOL_NONCE + 4], &e1);

        // Merged canonicalisation: prevhash zeroed, time kept, nNonce zeroed,
        // solution[8..72] zeroed.
        assert_eq!(header[4], 0, "prevhash byte must be zeroed");
        assert_eq!(&header[100..104], &[0xaa, 0xbb, 0xcc, 0xdd], "time is kept");
        assert!(header[108..140].iter().all(|&b| b == 0), "nNonce zeroed");
        assert!(
            header[151..215].iter().all(|&b| b == 0),
            "solution[8..72] zeroed"
        );

        // The header's solution region mirrors the submit solution's tail.
        assert_eq!(
            &header[SOL_OFF + NS_POOL_NONCE..SOL_OFF + NS_POOL_NONCE + 4],
            &e1
        );
    }

    #[test]
    fn non_merged_keeps_the_header_extranonce() {
        // Version < 7 ⇒ classic path, no canonicalisation; extranonce stays in nNonce.
        let prefix = vec![0u8; 108];
        let e1 = [1, 2, 3, 4];
        let mut template = vec![0u8; 125];
        template[0] = 4; // pre-PBaaS version
        let (header, _solution, merged) = assemble(&prefix, &e1, &template);
        assert!(!merged);
        assert_eq!(&header[108..112], &e1, "nNonce keeps the extranonce");
    }

    #[test]
    fn short_prefix_and_template_do_not_panic() {
        // Defensive: a truncated notify must not index out of bounds.
        let (header, solution, _) = assemble(&[0u8; 10], &[], &[8, 0, 0, 0, 0, 1]);
        assert_eq!(header.len(), HEADER_LEN);
        assert_eq!(solution.len(), SOLUTION_LEN);
    }
}
