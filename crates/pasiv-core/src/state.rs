// SPDX-License-Identifier: GPL-3.0-only
//! The state machine — single source of truth (ARCHITECTURE.md §5).
//!
//! Transitions are driven by four inputs only: user commands, supervisor
//! events, governor signals, watchdog verdicts. The tray and the window both
//! render this enum; the fee engine (M5) is driven by it too, which is what
//! makes "never charge a paused user" structural rather than promised.
//!
//! v2 (START-CONTROL-V2.md): the webview renders state, it never derives it.
//! Warm-up phases, the Stopping hop, and the reason an Idle is Idle all live
//! HERE — the four v1 defects were all the same bug: UI inferring state the
//! core already knew, or the core not tracking state the user could see.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    #[allow(dead_code)] // governor lands in M2
    Battery,
    #[allow(dead_code)]
    Thermal,
    #[allow(dead_code)]
    Fullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    MinerCrashed,
    SpawnFailed,
    /// Miner alive but can't reach the pool — detected from its connect logs
    /// plus a stuck-in-Starting timeout (supervisor).
    PoolUnreachable,
    /// Stop was requested and even the force-kill failed: the process is still
    /// running and the UI must say so — pretending to be Idle while a sidecar
    /// still hashes is the one failure mode that costs real money.
    StopFailed,
}

/// Why an Idle lane is Idle. Every route to Idle names how it got there —
/// "you stopped it", "it gave up", and "the app stopped itself" must never
/// render as the same silent nothing (v2 defect #4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleReason {
    /// Never started this session — the first-run / post-launch resting state.
    Fresh,
    /// The user pressed Stop (or the tray toggle).
    UserStopped,
    /// An unrecoverable error was acknowledged (a retry press uses UserStart).
    GaveUp,
    /// The app stopped itself: the fee payout swap-back failed repeatedly, so
    /// mining stopped rather than continue on the fee address (supervisor).
    Failsafe,
}

/// Warm-up phases, in DISPLAY order — the UI maps phase → arc fraction, and a
/// miner that skips a phase just jumps forward. Ordered so `Warm` can be
/// monotonic: XMRig re-logs "use pool" on reconnect, and that must never walk
/// the ring backwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarmPhase {
    /// Process spawned, pid acquired.
    Spawning,
    /// Process is up, pool login in flight.
    Connecting,
    /// RandomX dataset / hugepages init — the long one.
    Allocating,
    /// Connected, hashing not yet confirmed — genuinely unbounded.
    AwaitingShare,
}

/// `Paused` and `Error` carry their cause so the UI can name the reason and
/// the exit, per the UX rules. `Starting` carries the phase — not a progress
/// float; progress is a presentation concern (a core that emits 0.42 is a core
/// with opinions about a ring). `Idle` carries why.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MinerState {
    Idle {
        reason: IdleReason,
    },
    Starting {
        phase: WarmPhase,
    },
    Mining,
    /// Stop was requested; the process is not yet confirmed gone. Holds until
    /// `Exited` — never a bare timeout (invariant: "Stop means stopped").
    Stopping,
    Paused {
        reason: PauseReason,
    },
    Error {
        kind: ErrorKind,
    },
}

