# Sidecar provenance

Pasiv runs two miner binaries as separate processes beside the app: **its own
build of the Pearl GPU engine** and the **official XMRig** for the CPU. This is
the record of where each comes from and how that claim is enforced, so "is this
really what you say it is?" has an answer that is checked by the build rather
than asserted in a comment.

The pinned checksums live beside the fetch-and-verify tool in the app's release
tooling; the check runs at build time and in the release workflow before any
bundling, and the Windows app re-verifies the engine against the same pin every
time it starts it.

## Why fetched, not committed

The binaries were previously committed to the app repository. Three problems:

1. **Defender deleted them from the working tree.** The old GPU miner
   (SRBMiner-Multi, closed-source and unsigned) was detected as
   `Trojan:Win32/Kepavll!rfn`; on a Windows machine with real-time protection on
   it was quarantined out of a fresh clone within minutes — the build input
   disappeared with no obvious cause.
2. **A committed blob carries no provenance.** Nothing distinguished "the real
   upstream release" from "a binary someone swapped in", and a 29 MB opaque blob
   is exactly where you would hide something.
3. **Git history bloat** — tens of MB per triple, growing on every bump.

Fetching solves all three: the binary is absent from git, and every fetch checks
independent hashes before the file is allowed near a bundle.

## Verification chain

Each fetch verifies, in order, and refuses to continue on any mismatch:

1. **Archive SHA-256** against a value pinned in the app's release tooling.
2. **Extracted binary SHA-256** against a pinned value — covers the extraction
   step itself. Exactly one member is extracted from each archive, by its full
   path; widening that has to be a deliberate edit, so an archive can never
   smuggle a second file in.

Where the upstream author publishes a checksum of their own (XMRig's GPG-signed
`SHA256SUMS`), the pinned archive hash was verified against it before being
committed.

## Pearl engine — Pasiv's own build of CPPminer (since 0.6.11)

