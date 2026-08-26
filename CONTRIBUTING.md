# Contributing to pasiv-core

Thanks for looking. Issues — especially anything touching the fee path, the
ledger, a pool endpoint, or an address validator — are very welcome. Security
reports go through [SECURITY.md](SECURITY.md), not the public tracker.

## Before you open a PR: the contribution grant

pasiv-core is dual-licensed: public GPL-3.0-only, plus the copyright holder's
own proprietary use in the Pasiv applications. To keep that possible, every
contribution needs a copyright grant. By submitting a pull request you agree
that:

1. Your contribution is your own work, and you have the right to submit it.
2. You grant the Pasiv maintainers a perpetual, worldwide, irrevocable,
   royalty-free licence to use, modify, sublicense, and relicense your
   contribution — including in proprietary Pasiv products.
3. Your contribution is otherwise offered under GPL-3.0-only, like the rest
   of the repository.

Add a `Signed-off-by:` line to your commits (`git commit -s`) to record your
agreement. PRs without it can't be merged, however good the code — sorry.

## Ground rules for changes

- **The never-list is binding** ([docs/FEES.md](docs/FEES.md) §4). A PR that
  violates it will be closed regardless of technical merit.
- **The fee constants only change with a release.** `FEE_ADDRESS_XMR`,
  `SLICE_WINDOW_SECS`, and `SLICE_SECS` are deliberately compile-time; a PR
  touching them needs a maintainer-driven release and changelog entry.
- **Tests pin behaviour, not lines.** If you change behaviour, change the
  test that pinned it and say why in the commit message — the suite is full
  of tests that exist because something real broke once.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace` must all pass; CI enforces them.

## What lives here vs. the private repos

GUI, companion apps, cloud backend, signing, and release infrastructure are
proprietary and out of scope for this tracker — see the README's "What is not
open" section. Bugs in the *shipped* apps are still welcome as issues; if the
root cause turns out to live in this crate, it gets fixed here in the open.
