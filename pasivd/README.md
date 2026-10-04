# pasivd — the headless Pasiv node

Turn a server, NAS, or lab box into a rig in your Pasiv fleet with two commands
and no GUI. `pasivd` mines Monero on the CPU, paid to your own address — in
USDT via unMineable when your account has a USDT payout (the default for new
Pasiv installs), otherwise in XMR direct — and reports state to the fleet, so a
screenless machine shows up in the phone companion alongside your desktops. It
versions **independently** of the desktop app (currently `0.1.9`).

A daemon can't do a wallet signature (no browser), so it pairs like a TV app.

## Install

```bash
curl -fsSL https://pasiv.network/pasivd.sh | sh     # minisign-verified static binary + systemd unit
sudo pasivd claim                                   # prints a 6-char code
#   → enter the code in the Pasiv companion app: +  → Add node
sudo systemctl enable --now pasivd                  # starts mining once a payout exists on your account
```

The installer drops a static musl binary at `/usr/local/bin/pasivd` and a
hardened systemd unit (`Nice=19`, yields to real work). Nothing mines until you
claim the node **and** a payout is set on your account (desktop app → Wallets:
a USDT address, or a Monero address on the direct route; it syncs
automatically).

## Commands

| | |
|---|---|
| `pasivd claim` | mint a pairing code; approve it in the companion |
| `pasivd run` | the daemon: mine + publish state + obey start/stop/update (this is what the systemd unit runs) |
| `pasivd update` | fetch the latest release, verify its signature, stage it for the next start (`sudo pasivd update && sudo systemctl restart pasivd`) |
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

- **Non-custodial** — the pool pays your own address (unMineable converts to
  USDT and pays daily once past 1.5 USDT); pasivd never holds funds. unMineable
  only pays automatically once an address's "auto pay" is on, and it starts
  off, so from 0.1.7 the node switches it on for your USDT address (checked
  daily; it's the same setting as the switch on your unmineable.com address page).
- **Fee parity** — the same time-sliced 4% (20 s of every 500 s of mining), to the
  same compile-time fee address as the desktop on that route (the BTC treasury
  on unMineable, the Monero fee address direct). A headless node is not a
  fee-free loophole.
- **Remote actions are start, stop and update** — nothing from the phone can
  change the coin, pool, or payout, and an update installs only a release Pasiv
  signed. A stop survives restarts: a node you paused stays paused.
- **Signed self-update** — from 0.1.6 the node checks for a new release once a
  day and when you tap Update in the companion. The download must carry a valid
  signature from the same pinned minisign key as every desktop update, or it is
  refused. The installed `/usr/local/bin/pasivd` is never overwritten (the
  sandbox can't write it): the update is staged in `/var/lib/pasivd/update/`,
  and at each start the installed binary re-verifies it and runs it only if it
  is signed and newer. A staged build that never checks in is abandoned after
  three starts, and the installed one carries on. Nodes on 0.1.5 or earlier
  need one manual update: `curl -fsSL https://pasiv.network/pasivd.sh | sh &&
  sudo systemctl restart pasivd`.
- **No payout uplink** — the push never carries a payout address (enforced by the
  edge function, whose pure decision logic is tested in the app repository).
- The miner binary (XMRig) is fetched from its official release and
  **sha256-verified against a compile-time pin** before it runs.
- **Your hardware is not the product** — no overclocking, undervolting, or
  raised thermal/power limits, ever (never-list item 9). The node mines with
  what is already spare: `Nice=19`, `CPUWeight=20`, and it yields to real work.

## Resilience

**It keeps mining whatever the cloud does.** The cloud is consulted, never
obeyed into silence. At start the node polls your account once for the payout;
if the edge function, the network, or TLS is down — or the device has been
revoked or un-claimed — it logs that loudly and starts mining anyway on the
payout it last heard (cached in `/var/lib/pasivd/payout.json`, your own
address), then re-polls every 60 s until the cloud answers and hourly after
that. A revoked node warns once an hour and keeps hashing; only a node with no
payout anywhere waits, because there is nothing to mine to. Before 0.1.9 a
failed startup poll made `pasivd run` exit 1 and systemd restart it forever
without a hash.

**The miner loop never waits on the network.** Cloud pushes, polls, update
checks and the unMineable auto-pay check run on their own task with a 15 s
request / 5 s connect timeout (downloads get a longer ceiling); a hung request
delays the next push, never a respawn or the end of a fee slice. Local xmrig
stats that fail twice in a row are reported as unknown (hashrate 0) rather than
repeating the last good number, and the xmrig binary is replaced only after the
new one is downloaded and sha256-verified, so an unreachable release host never
leaves a node with no miner; a missing binary is re-fetched every 10 minutes.

**Config writes are atomic.** The device config and the payout cache are
written to a temp file in the same directory (0600 from creation), fsync'd, and
renamed over the old file, and only when the contents changed — a power cut
mid-write can never leave an empty identity file. An update's rollback health
is judged by work (five minutes of hashing or an accepted share), not by a
cloud push succeeding.

## Performance

The unit sandboxes pasivd to an unprivileged `pasivd` system user, which is
right for a machine you also use — and it means the miner cannot reserve RandomX huge pages
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

- `/etc/pasivd.json` — device id + secret (a bearer credential; kept `0600`,
  written by `pasivd claim` as root and handed to the `pasivd` service user so
  the sandboxed unit can read it). Override the path with `PASIVD_CONFIG`.
- `/var/lib/pasivd/` — the fetched XMRig, `payout.json` (the payout last heard
  from your account — what the node mines to when the cloud is unreachable),
  `fee-ledger.jsonl` (one JSON line per fee slice, the same format the desktop
  writes), `stopped` (present while the owner has the node stopped), and
  `update/` (a staged signed release).

## Crash reports

If pasivd **panics**, it sends one crash report to Sentry (`src/sentry.rs`)
before it exits: the panic message and `file:line`, a backtrace, the daemon
version, and two tags — `os` and `payout_route` (`usdt`/`direct`). Nothing
else leaves: no payout address, no hostname, no account name, no IP, no
session, no breadcrumbs. Every text field is scrubbed first (`src/scrub.rs`:
coin addresses of every shape, home paths, e-mails, IPs, MACs, hostnames, and
this machine's own host and user names → `<address>`, `<user>`, `<host>`, …), and
the cases are the public table `tests/contracts/scrub.json`.

Turn it off either way — the never-list in [`docs/FEES.md`](../docs/FEES.md)
promises telemetry you can switch off:

- `"telemetry": false` in `/etc/pasivd.json` (the key `pasivd claim` writes;
  a re-claim keeps your choice), or
- `PASIVD_TELEMETRY=0` in the unit's environment (`systemctl edit pasivd` →
  `[Service]` `Environment=PASIVD_TELEMETRY=0`).

Normal operation sends nothing to Sentry; `help` and `version` never start it.

## Build & test

```bash
cargo build --release --target x86_64-unknown-linux-musl   # the shipped static binary
cargo test                                                 # unit tests (pure logic)
cargo clippy --all-targets -- -D warnings
```

CI builds the musl binary and attaches it to every desktop release as
`pasivd-linux-x64` (+ `.sha256` + `.minisig` — signed with the same minisign key as every desktop update; the installer pins the public key, installs `minisign` if it is missing, and refuses to install without a valid signature); `pasivd.sh` resolves the latest one. Testing
notes and the coverage floor are in [`../docs/TESTING.md`](../docs/TESTING.md).
