# oxitext-swash

**A vendored fork of [`swash`](https://github.com/dfrg/swash) 0.2.10 by Chad
Brokaw, with Indic reordering fixes.** Font introspection, complex text shaping
and glyph rendering — the same crate, the same API, with two shaping defects
fixed.

This crate exists because upstream `swash` mis-shaped and then panicked on
ordinary Devanagari text, and upstream is dormant: issue
[#93 "Panic While Shaping"](https://github.com/dfrg/swash/issues/93) has been
open since 2025-04-20 with no fix, and
[#107](https://github.com/dfrg/swash/issues/107) records the author's time as
moved to fontations/HarfRust. `src/shape/buffer.rs` has had no substantive change
since 2021.

Read `PROVENANCE.md` before changing anything here. It is the audit trail:
what was forked, exactly which 24 of 61 files diverge and why, the inherited
`unsafe` inventory, and the standing rules for the directory (no `splitrs`, no
silent edits to unmodified files).

## What was fixed

One bug with two symptoms, in `swash::shape::buffer`. Both `reorder_complex` and
`reorder_myanmar` write a permutation into `order`, a scratch buffer owned by
`ShapeContext` that is never cleared and only ever grown. When the fill loop
ended early, the tail of that permutation still held indices from a *previous
cluster*:

* **Silent glyph corruption** — a stale in-range index duplicated one glyph and
  dropped another. The reph of every single-base syllable (`र्ग`, `र्क`, `र्य`, …)
  came out as a copy of its neighbour. `स्वर्ग` shaped to `[256, 84, 58, 58]`
  instead of `[256, 84, 58, 506]`, on a brand-new shaper.
* **`index out of bounds`** — a stale out-of-range index panicked. This is
  upstream #93; in a wasm build it takes down the whole canvas.

The fix corrects four range ends that used a length where an index was needed,
routes every emission through a guarded `emit!` (in range, not already placed,
still room), then closes the remaining hole explicitly and sweeps anything left,
so a future logic leak degrades to an identity fallback instead of a panic. A
`debug_assert_eq!` keeps such a leak loud in dev and test builds.

Verified: 0 panics over a 24-word Hindi corpus that panicked 4 times before, a
reused shaper now equal to a fresh one for all 24, and a 21-case font×script A/B
sweep in which the *only* changes are 6 lines, every one a fix — with Latin,
Arabic, Hebrew, CJK, Hangul, Thai, Myanmar and eight other Indic scripts
byte-identical.

## Using it

Inside the OxiText workspace this crate is aliased back to its upstream name, so
consumers write ordinary `swash` code:

```toml
swash = { package = "oxitext-swash", version = "0.2.4" }
```

```rust
use swash::shape::ShapeContext;
use swash::FontRef;
```

The library name is `oxitext_swash` (there is deliberately no `[lib] name =
"swash"`, so this crate cannot collide with upstream for anyone who depends on
both). If you depend on it directly, without the alias, import
`oxitext_swash::…`.

### Features

Upstream's feature table, unchanged: `default = ["std", "scale", "render"]`, plus
`libm` for `no_std` builds. Exactly one of `std` or `libm` is required — with
neither you now get a named error instead of a confusing failure inside
`read-fonts`. `scale` (and therefore `render`) pulls `yazi` and `zeno`; a
shaping-only consumer should take `default-features = false, features = ["std"]`
and leave them out of the graph entirely.

## Licence

Upstream `swash` is offered under `Apache-2.0 OR MIT`. OxiText elects and
redistributes it under the **Apache-2.0** arm, matching the rest of the
workspace. `LICENSE-MIT` ships here as provenance of upstream's offer, not as a
grant OxiText extends. See `NOTICE` and `PROVENANCE.md`.
