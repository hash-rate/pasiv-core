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

chmod +x "$TMP"
$SUDO mv "$TMP" "$BIN_PATH"

echo "→ installing systemd unit"
$SUDO tee "$UNIT_PATH" >/dev/null <<'UNIT'
[Unit]
Description=Pasiv headless mining node
After=network-online.target
Wants=network-online.target

[Service]
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
