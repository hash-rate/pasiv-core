# pasivd — the headless Pasiv node

Turn a server, NAS, or lab box into a rig in your Pasiv fleet with two commands
and no GUI. `pasivd` mines Monero (CPU) to your payout address and reports state
to the fleet, so a screenless machine shows up in the phone companion alongside
your desktops. It versions **independently** of the desktop app (currently
`0.1.2`).

A daemon can't do a wallet signature (no browser), so it pairs like a TV app.

## Install

```bash
curl -fsSL https://pasiv.network/pasivd.sh | sh     # sha256-verified static binary + systemd unit
sudo pasivd claim                                   # prints a 6-char code
#   → enter the code in the Pasiv companion app: +  → Add node
sudo systemctl enable --now pasivd                  # starts mining once a payout exists on your account
```

The installer drops a static musl binary at `/usr/local/bin/pasivd` and a
hardened systemd unit (`Nice=19`, yields to real work). Nothing mines until you
claim the node **and** an XMR payout is set on your account (desktop app →
Coins → Monero, which syncs automatically).

## Commands

| | |
|---|---|
| `pasivd claim` | mint a pairing code; approve it in the companion |
| `pasivd run` | the daemon: mine + publish state + obey start/stop (this is what the systemd unit runs) |
| `pasivd doctor` | one diagnostic pass (`PASS`/`WARN`/`FAIL`), exit 1 on any failure — cron/systemd friendly |

## Trust model (mirrors the desktop — see `../docs/MONETISATION.md` §5)

- **Non-custodial** — mines straight to your payout address; pasivd never holds funds.
- **Fee parity** — the same time-sliced 4% (20 s of every 500 s of mining), to the
  same compile-time fee address. A headless node is not a fee-free loophole.
- **Remote actions are start/stop only** — nothing from the phone can change the
  coin, pool, or payout.
- **No payout uplink** — the push never carries a payout address (enforced by the
  edge function; see `../supabase/functions/pasivd/logic.test.ts`).
- The miner binary (XMRig) is fetched from its official release and
  **sha256-verified against a compile-time pin** before it runs.

## Config & data

- `/etc/pasivd.json` — device id + secret (a bearer credential; kept `0600`).
  Override the path with `PASIVD_CONFIG`.
- `/var/lib/pasivd/` — the fetched XMRig, and `fee-ledger.jsonl` (one JSON line
  per fee slice, the same format the desktop writes).

## Build & test

```bash
cargo build --release --target x86_64-unknown-linux-musl   # the shipped static binary
cargo test                                                 # unit tests (pure logic)
cargo clippy --all-targets -- -D warnings
```

CI builds the musl binary and attaches it to every desktop release as
`pasivd-linux-x64` (+ `.sha256`); `pasivd.sh` resolves the latest one. Testing
notes and the coverage floor are in [`../docs/TESTING.md`](../docs/TESTING.md).
