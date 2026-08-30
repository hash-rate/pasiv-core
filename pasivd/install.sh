#!/usr/bin/env bash
# Pasiv headless node installer.
#
#   curl -fsSL https://pasiv.network/pasivd.sh | sh
#
# Installs the pasivd binary + a systemd unit, then tells you to claim it.
# No mining starts until you claim the node in the Pasiv companion app and a
# payout address exists on your account — this script never touches a wallet.
set -eu

REPO_RELEASE="https://github.com/hash-rate/pasiv-releases/releases/latest/download"
BIN_URL="$REPO_RELEASE/pasivd-linux-x64"
SUM_URL="$REPO_RELEASE/pasivd-linux-x64.sha256"
SIG_URL="$REPO_RELEASE/pasivd-linux-x64.minisig"
# The minisign public key — the SAME key every desktop update is verified
# against. Pinned here, served from pasiv.network (an origin independent of
# the release host), so replacing the binary AND its checksum on the release
# host still cannot forge a signature this script accepts.
MINISIGN_PUB="RWQJhawYO7igroqjh+CUPCstCmt4Ka2DAmznjX2e1gsScv3k5u7jYWR3"
BIN_PATH="/usr/local/bin/pasivd"
UNIT_PATH="/etc/systemd/system/pasivd.service"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) ;;
  *) echo "pasivd currently ships for Linux x86_64 only (got $(uname -s)-$(uname -m))"; exit 1 ;;
esac

SUDO=""
[ "$(id -u)" -eq 0 ] || SUDO="sudo"

echo "→ downloading pasivd"
TMP="$(mktemp)"
curl -fsSL "$BIN_URL" -o "$TMP"

# You are piping a script from the internet that installs a MINER. The least we
# can do is prove the binary is the one we published: verify it against the
# checksum served beside it, and refuse to install on any mismatch.
echo "→ verifying checksum"
WANT="$(curl -fsSL "$SUM_URL" | awk '{print $1}')"
if command -v sha256sum >/dev/null 2>&1; then
  GOT="$(sha256sum "$TMP" | awk '{print $1}')"
else
  GOT="$(shasum -a 256 "$TMP" | awk '{print $1}')"
fi
if [ -z "$WANT" ] || [ "$WANT" != "$GOT" ]; then
  rm -f "$TMP"
  echo "checksum mismatch — refusing to install." >&2
  echo "  expected: ${WANT:-<none fetched>}" >&2
  echo "  got:      $GOT" >&2
  exit 1
fi
echo "  ✓ $GOT"

# The checksum above shares an origin with the binary, so alone it only proves
# transit integrity. The SIGNATURE does not: it is made offline with the same
# key the desktop updater trusts, and the public key is pinned in this script —
# served from pasiv.network, an origin independent of the release host.
# Verification is MANDATORY: a fallback would hand anyone who could replace the
# release assets a root install, since they could replace the checksum too.
# If minisign is missing we install it: first from the distro's own signed
# repos, and failing that from its author's release, checked against a hash
# pinned HERE. Both matter — minisign is simply absent from some current
# distributions (Ubuntu 22.04 LTS, supported to 2027, has no such package), and
# without the fallback this script would refuse to install on them at all.
# Pinning the hash in a script served from pasiv.network keeps the trust
# argument intact: the thing that verifies the download is itself verified
# against an origin independent of the release host.
MINISIGN_TGZ="https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-linux.tar.gz"
MINISIGN_TGZ_SHA256="9a599b48ba6eb7b1e80f12f36b94ceca7c00b7a5173c95c3efc88d9822957e73"
MS=""   # set when we fall back to our own copy rather than a packaged one

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  else shasum -a 256 "$1" | awk '{print $1}'; fi
}

if ! command -v minisign >/dev/null 2>&1; then
  echo "→ installing minisign (required to verify the download)"
  if command -v apt-get >/dev/null 2>&1; then
    $SUDO apt-get update -qq >/dev/null 2>&1 || true
    $SUDO apt-get install -y -qq minisign >/dev/null 2>&1 || true
  elif command -v dnf >/dev/null 2>&1; then
    $SUDO dnf install -y -q minisign >/dev/null 2>&1 || true
  elif command -v apk >/dev/null 2>&1; then
    $SUDO apk add --no-progress minisign >/dev/null 2>&1 || true
  fi
fi

if ! command -v minisign >/dev/null 2>&1; then
  echo "  not packaged for this distribution — fetching the pinned build"
  MS_DIR="$(mktemp -d)"
  if curl -fsSL "$MINISIGN_TGZ" -o "$MS_DIR/ms.tgz" \
     && [ "$(sha256_of "$MS_DIR/ms.tgz")" = "$MINISIGN_TGZ_SHA256" ] \
     && tar xzf "$MS_DIR/ms.tgz" -C "$MS_DIR" 2>/dev/null \
     && [ -x "$MS_DIR/minisign-linux/x86_64/minisign" ]; then
    MS="$MS_DIR/minisign-linux/x86_64/minisign"
  else
    rm -rf "$MS_DIR"
  fi