/// The only four input families (§5).
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    // user commands
    UserStart,
    /// The entry to shutdown (replaces v1's UserStop): the state hops to
    /// `Stopping` immediately; `Idle` lands only when `Exited` proves the
    /// process is gone.
    StopRequested,
    // supervisor events
    /// The miner is producing hashes — it has a job from the pool and is
    /// working. This is what "Mining" means to a user: the machine is on it.
    /// We transition on this rather than the first *accepted share*, because a
    /// share can take many minutes to land at a laptop's hashrate — leaving the
    /// UI stuck on "warming up" (blank hashrate, no earnings) while the miner is
    /// plainly working. A share is a pool-side confirmation, not a prerequisite.
    Hashing,
    FirstShare,
    /// A warm-up signal was observed (spawn ok / dataset alloc / pool job).
    /// Monotonic: ranked by `WarmPhase` order, never regresses.
    Warm(WarmPhase),
    /// The sidecar process is confirmed gone (Terminated event, a verified
    /// force-kill, or an in-process adapter's stop() returning).
    Exited,
    MinerExitedRetrying,
    BackoffExhausted(ErrorKind),
    SpawnFailed,
    /// The miner is alive but can't reach the pool (detected from its logs +
    /// a stuck-in-Starting timeout). Distinct from a crash so the UI can say
    /// "check your connection" instead of "the miner keeps stopping".
    PoolUnreachable,
    /// The fee payout swap-back failed repeatedly; the app stopped itself.
    /// Named so the user sees the failsafe copy, not a silent Idle.
    FeeFailsafe,
    /// The user acknowledged an error without retrying.
    #[allow(dead_code)] // wired to the UI in v2 PR 2
    Dismiss,
    // governor signals (M2)
    #[allow(dead_code)]
    GovernorPause(PauseReason),
    #[allow(dead_code)]
    GovernorResume,
    // watchdog verdicts
    WatchdogRestart,
}

/// Pure transition table. `None` = input is not legal in this state (ignored).
pub fn transition(state: &MinerState, input: &Input) -> Option<MinerState> {
    use Input as I;
    use MinerState as S;
    match (state, input) {
        (S::Idle { .. } | S::Error { .. }, I::UserStart) => Some(S::Starting {
            phase: WarmPhase::Spawning,
        }),
        // Monotonic warm-up: take the max, and only emit when it MOVED — a
        // repeated or backwards signal is a no-op, not an event.
        (S::Starting { phase }, I::Warm(q)) => (q > phase).then_some(S::Starting { phase: *q }),
        // Hashing → Mining is the primary path (the miner is working). FirstShare
        // is kept as a fallback for any miner whose stats report shares before a
        // steady hashrate, so it can never miss the transition.
        (S::Starting { .. }, I::Hashing | I::FirstShare) => Some(S::Mining),
        // The stop hop: every active state → Stopping, immediately. Idle lands
        // only on Exited — the state machine never times out into Idle while a
        // sidecar might still be hashing.
        (S::Starting { .. } | S::Mining | S::Paused { .. } | S::Error { .. }, I::StopRequested) => {
            Some(S::Stopping)
        }
        (S::Stopping, I::Exited) => Some(S::Idle {
            reason: IdleReason::UserStopped,
        }),
        // Even the force-kill failed: surface it, never pretend Idle.
        (S::Stopping, I::BackoffExhausted(kind)) => Some(S::Error { kind: *kind }),
        (S::Mining, I::GovernorPause(reason)) => Some(S::Paused { reason: *reason }),
        (S::Paused { .. }, I::GovernorResume) => Some(S::Mining),
        (S::Starting { .. } | S::Mining, I::MinerExitedRetrying) => Some(S::Starting {
            phase: WarmPhase::Spawning,
        }),
        (S::Mining, I::WatchdogRestart) => Some(S::Starting {
            phase: WarmPhase::Spawning,
        }),
        (S::Starting { .. } | S::Mining, I::BackoffExhausted(kind)) => {
            Some(S::Error { kind: *kind })
        }
        (S::Starting { .. } | S::Mining, I::PoolUnreachable) => Some(S::Error {
            kind: ErrorKind::PoolUnreachable,
        }),
        (S::Idle { .. } | S::Starting { .. }, I::SpawnFailed) => Some(S::Error {
            kind: ErrorKind::SpawnFailed,
        }),
        // The app stopped itself (fee swap-back failsafe) — an Idle that says so.
        (S::Starting { .. } | S::Mining, I::FeeFailsafe) => Some(S::Idle {
            reason: IdleReason::Failsafe,
        }),
        (S::Error { .. }, I::Dismiss) => Some(S::Idle {
            reason: IdleReason::GaveUp,
        }),
        // A start press during Stopping is queued by the caller, never a
        // transition — the button's meaning must not depend on invisible
        // history. (No arm: falls through to None.)
        _ => None,
    }
}

