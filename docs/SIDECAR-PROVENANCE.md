# Sidecar provenance

Pasiv bundles third-party miner binaries as Tauri `externalBin` sidecars. This is
the record of where they come from and how that claim is enforced, so "is this
really upstream?" has an answer that is checked by the build rather than asserted
in a comment.

Pinned values live in [`tool/sidecars.json`](../tool/sidecars.json); the check
runs in [`tool/fetch_sidecars.mjs`](../tool/fetch_sidecars.mjs) and in the
release workflow before any bundling.

## Why fetched, not committed

The binaries were previously committed to the app repository. Three problems:

1. **Defender deletes them from the working tree.** SRBMiner is detected as
   `Trojan:Win32/Kepavll!rfn`. On a Windows machine with real-time protection on,
   the binary is quarantined out of a fresh clone within minutes — the build
   input disappears with no obvious cause.
2. **A committed blob carries no provenance.** Nothing distinguished "the real
   upstream release" from "a binary someone swapped in", and a 29 MB opaque blob
   is exactly where you would hide something.
3. **Git history bloat** — ~54 MB across two triples, growing on every bump.

Fetching solves all three: the binary is absent from git, and every fetch checks
three independent hashes before the file is allowed near a bundle.

## Verification chain

Each fetch verifies, in order, and refuses to continue on any mismatch:

1. **Archive MD5** against the checksum the upstream author publishes next to the
   release. Proves we received what they released.
2. **Archive SHA-256** against a value pinned in this repo. MD5 is not
   collision-resistant and must never be the only gate.
3. **Extracted binary SHA-256** against a pinned value — covers the extraction
   step itself.

## SRBMiner-Multi 3.4.6

Upstream: <https://github.com/doktor83/SRBMiner-Multi> — release `3.4.6`,
published 2026-07-11.

Verified 2026-07-27 by downloading both release archives, checking them against
the author's published checksums, extracting, and comparing byte-for-byte with
the binaries that were committed at `v0.3.8`. **Both matched exactly** — the
committed binaries were genuine, unmodified upstream builds.

### `x86_64-pc-windows-msvc`

| | |
|---|---|
| Archive | `SRBMiner-Multi-3-4-6-win64.zip` (29,555,041 bytes) |
| Archive MD5 (published = computed) | `a2eafb6db1fd0077f4fcfc4461fdecce` |
| Archive SHA-256 | `b409b0a3b4e7945e5c4bfd022c344213f89e1aff609149da410d99171867b878` |
| Member extracted | `SRBMiner-Multi-3-4-6/SRBMiner-MULTI.exe` (29,716,480 bytes) |
| Binary SHA-256 | `94a8074259a54537f285f146b0f98111ab8771bc456dd28568d23bab83526447` |

### `x86_64-unknown-linux-gnu`

| | |
|---|---|
| Archive | `SRBMiner-Multi-3-4-6-Linux.tar.gz` (23,263,007 bytes) |
| Archive MD5 (published = computed) | `3adaa703f619881eb385776c29e1b7ec` |
| Archive SHA-256 | `1a94444b943827040cc6047f1aad2f8f97d9bc38c229ee70f8c124a350133532` |
| Member extracted | `SRBMiner-Multi-3-4-6/SRBMiner-MULTI` (24,119,920 bytes) |
| Binary SHA-256 | `64eeaa40a02600bd03cfb6986eb36ead54b08bd243390396120cca27fbf3f399` |

macOS is intentionally absent: upstream publishes no macOS SRBMiner build, which
is why Pearl is Windows/Linux-only and `coins::CoinSpec::is_available` gates the
picker to match.

### What is deliberately NOT extracted

The Windows archive also contains **`WinRing0x64.sys`**, a signed kernel driver
SRBMiner uses for MSR access. It is on Microsoft's vulnerable-driver blocklist
and has been abused as a privilege-escalation primitive. Pasiv does not need it
and must never ship it.

`fetch_sidecars.mjs` extracts exactly one member, by full path, from each
archive. Widening that has to be a deliberate edit — it cannot happen by
accident through a glob.

## Upstream is unsigned

SRBMiner's own binaries carry no Authenticode signature (verified: `NotSigned`).
So code-signing Pasiv would not inoculate the sidecar — it stays an unsigned
third-party binary either way, and AV heuristics will keep firing on it.

That is the reasoning behind the split in `docs/WINDOWS-FALSE-POSITIVE.md`:
signing addresses SmartScreen and installer reputation; only a false-positive
determination from Microsoft addresses the quarantine.

## Bumping a version

1. Update `version` and the three URLs/hashes per target in `tool/sidecars.json`.
2. `npm run fetch:sidecars` — it will refuse to install anything whose hashes do
   not match, so a bad paste fails loudly rather than shipping.
3. Re-verify against the author's published checksum file for the new release and
   update the tables above.
4. Update the vendored-version note in the app's SRBMiner adapter.