fi

# One handle for whichever we ended up with.
[ -n "$MS" ] || MS="$(command -v minisign 2>/dev/null || true)"
if [ -z "$MS" ]; then
  rm -f "$TMP"
  echo "minisign is required to verify this download and could not be installed —" >&2
  echo "refusing to install. Install it yourself (apt/dnf/apk: minisign) and re-run." >&2
  exit 1
fi
echo "→ verifying signature"
SIG_TMP="$(mktemp)"
if ! curl -fsSL "$SIG_URL" -o "$SIG_TMP"; then
  rm -f "$TMP" "$SIG_TMP"
  echo "signature not published for this release — refusing to install." >&2
  exit 1
fi
if ! "$MS" -Vm "$TMP" -x "$SIG_TMP" -P "$MINISIGN_PUB" >/dev/null; then
  rm -f "$TMP" "$SIG_TMP"
  echo "SIGNATURE VERIFICATION FAILED — refusing to install." >&2
  exit 1
fi
rm -f "$SIG_TMP"
echo "  ✓ minisign signature valid"

chmod 755 "$TMP"
$SUDO mv "$TMP" "$BIN_PATH"
# mv preserves the invoking user's ownership — without this, a system-wide
# root-run daemon binary stays writable by whoever installed it, and the
# unit's DynamicUser cannot execute a 0700 file owned by someone else.
$SUDO chown 0:0 "$BIN_PATH"
$SUDO chmod 755 "$BIN_PATH"

# ---------------------------------------------------------------------------
# Performance boost. The unit below sandboxes pasivd to a dynamic non-root
# user (correct for a consumer machine), which also means the miner can never
# reserve RandomX huge pages or apply the MSR preset — the two levers worth
# 5–15% hashrate depending on silicon. So the unit runs ONE privileged
# ExecStartPre script that does both, best-effort, before the sandbox drops.
# Consumers should not have to know any of this exists.

# wrmsr (msr-tools) is what applies the MSR preset. Best-effort: without it
# the boost script skips that half and says so in the journal.
if ! command -v wrmsr >/dev/null 2>&1; then
  if command -v apt-get >/dev/null 2>&1; then
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq msr-tools >/dev/null 2>&1 || true
  elif command -v dnf >/dev/null 2>&1; then
    $SUDO dnf install -y -q msr-tools >/dev/null 2>&1 || true
  elif command -v apk >/dev/null 2>&1; then
    $SUDO apk add -q msr-tools >/dev/null 2>&1 || true
  fi
fi

echo "→ installing performance boost (huge pages + MSR preset)"
$SUDO mkdir -p /usr/local/libexec
$SUDO tee /usr/local/libexec/pasivd-boost.sh >/dev/null <<'BOOST'
#!/bin/sh
# RandomX performance boost — run as root by pasivd.service (ExecStartPre=+)
# just before the sandboxed daemon starts. EVERYTHING here is best-effort: a
# locked-down kernel, an exotic CPU, or a container may refuse any line, and
# mining must still start — so nothing here ever exits non-zero.
#
#   1. Huge pages: RandomX wants ~1168 2MB pages for its dataset plus one per
#      mining thread. An unprivileged miner cannot raise vm.nr_hugepages.
#      Missing pages cost a few percent hashrate.
#   2. MSR preset: the exact registers and values xmrig applies when it has
#      privileges (upstream scripts/randomx_boost.sh, GPLv3 like this repo).
#      Worth ~5% on Intel and up to ~15% on AMD Zen. The sandboxed miner can
#      never touch /dev/cpu/*/msr.
#
# Kernel lockdown (Secure Boot) forbids raw MSR writes outright; that is
# detected and reported as one clear journal line instead of a spray of
# per-CPU write failures.

log() { echo "pasivd-boost: $*"; }

# --- huge pages -------------------------------------------------------------
want=$(( 1168 + $(nproc 2>/dev/null || echo 4) + 8 ))
have=$(awk '/^HugePages_Total/{print $2}' /proc/meminfo 2>/dev/null)
have=${have:-0}
if [ "$have" -lt "$want" ]; then
  if sysctl -w vm.nr_hugepages="$want" >/dev/null 2>&1; then
    got=$(awk '/^HugePages_Total/{print $2}' /proc/meminfo 2>/dev/null)
    log "huge pages: $have -> ${got:-?} (wanted $want)"
  else
    log "huge pages: could not raise vm.nr_hugepages (have $have, wanted $want)"
  fi