Upstream: <https://github.com/danifest751/CPPminer> (MIT), branch `release/fork`,
pinned by commit. Pasiv does not fetch upstream's binaries: a dedicated CI
workflow clones the pinned commit, applies two small build patches, fetches
CUTLASS and BLAKE3 at pinned versions and builds with every runtime statically
linked (CUDA 12.4.1; SASS sm_75/86/89 + PTX compute_89), so the one file runs on
a machine that has only the NVIDIA driver. The Windows executable is then
Authenticode-signed (DRAGON HEDGE PTY LTD, Certum, RFC 3161 timestamp — CI never
holds the key), both platforms are packaged, and the result is published as a
prerelease `engine-v<version>` on
[hash-rate/pasiv-releases](https://github.com/hash-rate/pasiv-releases/releases).
A `BUILD-INFO.txt` inside each archive names the pin, toolchain and CI run;
`THIRD-PARTY-NOTICES.txt` (from 0.1.1) carries the CUTLASS, BLAKE3, CPPminer
and CUDA runtime notices.

Why: SRBMiner (below, retired) was closed and unsigned, and Defender quarantined
it — and an unsigned build of this very engine — on sight, which is where every
Windows prompt Pasiv used to show came from. The signed build of the same bytes
runs clean from a scanned folder with zero detections. The engine runs with its
developer fee switched off, so nothing on the GPU lane goes to a third-party
developer.

macOS is intentionally absent: the engine is CUDA-only, which is why Pearl is
Windows/Linux-only and `coins::CoinSpec::is_available` gates the picker to match.

### engine 0.1.1 — 2026-10-24 (current pin)

Upstream `danifest751/CPPminer` @ `b6eb5e2df345418be70ecba7077bbd65003df973`
+ the two build patches; CUDA 12.4.1; archs 75-real;86-real;89; CUTLASS 2.11.0;
BLAKE3 1.8.5. Same bytes-producing toolchain as 0.1.0; adds `THIRD-PARTY-NOTICES.txt`.

| Target | Archive | Archive SHA-256 | Member | Binary SHA-256 |
|---|---|---|---|---|
| `x86_64-pc-windows-msvc` | `cppminer-0.1.1-windows-x64.zip` | `aeb47e84ad5914cf4045b9be3c40a61236f463b109b9ee7a0af47cbf967a9103` | `cppminer-0.1.1/cppminer.exe` (signed) | `12588978b1cd0d23ac6a615f43ed12b7109494aa77ae4d16bf1c77bd8545583a` |
| `x86_64-unknown-linux-gnu` | `cppminer-0.1.1-linux-x64.tar.gz` | `cbcaf4ba0cff9ca11c7bb95f9f31696b976adc6608c619defd49c416576cd86f` | `cppminer-0.1.1/cppminer` | `aa4236b899e8d2f563ad2f9216500b637794ed79503a7e3d049e448c8844e1d0` |

### engine 0.1.0 — 2026-10-23

Same upstream pin, patches and toolchain as 0.1.1.

| Target | Archive | Archive SHA-256 | Member | Binary SHA-256 |
|---|---|---|---|---|
| `x86_64-pc-windows-msvc` | `cppminer-0.1.0-windows-x64.zip` | `86774ed4ddff0c698c6deba877a332c53c1378f49a03c1995f48d36e530e4466` | `cppminer-0.1.0/cppminer.exe` (signed) | `1c1261e6551e1c37feeed1732a282d0b1d0fcc78c90b53ba7aa2d87ff47a42ca` |
| `x86_64-unknown-linux-gnu` | `cppminer-0.1.0-linux-x64.tar.gz` | `3e6e93911b71eb56f85f2c55f3fcb8a51d6894100e931b9e87b7219130f27b26` | `cppminer-0.1.0/cppminer` | `35abc0f6c6303878e034cb10a3a5b9149f9a84febc8bc970df3d106008acde1a` |

Verified before publishing: both binaries ran the engine's `--mock` self-test
to `verify OK` on real GPUs (RTX 4060, RTX 4070 SUPER) with no runtime library
beside them, and the Windows run from a scanned folder recorded 0 Defender
detections.

## pasiv-rx 0.1.2 (the CPU engine from Pasiv 0.7.2 / pasivd 0.1.13)

Pasiv's own RandomX CPU miner: <https://github.com/hash-rate/pasiv-rx>, GPL-3.0,
on tevador/RandomX 1.2.1 (BSD-3) with three Pasiv patches (`patches/`). It
replaces XMRig on every platform and has no developer donation. Built by the
repo's `build.yml` (run 40314759725, commit
`957b9e02dc9c9404808b2f2214cbd59b10c009b8`) and published as prerelease
`cpu-engine-v0.1.2` on hash-rate/pasiv-releases by `tool/cpu-engine-release.sh`
(pasiv repo); the exact corresponding source is `pasiv-rx-0.1.2-source.tar.gz`
in that release.

| Target | Archive | Archive sha256 | Member | Binary sha256 |
|---|---|---|---|---|
| `x86_64-pc-windows-msvc` | `pasiv-rx-0.1.2-windows-x64.zip` | `6395e5b19c28a6b8a22c8968ea714532ed58060f1950d7b8606616d0c9e1f645` | `pasiv-rx-0.1.2/pasiv-rx.exe` (Authenticode, DRAGON HEDGE PTY LTD, Certum RFC3161) | `d77de4020ef442ac5e389008648d8a82a3dedbc7687e29d912c038d966400356` |
| `x86_64-unknown-linux-gnu` | `pasiv-rx-0.1.2-linux-x64.tar.gz` | `a5c2bc53bcbafea9ac74038e34ea8f26e8542da05703239b54fa97ade21f074c` | `pasiv-rx-0.1.2/pasiv-rx` | `941aacd5b769ec676a819c57b7055821db967feb10608d76d21e6be615fe5540` |
| `x86_64-unknown-linux-musl` (pasivd) | `pasiv-rx-0.1.2-linux-x64-static.tar.gz` | `e4438c3a7f54e14e5f90f8f9c8eaa86752c81a60a1f6311f0a65a7d7b7cf8a74` | `pasiv-rx-0.1.2/pasiv-rx` | `331404b3a05d39cc60c1e4e8e731946e91f005bda05ec6d060f80ddd729e0c8b` |
| `aarch64-apple-darwin` | `pasiv-rx-0.1.2-macos-arm64.tar.gz` | `92d8c6988181340cedc5d8643d3ccdc88635958f031ccd8faf07727aa2a1bfd4` | `pasiv-rx-0.1.2/pasiv-rx` (Developer-ID-signed inside the app bundle) | `d649ec2a5f082b7fa0e7f0428cdfc7dfaa017f50f18d4f2ff68d9f99c8387f64` |

