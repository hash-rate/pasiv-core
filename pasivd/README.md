# pasivd — the headless Pasiv node

Turn a server, NAS, or lab box into a rig in your Pasiv fleet with two commands
and no GUI. `pasivd` mines Monero (CPU) to your payout address and reports state
to the fleet, so a screenless machine shows up in the phone companion alongside
your desktops. It versions **independently** of the desktop app (currently
`0.1.2`).

A daemon can't do a wallet signature (no browser), so it pairs like a TV app.

## Install

```bash
curl -fsSL https://pasiv.network/pasivd.sh | sh     # minisign-signed static binary + systemd unit
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
| `pasivd help` | help; also `pasivd` on its own, `-h`, `--help`, and `pasivd <command> --help` |
| `pasivd version` | print the version (`-V` / `--version` too) |

`doctor` also reports **perf**: whether the RandomX huge pages and the CPU MSR
preset are actually in effect. The installer applies both automatically (a
privileged `ExecStartPre` that runs before the sandbox drops), worth ~5-15%
hashrate; `doctor` names it when a locked-down kernel (Secure Boot) or a missing
`msr-tools` silently skipped it, so an under-earning node explains itself.

Output is coloured on a terminal and plain everywhere else (a pipe, a log,
`NO_COLOR`, `TERM=dumb`). A typo suggests the nearest command; a wrong command
exits `2` (usage) and a failure exits `1`, so a wrapper can tell them apart.

## Trust model (mirrors the desktop — see [`../docs/FEES.md`](../docs/FEES.md), the binding never-list)

- **Non-custodial** — mines straight to your payout address; pasivd never holds funds.
- **Fee parity** — the same time-sliced 4% (20 s of every 500 s of mining), to the
  same compile-time fee address. A headless node is not a fee-free loophole.
- **Remote actions are start/stop only** — nothing from the phone can change the
  coin, pool, or payout.
- **No payout uplink** — the push never carries a payout address (enforced by the
  edge function, whose pure decision logic is tested in the app repository).
- The miner binary (XMRig) is fetched from its official release and
  **sha256-verified against a compile-time pin** before it runs.
- **Your hardware is not the product** — no overclocking, undervolting, or
  raised thermal/power limits, ever (never-list item 9). The node mines with
  what is already spare: `Nice=19`, `CPUWeight=20`, and it yields to real work.

## Performance

The unit sandboxes pasivd to a dynamic non-root user, which is right for a
machine you also use — and it means the miner cannot reserve RandomX huge pages
or apply the CPU MSR preset itself. Those are worth roughly **5-15%** together,
so the installer applies them for you: `/usr/local/libexec/pasivd-boost.sh` runs
privileged (`ExecStartPre=-+`) just before the sandbox drops, and is best-effort
throughout — a locked-down kernel, a container, or a missing tool skips
gracefully and mining still starts.

MSR values come verbatim from [XMRig's
`randomx_boost.sh`](https://github.com/xmrig/xmrig/blob/master/scripts/randomx_boost.sh)
(GPLv3, like this repo), covering AMD Zen1-5 and Intel. Hardware pokes are not
something to improvise.

`pasivd doctor` reports whether each landed. **Secure Boot blocks the MSR half
outright** — the kernel refuses raw MSR writes under lockdown — and `doctor`
says so explicitly rather than implying a fix exists.

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
`pasivd-linux-x64` (+ `.sha256` + `.minisig` — signed with the same minisign key as every desktop update; the installer pins the public key and verifies when `minisign` is installed); `pasivd.sh` resolves the latest one. Testing
notes and the coverage floor are in [`../docs/TESTING.md`](../docs/TESTING.md).