else
  log "huge pages: $have already reserved"
fi

# --- MSR preset ---------------------------------------------------------
# Registers, values, and CPU detection mirror xmrig's scripts/randomx_boost.sh
# verbatim — hardware pokes are not something to improvise.
case "$(uname -m)" in
  x86_64) ;;
  *) log "msr: not x86_64 — skipping"; exit 0 ;;
esac
if grep -qE '\[(integrity|confidentiality)\]' /sys/kernel/security/lockdown 2>/dev/null; then
  log "msr: kernel lockdown (Secure Boot) blocks MSR writes — skipping. Disabling Secure Boot unlocks ~5-15% hashrate."
  exit 0
fi
if [ -e /sys/module/msr/parameters/allow_writes ]; then
  echo on > /sys/module/msr/parameters/allow_writes 2>/dev/null || true
else
  modprobe msr allow_writes=on 2>/dev/null || true
fi
if ! command -v wrmsr >/dev/null 2>&1; then
  log "msr: wrmsr not installed (package: msr-tools) — skipping"
  exit 0
fi
if grep -qE 'AMD Ryzen|AMD EPYC|AuthenticAMD' /proc/cpuinfo; then
  if grep -qE 'cpu family[[:space:]]+:[[:space:]]*25' /proc/cpuinfo; then
    if grep -qE 'model[[:space:]]+:[[:space:]]*(97|117)' /proc/cpuinfo; then
      if wrmsr -a 0xc0011020 0x4400000000000 && wrmsr -a 0xc0011021 0x4000000000040 \
         && wrmsr -a 0xc0011022 0x8680000401570000 && wrmsr -a 0xc001102b 0x2040cc10; then
        log "msr: Zen4 preset applied"
      else log "msr: Zen4 preset failed"; fi
    else
      if wrmsr -a 0xc0011020 0x4480000000000 && wrmsr -a 0xc0011021 0x1c000200000040 \
         && wrmsr -a 0xc0011022 0xc000000401570000 && wrmsr -a 0xc001102b 0x2000cc10; then
        log "msr: Zen3 preset applied"
      else log "msr: Zen3 preset failed"; fi
    fi
  elif grep -qE 'cpu family[[:space:]]+:[[:space:]]*26' /proc/cpuinfo; then
    if wrmsr -a 0xc0011020 0x4400000000000 && wrmsr -a 0xc0011021 0x4000000000040 \
       && wrmsr -a 0xc0011022 0x8680000401570000 && wrmsr -a 0xc001102b 0x2040cc10; then
      log "msr: Zen5 preset applied"
    else log "msr: Zen5 preset failed"; fi
  else
    if wrmsr -a 0xc0011020 0 && wrmsr -a 0xc0011021 0x40 \
       && wrmsr -a 0xc0011022 0x1510000 && wrmsr -a 0xc001102b 0x2000cc16; then
      log "msr: Zen1/Zen2 preset applied"
    else log "msr: Zen1/Zen2 preset failed"; fi
  fi
elif grep -q Intel /proc/cpuinfo; then
  if wrmsr -a 0x1a4 0xf; then
    log "msr: Intel preset applied (prefetchers off)"
  else log "msr: Intel preset failed"; fi
else
  log "msr: unrecognised CPU vendor — skipping"
fi
exit 0
BOOST
$SUDO chown 0:0 /usr/local/libexec/pasivd-boost.sh
$SUDO chmod 755 /usr/local/libexec/pasivd-boost.sh

echo "→ installing systemd unit"
$SUDO tee "$UNIT_PATH" >/dev/null <<'UNIT'
[Unit]
Description=Pasiv headless mining node
After=network-online.target
Wants=network-online.target

[Service]
# One privileged pass ('+') before the sandbox drops, never fatal ('-'):
# reserves RandomX huge pages and applies xmrig's MSR preset. The sandboxed
# miner cannot do either itself, and without them a node quietly leaves
# 5-15% hashrate on the table. Details: /usr/local/libexec/pasivd-boost.sh.
ExecStartPre=-+/usr/local/libexec/pasivd-boost.sh
ExecStart=/usr/local/bin/pasivd run
Restart=always
RestartSec=10
# Mine with what's spare: the node yields to everything else on the box.
Nice=19
CPUWeight=20
IOWeight=20
DynamicUser=yes
StateDirectory=pasivd
ConfigurationDirectory=pasivd
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes

[Install]
WantedBy=multi-user.target
UNIT

$SUDO systemctl daemon-reload

cat <<EOF

  pasivd installed.

  1. Claim this node:      sudo pasivd claim
     (enter the printed code in the Pasiv companion app → + )
  2. Then start it:        sudo systemctl enable --now pasivd

  Status:  systemctl status pasivd
  Logs:    journalctl -u pasivd -f

EOF
