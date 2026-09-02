# Sidecar provenance

Pasiv bundles third-party miner binaries as Tauri `externalBin` sidecars. This is
the record of where they come from and how that claim is enforced, so "is this
really upstream?" has an answer that is checked by the build rather than asserted
in a comment.

The pinned checksums live beside the fetch-and-verify tool in the app's release
tooling; the check runs at build time and in the release workflow before any
bundling.

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

## SRBMiner-Multi 3.5.3

Upstream: <https://github.com/doktor83/SRBMiner-Multi> — release `3.5.3`.

**These values are not maintained by hand.** They are the pin the build actually
uses: `tool/sidecars.json` in the private build repo, read by `build.rs` at
compile time and verified at download time (archive SHA-256, then extracted
binary SHA-256). A test fails if this document and that pin ever disagree, which
is how the previous drift was found — this file described **3.4.6** while every
shipped build had been fetching **3.5.3** for some time. Nothing was mis-shipped;
the pin was correct throughout. But a provenance document that names the wrong
version defeats its own purpose, since its whole job is to let someone outside
this project verify what we distribute.

### `x86_64-pc-windows-msvc`

| | |
|---|---|
| Archive MD5 | `96d97aee499eeaeb50b5c4e82aeccabe` |
| Archive SHA-256 | `432cc06a01d369afa02d5164b4b71e1a6b3b6243ddc9ef418fd80b28a566ab5f` |
| Member extracted | `SRBMiner-Multi-3-5-3/SRBMiner-MULTI.exe` |
| Binary SHA-256 | `64124054845977bdb35543c9bc13398a38f8468793f1068f9e17123d79bd56a4` |

### `x86_64-unknown-linux-gnu`

| | |
|---|---|
| Archive MD5 | `9ab5beec11bd58480d20415273649799` |
| Archive SHA-256 | `9e128f7ae47fc6a8f7e70ae97deece76974893c1c31dcbd7ca3b030ce337b8c7` |
| Member extracted | `SRBMiner-Multi-3-5-3/SRBMiner-MULTI` |
| Binary SHA-256 | `38f24cb3ea7f37088092aa43f34dbf5baf2f838f5b7700923c16a6812888df25` |

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
