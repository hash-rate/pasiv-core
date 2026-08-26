# Security policy

Pasiv runs mining software on hardware you own, with your explicit consent, and never
takes custody of funds — pools pay your address directly. We take the security of the
app and its update channel seriously.

## Reporting a vulnerability

Email **support@pasiv.network** with `[SECURITY]` in the subject. Please include:

- what you found and where (app, updater, website, or release pipeline),
- steps to reproduce or a proof of concept,
- the version / commit and your platform.

Report privately first — please don't open a public issue for a security bug. We'll
acknowledge within a few days and keep you posted through the fix.

## Scope

**In scope:** this repository (the pasiv-core crate and the pasivd daemon), the update manifest + release
artifacts (`hash-rate/pasiv-releases`), and the website (`pasiv.network`).

**Out of scope:** the vendored third-party miners (XMRig, SRBMiner) and the pools —
report those upstream. Pasiv runs XMRig as a separate sidecar process, not linked into
the app.

## What we guarantee

- Updater packages are signed (minisign) and verified before install.
- macOS builds are Developer ID–signed and Apple-notarized.
- The fee address is a compile-time constant; changing it requires a signed release and
  a changelog entry (see [`docs/MONETISATION.md`](docs/MONETISATION.md) §5, the binding
  never-list).

## Design notes

Decisions that look odd without the reasoning behind them.

### The miner's local API token is never in the command line

XMRig exposes a control API on `127.0.0.1:42169`. Pasiv runs it **unrestricted**, because
the governor pauses and resumes in place and the fee slice swaps the payout address
without a costly RandomX re-init — both need write access. That makes the API token
equivalent to control of where the hashrate goes.

Process arguments are world-readable on macOS: any local process can run `ps -eo args`
and read another user's full command line. So the token is written to
`xmrig-runtime.json` in the app data directory, created `0600`, and passed with `-c`;
only the pool, the payout address and the thread count travel in `argv`. The token is
128 bits from the OS CSPRNG, regenerated per launch, and never persisted anywhere else.

### Content-Security-Policy

The webview holds IPC handles that start and stop mining and rewrite the payout address,
and it runs a large third-party wallet stack (Reown AppKit, WalletConnect, Solana). One
compromised dependency executing script in that context owns the miner, so `script-src`
allows no `'unsafe-inline'` and no `'unsafe-eval'` — `'wasm-unsafe-eval'` only, which
permits WebAssembly and nothing else.

`connect-src` is deliberately broad. wagmi/viem embed a registry of RPC and explorer
hosts for every EVM chain, so an allowlist there would break wallet sign-in on some
future chain without buying much: script execution, not egress, is what turns a
dependency bug into control of the miner.

Verified against the real bundle in headless Chrome — the app boots and renders under
this policy. Two things it deliberately blocks:

1. `@coinbase/wallet-sdk` injects an inline telemetry script at load
   (`script.textContent = <ClientAnalytics blob>`) even with `enableCoinbase: false`.
   Blocking inline script stops it; the SDK catches the failure itself, so nothing
   downstream breaks.
2. Reown's remote brand fonts (`fonts.reown.com`). The wallet modal falls back to the
   system face — cosmetic only, and consistent with self-hosting our own faces so no
   third party sees usage. Add that origin to `font-src` if the modal's typography
   matters more than the extra request.

Opening the wallet modal and completing a WalletConnect sign-in cannot be exercised
headlessly, so that path still wants a human pass after any CSP change.

### Known-unfixed dependency advisories

`npm audit` reports advisories in `bigint-buffer`, reached through
`@reown/appkit-adapter-solana` → `@solana/spl-token`. The package is unmaintained and has
no patched version. The advisory is a buffer overflow in its **native** addon; a webview
cannot load native addons, and the code Vite actually bundles is the pure-JS fallback
(`toString("hex")` → `BigInt`). Not reachable in the shipped app. Dropping the Solana
adapter — Phantom sign-in — would remove the subtree entirely if that trade is ever
worth making.