Verified before publishing: the signed and unsigned Windows builds mined live
from a scanned, unexcluded folder (30 min and 60 min, 0 rejected, 0 Defender
detections); pasivd 0.1.13 migrated a live XMRig node on the rack (engine
fetched and verified, XMRig retired after, 25/25 shares, fee slice swapped).

## XMRig 6.26.0 (retired in Pasiv 0.7.2 / pasivd 0.1.13)

Upstream: <https://github.com/xmrig/xmrig>. Fetched on demand since 2026-08-26;
before the switch the previously committed binaries were confirmed
**byte-identical** (sha256) to the official upstream release builds, so nothing
about the shipped product changed. Upstream publishes a **GPG-signed
`SHA256SUMS`** with every release; every archive hash below was verified
against that signed file.

| Target | Archive | Archive SHA-256 | Member | Binary SHA-256 |
|---|---|---|---|---|
| `aarch64-apple-darwin` | `xmrig-6.26.0-macos-arm64.tar.gz` | `6ae4eb4216e99a201ae9a3d2c3a7c275207c5165cfc25da1f3d735d6c4829c18` | `xmrig-6.26.0/xmrig` | `c66f9881bed79a550e18d54b9ae5cf03b91a0e881efdbf7962db2e58de0b4f7b` |
| `x86_64-unknown-linux-gnu` | `xmrig-6.26.0-linux-static-x64.tar.gz` | `fc6f8ae5f64e4f17481f7e3be29a1c56949f216a998414188003eae1db20c9e5` | `xmrig-6.26.0/xmrig` | `b20f39fc00d242e706b6c30367ad811c676e0575050a4ec2f30104b696944b49` |
| `x86_64-pc-windows-msvc` | `xmrig-6.26.0-windows-x64.zip` | `bba8097cb37d9b458a1cb1137876b27cde6740d17fe4ccbc086ba07d87d9e147` | `xmrig-6.26.0/xmrig.exe` | `6fa80698d7268f6e88aa88c06fb27ee99e1bcee747c2e76911e6206a5b1aeeb3` |

`pasivd` (this repository) fetches and verifies the Linux build the same way;
see [`pasivd/README.md`](../pasivd/README.md).

## SRBMiner-Multi — RETIRED in 0.6.11 (2026-10)

Kept for the record. Upstream <https://github.com/doktor83/SRBMiner-Multi>; the
last pin was 3.5.3 (Windows archive SHA-256
`432cc06a01d369afa02d5164b4b71e1a6b3b6243ddc9ef418fd80b28a566ab5f`, Linux
`9e128f7ae47fc6a8f7e70ae97deece76974893c1c31dcbd7ca3b030ce337b8c7`). It was the
GPU miner from 0.3.8 to 0.6.10. Its 2% developer fee and the Defender
exclusion / UAC / Smart App Control friction left with it; Ravencoin and Ergo,
which only it mined, retired a week ahead of their 1 November 2026 date. Its
Windows archive also carried `WinRing0x64.sys`, a kernel driver on Microsoft's
vulnerable-driver blocklist, which Pasiv never extracted or shipped.

## Upstream signatures

XMRig's binaries carry no Authenticode signature, and SRBMiner's never did —
which is why code-signing Pasiv alone could never inoculate a bundled
third-party miner. The Pearl engine closes that gap by being Pasiv's own build,
signed with Pasiv's certificate.

## Bumping a version

XMRig: update the version and the per-target URLs and hashes in the pin file,
run the fetch tool (it refuses to install anything whose hashes do not match,
so a bad paste fails loudly rather than shipping), re-verify against the
author's signed checksum file, and update the table above.

Pearl engine (a hardfork, a kernel fix, a toolchain change): bump the upstream
commit and/or toolchain pins and the engine version; CI builds on that push;
the release script signs, publishes `engine-v<version>`, writes the new pins,
and prints the table row for this file. Every Windows rig re-fetches on its
next start (the binary is hashed against the pin before it runs); Linux rigs
pick it up with their next package update.