/// Display rank: the most alive state wins the tray. Public so the fleet can
/// compute `degraded` from the same ordering the rollup uses.
pub fn rank(s: &MinerState) -> u8 {
    match s {
        MinerState::Idle { .. } => 0,
        MinerState::Error { .. } => 1,
        MinerState::Paused { .. } => 2,
        MinerState::Starting { .. } => 3,
        // Above Starting, below Mining: a stop press wins the tray immediately,
        // but never outranks a lane that is still genuinely mining.
        MinerState::Stopping => 4,
        MinerState::Mining => 5,
    }
}

/// Display rollup across miners (§5): the most alive state wins the tray;
/// per-miner rows carry the detail. Mining > Stopping > Starting > Paused >
/// Error > Idle, so a dead GPU never hides a healthy CPU — and vice versa an
/// Error shows only when nothing is running.
pub fn rollup<'a, I>(states: I) -> MinerState
where
    I: IntoIterator<Item = &'a MinerState>,
{
    let mut best = MinerState::Idle {
        reason: IdleReason::Fresh,
    };
    for s in states {
        if rank(s) > rank(&best) {
            best = s.clone();
        }
    }
    best
}

impl MinerState {
    /// Short human label; the UI owns richer copy.
    pub fn label(&self) -> String {
        match self {
            MinerState::Idle {
                reason: IdleReason::GaveUp,
            } => "stopped (gave up)".into(),
            MinerState::Idle {
                reason: IdleReason::Failsafe,
            } => "stopped (failsafe)".into(),
            MinerState::Idle { .. } => "idle".into(),
            MinerState::Starting { .. } => "warming up".into(),
            MinerState::Mining => "mining".into(),
            MinerState::Stopping => "stopping".into(),
            MinerState::Paused { reason } => format!("paused ({reason:?})").to_lowercase(),
            MinerState::Error { kind } => format!("error ({kind:?})").to_lowercase(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Input as I;
    use MinerState as S;

    fn idle() -> S {
        S::Idle {
            reason: IdleReason::Fresh,
        }
    }
    fn starting(p: WarmPhase) -> S {
        S::Starting { phase: p }
    }

    #[test]
    fn happy_path_with_the_stop_hop() {
        let s = transition(&idle(), &I::UserStart).unwrap();
        assert_eq!(s, starting(WarmPhase::Spawning));
        let s = transition(&s, &I::Warm(WarmPhase::Connecting)).unwrap();
        let s = transition(&s, &I::Warm(WarmPhase::AwaitingShare)).unwrap();
        assert_eq!(s, starting(WarmPhase::AwaitingShare));
        let s = transition(&s, &I::Hashing).unwrap();
        assert_eq!(s, S::Mining);
        // Stop is a hop, not a jump: Mining → Stopping → (Exited) → Idle, and
        // the Idle knows the user did it.
        let s = transition(&s, &I::StopRequested).unwrap();
        assert_eq!(s, S::Stopping);
        let s = transition(&s, &I::Exited).unwrap();
        assert_eq!(
            s,
            S::Idle {
                reason: IdleReason::UserStopped
            }
        );
    }

    #[test]
    fn warm_is_monotonic_and_silent_when_it_does_not_move() {
        // Forward moves emit.
        let s = starting(WarmPhase::Spawning);
        let s = transition(&s, &I::Warm(WarmPhase::Allocating)).unwrap();
        assert_eq!(s, starting(WarmPhase::Allocating));
        // A repeat or a regression (XMRig re-logs "use pool" on reconnect) is
        // ignored — the ring never walks backwards, and no event is emitted.
        assert_eq!(transition(&s, &I::Warm(WarmPhase::Allocating)), None);
        assert_eq!(transition(&s, &I::Warm(WarmPhase::Connecting)), None);
        // Warm means nothing outside Starting.
        assert_eq!(
            transition(&S::Mining, &I::Warm(WarmPhase::Connecting)),
            None
        );
        assert_eq!(transition(&idle(), &I::Warm(WarmPhase::Connecting)), None);
    }

    #[test]
    fn stopping_holds_until_exit_is_confirmed() {
        let s = transition(&S::Mining, &I::StopRequested).unwrap();
        assert_eq!(s, S::Stopping);
        // Nothing else moves it to Idle — not a start press, not a share, not
        // a governor signal. Only Exited (or the force-kill failure surfacing).
        for i in [
            I::UserStart,
            I::StopRequested,
            I::Hashing,
            I::FirstShare,
            I::Warm(WarmPhase::AwaitingShare),
            I::MinerExitedRetrying,
            I::SpawnFailed,
            I::PoolUnreachable,
            I::FeeFailsafe,
            I::GovernorPause(PauseReason::Battery),
            I::GovernorResume,
            I::WatchdogRestart,
            I::Dismiss,
        ] {
            assert_eq!(transition(&S::Stopping, &i), None, "Stopping --{i:?}-->");
        }
        assert_eq!(
            transition(&S::Stopping, &I::Exited),
            Some(S::Idle {
                reason: IdleReason::UserStopped
            })
        );
        // The unkillable-process escape: surface StopFailed, never fake Idle.
        assert_eq!(
            transition(&S::Stopping, &I::BackoffExhausted(ErrorKind::StopFailed)),
            Some(S::Error {
                kind: ErrorKind::StopFailed
            })
        );
    }

    #[test]
    fn failsafe_idle_names_itself() {
        for from in [S::Mining, starting(WarmPhase::AwaitingShare)] {
            assert_eq!(
                transition(&from, &I::FeeFailsafe),
                Some(S::Idle {
                    reason: IdleReason::Failsafe
                })
            );
        }
        assert_eq!(transition(&idle(), &I::FeeFailsafe), None);
    }

    #[test]
    fn mining_is_only_reachable_via_hashing_share_or_resume() {
        // Honesty by construction: no input takes Idle or Error straight to Mining.
        for input in [
            I::UserStart,
            I::MinerExitedRetrying,
            I::WatchdogRestart,
            I::GovernorResume,
        ] {
            assert_ne!(transition(&idle(), &input), Some(S::Mining));
            assert_ne!(
                transition(
                    &S::Error {
                        kind: ErrorKind::MinerCrashed
                    },
                    &input
                ),
                Some(S::Mining)
            );
        }
    }

    #[test]
    fn crash_retries_then_exhausts() {
        let s = transition(&S::Mining, &I::MinerExitedRetrying).unwrap();
        assert_eq!(s, starting(WarmPhase::Spawning));
        let s = transition(&s, &I::BackoffExhausted(ErrorKind::MinerCrashed)).unwrap();
        assert_eq!(
            s,
            S::Error {
                kind: ErrorKind::MinerCrashed
            }
        );
        // and the user can retry out of Error — a fresh warm-up from the seed.
        assert_eq!(
            transition(&s, &I::UserStart),
            Some(starting(WarmPhase::Spawning))
        );
        // or acknowledge it, landing in an Idle that says it gave up.
        assert_eq!(
            transition(&s, &I::Dismiss),
            Some(S::Idle {
                reason: IdleReason::GaveUp
            })
        );
    }

    #[test]
    fn hashing_reaches_mining_without_waiting_for_a_share() {
        // The primary path: the miner is producing hashes → Mining, so the UI
        // leaves "warming up" and shows the live hashrate promptly.
        assert_eq!(
            transition(&starting(WarmPhase::AwaitingShare), &I::Hashing),
            Some(S::Mining)
        );
        // A share still works as the fallback path.
        assert_eq!(
            transition(&starting(WarmPhase::Connecting), &I::FirstShare),
            Some(S::Mining)
        );
        // Neither promotes from Idle — only a running miner in Starting.
        assert_eq!(transition(&idle(), &I::Hashing), None);
        // Hashing while already Mining is a no-op (idempotent).
        assert_eq!(transition(&S::Mining, &I::Hashing), None);
    }

    #[test]
    fn governor_pause_names_its_reason_and_resumes() {
        let s = transition(&S::Mining, &I::GovernorPause(PauseReason::Battery)).unwrap();
        assert_eq!(
            s,
            S::Paused {
                reason: PauseReason::Battery
            }
        );
        assert_eq!(transition(&s, &I::GovernorResume), Some(S::Mining));
    }

    #[test]
    fn watchdog_restart_only_fires_from_mining() {
        assert_eq!(transition(&idle(), &I::WatchdogRestart), None);
        assert_eq!(
            transition(&starting(WarmPhase::Spawning), &I::WatchdogRestart),
            None
        );
        assert_eq!(
            transition(&S::Mining, &I::WatchdogRestart),
            Some(starting(WarmPhase::Spawning))
        );
    }

    #[test]
    fn illegal_inputs_are_ignored() {
        assert_eq!(transition(&idle(), &I::StopRequested), None);
        assert_eq!(transition(&S::Mining, &I::UserStart), None);
        assert_eq!(transition(&idle(), &I::FirstShare), None);
        assert_eq!(transition(&idle(), &I::Exited), None);
        assert_eq!(transition(&S::Mining, &I::Exited), None);
    }

    #[test]
    fn pool_unreachable_surfaces_a_distinct_error() {
        // From Starting (never got a first share) and from Mining (pool
        // dropped) it lands in a named PoolUnreachable error, and the user
        // can retry out of it.
        for from in [starting(WarmPhase::AwaitingShare), S::Mining] {
            let s = transition(&from, &I::PoolUnreachable).unwrap();
            assert_eq!(
                s,
                S::Error {
                    kind: ErrorKind::PoolUnreachable
                }
            );
            assert_eq!(
                transition(&s, &I::UserStart),
                Some(starting(WarmPhase::Spawning))
            );
        }
        // Not legal from Idle — the miner isn't even running.
        assert_eq!(transition(&idle(), &I::PoolUnreachable), None);
    }

    fn all_states() -> Vec<S> {
        let mut v = vec![S::Mining, S::Stopping];
        for reason in [
            IdleReason::Fresh,
            IdleReason::UserStopped,
            IdleReason::GaveUp,
            IdleReason::Failsafe,
        ] {
            v.push(S::Idle { reason });
        }
        for phase in [
            WarmPhase::Spawning,
            WarmPhase::Connecting,
            WarmPhase::Allocating,
            WarmPhase::AwaitingShare,
        ] {
            v.push(S::Starting { phase });
        }
        for reason in [PauseReason::Battery, PauseReason::Thermal] {
            v.push(S::Paused { reason });
        }
        for kind in [
            ErrorKind::MinerCrashed,
            ErrorKind::SpawnFailed,
            ErrorKind::PoolUnreachable,
            ErrorKind::StopFailed,
        ] {
            v.push(S::Error { kind });
        }
        v
    }

    fn all_inputs() -> Vec<I> {
        vec![
            I::UserStart,
            I::StopRequested,
            I::Hashing,
            I::FirstShare,
            I::Warm(WarmPhase::Spawning),
            I::Warm(WarmPhase::Connecting),
            I::Warm(WarmPhase::Allocating),
            I::Warm(WarmPhase::AwaitingShare),
            I::Exited,
            I::MinerExitedRetrying,
            I::BackoffExhausted(ErrorKind::MinerCrashed),
            I::BackoffExhausted(ErrorKind::StopFailed),
            I::SpawnFailed,
            I::PoolUnreachable,
            I::FeeFailsafe,
            I::Dismiss,
            I::GovernorPause(PauseReason::Battery),
            I::GovernorPause(PauseReason::Thermal),
            I::GovernorResume,
            I::WatchdogRestart,
        ]
    }

    /// Exhaustive sweep: global invariants over every (state, input) pair.
    /// These are the honesty guarantees the UI and fee engine lean on.
    #[test]
    fn invariants_hold_over_the_entire_table() {
        for s in all_states() {
            for i in all_inputs() {
                let Some(next) = transition(&s, &i) else {
                    continue;
                };

                // 1. Mining is only ever entered via real work (hashes or a
                //    share) or a governor resume.
                if next == S::Mining && s != S::Mining {
                    assert!(
                        matches!(i, I::Hashing | I::FirstShare | I::GovernorResume),
                        "{s:?} --{i:?}--> Mining is a lie"
                    );
                }
                // 2. Paused is only ever entered from Mining, by the governor,
                //    and always names the governor's reason.
                if let S::Paused { reason } = &next {
                    if !matches!(s, S::Paused { .. }) {
                        assert_eq!(s, S::Mining, "{s:?} --{i:?}--> Paused skipped Mining");
                        assert!(
                            matches!(i, I::GovernorPause(r) if r == *reason),
                            "pause reason must come from the governor input"
                        );
                    }
                }
                // 3. Error always carries the kind of the input that caused it.
                if let S::Error { kind } = &next {
                    match &i {
                        I::BackoffExhausted(k) => assert_eq!(k, kind),
                        I::SpawnFailed => assert_eq!(*kind, ErrorKind::SpawnFailed),
                        I::PoolUnreachable => assert_eq!(*kind, ErrorKind::PoolUnreachable),
                        other => panic!("{other:?} may not produce Error"),
                    }
                }
                // 4. StopRequested from any state lands in Stopping, always —
                //    and nothing reaches Idle except a confirmed exit, the
                //    failsafe, or a dismissal (each carrying its reason).
                if i == I::StopRequested {
                    assert_eq!(next, S::Stopping);
                }
                if let S::Idle { reason } = &next {
                    match &i {
                        I::Exited => assert_eq!(*reason, IdleReason::UserStopped),
                        I::FeeFailsafe => assert_eq!(*reason, IdleReason::Failsafe),
                        I::Dismiss => assert_eq!(*reason, IdleReason::GaveUp),
                        other => panic!("{other:?} may not produce Idle"),
                    }
                }
                // 5. Idle is only left by an explicit user start.
                if matches!(s, S::Idle { .. }) {
                    assert!(
                        matches!(i, I::UserStart | I::SpawnFailed),
                        "Idle --{i:?}--> {next:?}: mining must never self-start"
                    );
                }
                // 6. Stopping is left only by proof: a confirmed exit or a
                //    surfaced kill failure. Never by a timer, a press, a share.
                if s == S::Stopping {
                    assert!(
                        matches!(i, I::Exited | I::BackoffExhausted(_)),
                        "Stopping --{i:?}--> {next:?} leaks past the exit proof"
                    );
                }
                // 7. Warm never regresses and never leaves Starting.
                if let (S::Starting { phase }, I::Warm(q)) = (&s, &i) {
                    assert!(q > phase, "Warm emitted without moving forward");
                    assert!(matches!(next, S::Starting { .. }));
                }
            }
        }
    }

    #[test]
    fn rollup_prefers_life_over_death_and_stopping_sits_below_mining() {
        use super::rollup;
        let err = S::Error {
            kind: ErrorKind::MinerCrashed,
        };
        let paused = S::Paused {
            reason: PauseReason::Battery,
        };
        let states = |v: &[&S]| v.iter().map(|s| (*s).clone()).collect::<Vec<_>>();

        let v = states(&[&err, &S::Mining]);
        assert_eq!(rollup(v.iter()), S::Mining);
        let v = states(&[&err]);
        assert_eq!(rollup(v.iter()), err);
        assert_eq!(rollup(Vec::<S>::new().iter()), idle());
        let v = states(&[&paused, &err]);
        assert_eq!(rollup(v.iter()), paused);
        let v = states(&[&starting(WarmPhase::Spawning), &paused]);
        assert_eq!(rollup(v.iter()), starting(WarmPhase::Spawning));
        // A stop press wins the tray over a lane still warming, but a lane
        // still genuinely mining outranks it.
        let v = states(&[&S::Stopping, &starting(WarmPhase::Spawning)]);
        assert_eq!(rollup(v.iter()), S::Stopping);
        let v = states(&[&S::Stopping, &S::Mining]);
        assert_eq!(rollup(v.iter()), S::Mining);
    }
}
