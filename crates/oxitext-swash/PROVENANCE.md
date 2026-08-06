# Provenance of `oxitext-swash`

`oxitext-swash` is a **vendored fork**, not an original crate. This file is the
audit trail: what was forked, from where, why, exactly what diverges, and what is
knowingly still broken. It is meant to stay accurate enough that a future
maintainer can rebase onto a new upstream release — or un-fork entirely — without
re-deriving any of it.

## Upstream

| | |
|---|---|
| Crate | `swash` |
| Version | **0.2.10**, published to crates.io 2026-07-17 (the newest release; no 0.2.11 exists) |
| Repository | <https://github.com/dfrg/swash> |
| Upstream commit | `7773843` — "Bump version number to 0.2.10 (#132)"; `dfrg/swash` HEAD *is* this release commit |
| Author | Chad Brokaw \<cbrokaw@gmail.com\> |
| Upstream licence | `Apache-2.0 OR MIT` |
| Fork date | 2026-08-05 (OxiText 0.2.2) |
| Source of the copy | the crates.io registry checkout of `swash-0.2.10` (`src/` byte-copied, `LICENSE-APACHE` and `LICENSE-MIT` verbatim) |
| Size | 61 Rust files as vendored, **68 after the S9 split**; **22 970 code SLoC** (`tokei`) |

### Licence election

Upstream offers this work under `Apache-2.0 OR MIT`. **OxiText elects and
redistributes it under the Apache-2.0 arm**, which is exactly what upstream's
`OR` permits, so this crate inherits `license.workspace = true` (`Apache-2.0`)
like every other member of the workspace. `LICENSE-MIT` still ships beside
`LICENSE-APACHE` in this directory as provenance of upstream's offer — it is not
a grant OxiText extends. Both licence files are verbatim and are never edited,
including `Copyright (c) 2020 Chad Brokaw`. See `NOTICE`.

Per Apache-2.0 §4(b), every file this fork modifies carries a header of the form

```
// OXITEXT MODIFICATION (oxitext 0.2.2): <what changed>. See <...>PROVENANCE.md.
```

Files that were not modified are byte-identical to upstream — **37 of 61 still
are**. That was originally the whole audit: `diff -r crates/oxitext-swash/src
<registry>/swash-0.2.10/src` was meant to stay short and readable.

**As of the 2026-08-05 user election that brought this crate under COOLJAPAN house
style, byte-identity is no longer the binding rule.** The fork is ours: upstream is
dormant (#107), so the rebase path the identity argument protected is worth less than
the house rules the rest of the workspace lives by. The divergence table below and
the per-file `OXITEXT MODIFICATION` headers **are** the audit now, and both are
mandatory: any change to a file here updates both in the same commit.

### Why fork

1. **The blocking defect.** Two Indic reordering defects in
   `src/shape/buffer.rs` (below) made Devanagari shaping produce wrong glyphs and
   panic. In a wasm build a panic in the shaper takes down the whole canvas.
2. **There is nothing to ride.** Upstream issue **#93 "Panic While Shaping"** has
   been open since 2025-04-20 with no fix, no PR and no assignee.
   `src/shape/buffer.rs` has had no substantive change since 2021. Upstream issue
   **#107 "Migrating to harfrust"** (open 2025-08-17) records swash as dormant.
   Open PRs #130 and #135 touch neither defect.
3. **Two consumers in two crates.** `oxitext-shape` uses the shaper
   unconditionally and `oxitext-raster` uses the scaler behind `swash-backend`;
   both are published, so a path-only dependency is impossible and the fix has to
   live in a published crate of its own.

The crate is aliased back to its upstream name in the workspace
(`swash = { package = "oxitext-swash", ... }`), so **no oxitext-owned source file
changed a single `use swash::` line** when the fork landed.

---

## The fix — one bug, two symptoms

`reorder_complex` and `reorder_myanmar` build a permutation into `order`, a
scratch `Vec<usize>` owned by `shape::State` (`src/shape/mod.rs`) that
`State::reset()` **never clears** and `Vec::resize(len, 0)` only ever grows. When
the fill loop terminates with **`j < len`**, the tail of `order[..len]` still
holds indices written by a *previous cluster*, and the copy loop then reads them:

```rust
buf.copy_from_slice(glyphs);
for (i, j) in order.iter().enumerate() {
    glyphs[i] = buf[*j];        // <- the payload site
}
```

* a stale index `< len` → a glyph is **duplicated** and the intended one dropped
  → **defect (a)**, the reph silently replaced by a copy of a neighbour;
* a stale index `>= len` → **`index out of bounds`** → **defect (b)**, which is
  upstream **dfrg/swash#93**, reported via parley/vello_editor on `बर्नार्ड`.

`j < len` had three independent causes:

1. **The dropped reph (dominant).** The reph is marked `ignored`, and is
   re-emitted only when `last_base.is_none()` or when the sweep reaches
   `Some(i) == last_base`. But the *first* base is itself marked `ignored` as
   `first_base`, so for any single-base syllable (`र्ग र्क र्य र्व र्ष र्ण` — the whole
   reph vocabulary) `first_base == last_base`, the sweep `continue`s past it, the
   hook never fires and the reph is silently dropped. A permanent property, not a
   race.
2. **A length used as an exclusive range end**, at four sites (`VPre` and `VMPre`
   in `reorder_complex`, Myanmar `Anusvara` and `VPre` in `reorder_myanmar`), all
   `Some(r) => Some(r.start..i - r.start + 1)`. Correct by coincidence when
   `r.start == 0`; for `r.start = 2, i = 3` it yields the empty range `2..2` —
   two glyphs marked `ignored`, zero emitted, two stale slots.
3. **No invariant.** Nothing asserted that `order[..len]` was a permutation of
   `0..len`.

`मार्ग` was correct on a *fresh* shaper only by accident: `Vec::resize(len, 0)`
fills the hole with `0`, which for `[Reph, Halant, Base]` happens to be the
reph's own index. `स्वर्ग`'s first cluster leaves `order == [0,1,2]`, so the second
cluster's hole inherits the base index instead.

**The fix**, entirely inside `src/shape/buffer.rs` (7 hunks):

1. All four range ends corrected to `r.start..i + 1`.
2. Every `order[j] = i; j += 1;` routed through an `emit!` macro with three
   guards — `index < len` (no OOB writes), `!placed[index]` (no duplication,
   which is what makes the now-possibly-overlapping VPre ranges safe) and
   `j < len` (no overflow of `order[..len]`). A `macro_rules!` rather than a
   closure because the body mutates three locals the surrounding code also reads;
   it keeps the original control flow line-for-line reviewable against upstream.
3. The hole is closed explicitly (`if let Some(i) = reph { emit!(i); }` — the
   *correctness* fix, naming the single-base case), then swept
   (`for i in 0..len { emit!(i); }` — the *safety* fix, so a leaked index degrades
   to an identity fallback instead of a wasm-fatal panic), then
   `debug_assert_eq!(j, len)`. `reorder_myanmar` gets the same treatment plus a
   trailing `anus.take()` drain, losing its `anus.take().unwrap()` to an `if let`
   on the way.

`reorder_myanmar` is fixed too: it shares the same never-cleared `order`, its
consumer `push_order` indexes `chars` with it, and it carries two of the four
length-as-range-end sites.

### On `debug_assert_eq!(j, len)`

`CONTRIBUTING.md` forbids `assert!`/`panic!` on data derived from untrusted
input. The retained `debug_assert_eq!` is **not** a violation: the completion
sweep immediately above it makes the condition unreachable, and `debug_assert` is
a release no-op. Its job is to keep a future logic leak loud in dev and test
builds while the release path degrades safely. The shaping tests therefore must
keep running in the dev profile.

### Evidence

| Measurement | Pristine 0.2.10 | This crate |
|---|---|---|
| 24-word Hindi corpus, one reused shaper, Noto Sans Devanagari 2.006 | **4 panics** (`पूर्ण`, `वर्तमान`, `आदर्श`, `संघर्ष`) + silent reph loss | **0 panics**; every reph-bearing word carries gid 506 exactly once |
| `स्वर्ग`, fresh shaper | `[256, 84, 58, 58]` (reph replaced by a copy of the base) | `[256, 84, 58, 506]` |
| `"दिल्ली मार्ग"`, one call | `[544, 73, 252, 83, 33, 3, 80, 31, 58, 58]` | `[544, 73, 252, 83, 33, 3, 80, 31, 58, 506]` |
| `"सूर्य पूर्व वर्षा मार्ग"`, one call | **panic** at the copy site | 19 glyphs, one reph per word |
| fresh shaper vs reused shaper over the 24 words | 4 disagree | all 24 equal |
| `बर्नार्ड` (issue #93's reproducer, Nirmala) | panic | `[271, 267, 301, 330, 260, 330]` |
| A/B sweep, 21 font×script cases, ~90 strings, fresh and reused, comparing glyph id + x/y + advance | — | **6 changed lines, every one a fix**: 4 Devanagari gain their reph, Bengali `র্ক` `885 885`→`885 954`, Oriya `ର୍କ` `1201 1201`→`1201 1258`. **Zero** change to Latin (including kerning and `fi/fl/ffi/ffl`), Cyrillic, Greek, Arabic, Hebrew, Thai, Japanese, Han, Hangul, **Myanmar**, Tamil, Telugu, Gujarati, Gurmukhi, Malayalam, Kannada, Sinhala, Javanese |
| Randomized stress, 440 000 strings, 1–24 chars, 14 script/font pairs, shared reused `ShapeContext`, dev profile | **12 panics** | **0 panics, 0 `debug_assert_eq!` failures** |

The shipped regression corpus is
`crates/oxitext-shape/tests/devanagari_reorder.rs` (6 cases, 5 of which were
verified RED against the pristine vendored code before the fix landed) plus the
permutation-invariant unit tests at the bottom of `src/shape/buffer.rs` (8 cases,
font-independent, including a deliberately pre-poisoned `order`).

`reorder_complex` is reached only from the `EngineMode::Complex` arm of
`Shaper::add_cluster`, and mode selection is
`if gsub.lang != 0 && script.is_complex() { Myanmar or Complex } else { Simple }`.
Latin, Arabic, Hebrew, CJK and Hangul take `EngineMode::Simple` and never enter
either changed function — the structural half of the same claim the A/B sweep
makes empirically.

### Upstream credit

The fix is original, but **dfrg/swash#93 "Panic While Shaping"** (open since
2025-04-20) is the prior public report of defect (b): same function, same line,
same message shape. It must be cited in any upstream PR. Defect (a) has no
upstream issue (a GitHub issue search for "devanagari" over `dfrg/swash` returns
`total_count 0`).

The tree state immediately after the fix landed — the fix and nothing else, no
conformance churn — is preserved as the **upstream-offerable diff**, so a PR can
be cut from it if the user ever authorizes one. Posting anything to `dfrg/swash`
requires explicit user approval and has not been done.

---

## Divergence from upstream — complete, per file

**37 of the 61 source files are still byte-identical to upstream.** The 24 that
differ, plus the one that became a directory (`text/unicode_data.rs`, see the split
table below):

| File | Hunks | What changed |
|---|---|---|
| `shape/buffer.rs` | 9 | **The fix** (7 hunks in `reorder_complex` / `reorder_myanmar`), the §4(b) header, and an appended `#[cfg(test)] mod tests` (8 permutation-invariant tests; upstream has none in this file). |
| `shape/mod.rs` | 12 | 10 doctest imports `use swash::` → `use oxitext_swash::`; the 6 `self.store.unwrap()` sites converted to `let ... else` / `if let`. |
| `scale/mod.rs` | 14 | 8 doctest imports; `not(feature = "std")` added to the `core_maths` import `cfg`; `derivable_impls` on `Default for Source`; one `unused_mut`. |
| `font.rs` | 3 | 2 doctest imports. |
| `strike.rs` | 8 | `not(feature = "std")` added to the `core_maths` import `cfg`; 7 `manual_div_ceil` sites. |
| `cache.rs` | 3 | 2 `.unwrap()` sites converted (`try_into().unwrap()` → `as u64`; `entries.last().unwrap()` → index of the just-pushed element). |
| `metrics.rs` | 2 | 1 `.unwrap()` site → `map_or(1, ...)`. |
| `feature/at.rs` | 2 | 1 `.unwrap()` site → `?`. |
| `shape/at.rs` | 6 | 1 `.unwrap()` site → `let ... else`; 3 `sort_unstable_by` → `sort_unstable_by_key`; 1 `explicit_counter_loop`. |
| `shape/partition.rs` | 2 | 1 `.unwrap()` site → `let ... else`. |
| `lib.rs` | 1 | Crate-level allows pruned from five to **one** (`too_many_arguments`, with its rationale); `#![deny(clippy::unwrap_used, clippy::expect_used)]` added; the `compile_error!` feature guard added; the `//!` provenance block added. |
| `scale/bitmap/mod.rs` | 2 | `not(feature = "std")` added to the `core_maths` import `cfg`; 1 `explicit_counter_loop`. |
| `text/lang_data.rs` | 2 | `LANG_ENTRIES` `const` → `static`; `&'static str` → `&str`. |
| `attributes.rs` | 3 | `derivable_impls` (`Default for Style`). |
| `scale/image.rs` | 3 | `derivable_impls` (`Default for Content`). |
| `text/cluster/char.rs` | 4 | `derivable_impls` (`Default for ShapeClass`). |
| `scale/bitmap/png.rs` | — | **`yazi` → `oxiarc-deflate`** (see below); 4 `manual_div_ceil` sites; an appended `#[cfg(test)] mod tests` (4 decode tests). |
| `text/compose.rs` | 2 | 1 `manual_is_multiple_of` site. |
| `internal/at.rs` | 2 | `len_zero`. |
| `internal/glyf.rs` | 2 | `needless_lifetimes`. |
| `internal/head.rs` | 2 | `redundant_static_lifetimes`. |
| `feature/util.rs` | 2 | `redundant_static_lifetimes`. |
| `string.rs` | 2 | `redundant_static_lifetimes`. |
| `shape/aat.rs` | 6 | `unused_enumerate_index` ×4. |

Every hunk count includes the file's §4(b) header. Every change outside
`shape/buffer.rs` is either a doctest import rename, a `cfg` correction, an
`.unwrap()` conversion, or a machine-applicable clippy fix — **no behaviour
change**.

Files deliberately **not** vendored from the upstream `.crate`: `.cargo-ok`,
`.cargo_vcs_info.json`, `Cargo.lock`, `Cargo.toml.orig`, `.gitignore`,
`.github/`, `.typos.toml`, upstream's `README.md` (ours replaces it), and
`.clippy.toml` — whose `doc-valid-idents` was folded into the **workspace**
`clippy.toml` instead. A crate-local `clippy.toml` does not inherit the workspace
one, so keeping it would have silently dropped `msrv = "1.89"` for these 22.9k
vendored lines, which is exactly where an MSRV-raising clippy suggestion could
land unnoticed.

### The `text/unicode_data` split

Upstream shipped the generated Unicode character database as a single
**5 491-line** `src/text/unicode_data.rs`, the only file in the crate over the
workspace's 2000-line limit. It is now a directory module, split along the natural
seams of the generated data, with **the tables byte-for-byte unchanged** and every
public path preserved by glob re-exports in `mod.rs` — `unicode_data::X` still
resolves for every `X`, so not one consumer changed:

| Submodule | Lines | Upstream lines | Contents |
|---|---|---|---|
| `mod.rs` | 40 | — | module declarations, re-exports, `#![allow(dead_code)]` |
| `enums.rs` | 813 | 1–811 | `UNICODE_VERSION` and the 11 property enums |
| `script_tables.rs` | 331 | 812–1135 | `SCRIPT_TAGS`, `SCRIPTS_BY_TAG`, `SCRIPT_NAMES`, `SCRIPT_COMPLEXITY`, `BRACKETS`, `MIRRORS` |
| `record_index.rs` | 1 469 | 1136–2598 | the three-level code-point → record trie and `get_record_index` |
| `records.rs` | 1 146 | 2599–3735 | `Record`, `Flags`, `r()` and the 2 035-entry `RECORDS` |
| `compose.rs` | 353 | 3736–4083 | canonical-composition tables and `compose_index` |
| `decompose_index.rs` | 686 | 4084–4764 | canonical and compatibility decomposition tries |
| `decompose.rs` | 732 | 4765–5491 | `DECOMPOSE` and `DECOMPOSE_COMPAT` |

The only edits beyond the cut are three import lines (`records.rs` and
`script_tables.rs` now reach the enums through `super::enums::…` where the single
file used `self::…`) and the `#![allow(dead_code)]` moved up to `mod.rs`, which
covers every submodule. Verified by normalised comparison against the pre-split
file: identical modulo exactly those edits. The private trie tables stay private,
beside the functions that index them.

### `yazi` → `oxiarc-deflate`

Upstream inflated the zlib stream of PNG-embedded `CBDT`/`sbix` strikes with
`yazi`, feeding each IDAT chunk into a streaming decoder as it walked the chunk
list. OxiText replaced it with **`oxiarc-deflate`** — the same pure-Rust COOLJAPAN
inflater `oxitext-core`'s own PNG reader uses — per CLAUDE.md's
dependency-replacement table and the minimum-dependency rule. `yazi` is gone from
every manifest and from every feature combination of the dependency graph, and
`deny.toml` bans it so it cannot creep back.

`oxiarc_deflate::zlib_decompress` is one-shot, so the chunk payloads are collected
and inflated once. By the PNG spec (11.2.4) the zlib stream **is** the concatenation
of the IDAT payloads, so this is semantically exact rather than an approximation;
the single-chunk case — overwhelmingly the common one for an embedded strike — is
borrowed with no copy, and only a multi-chunk stream is concatenated. Two behaviour
notes, both recorded in the file's own header: `zlib_decompress` verifies the
Adler-32 trailer, which the streaming path did not, so a corrupt strike is now
rejected rather than half-decoded; and the caller's scratch `Vec` is replaced rather
than filled in place, because an Adam7 interlaced stream does not inflate to
`(pitch + 1) * height` and a hand-derived bound would silently truncate. Neither
copies pixel data.

**`scale` now implies `std`.** `oxiarc-deflate 0.4` is std-only by design
(`std::io`, `std::sync`, `std::thread`), where `yazi` was `no_std`-capable, so
`no_std` + `scale` is no longer buildable. It has no consumer in this workspace, and
`no_std` *shaping* — `libm` without `scale` — is untouched. Regression coverage for
the whole seam is the 4 tests at the bottom of `src/scale/bitmap/png.rs`, which
build real zlib streams with `oxiarc-deflate` and decode them back to exact pixels,
including across 2, 3 and 5 IDAT chunks.

### Manifest divergence

`Cargo.toml` is ours, not upstream's normalized one, but preserves upstream's
`[features]` table verbatim (`default = ["std", "scale", "render"]`), its
`[lints.clippy]` table (`doc_markdown`, `semicolon_if_nothing_returned`) and its
`[package.metadata.docs.rs] all-features = true`. There is **no `[lib] name`
section**: the library is `oxitext_swash`, not `swash`, so a published crate
cannot squat another crate's lib name or collide for anyone depending on both.
That rename is what the 17 doctest import rewrites pay for.

Its `[features]` table is upstream's with one change: `scale` gained `std` and
swapped `dep:yazi` for `dep:oxiarc-deflate` (above).

Dependencies are inherited from `[workspace.dependencies]` per
`CONTRIBUTING.md`, and `skrifa` is **pinned at 0.44** — the top of upstream's
`>= 0.31.1, <= 0.44` range and what OxiText resolves today — rather than left as
a 13-minor-version range a downstream lockfile could drift under us.

### The `64` / `MAX_CLUSTER_SIZE` coupling (a trap, not a bug)

`MAX_CLUSTER_SIZE` is **32** (`src/text/cluster/cluster.rs`). `reorder_complex`
uses `[false; 64]` for both `ignored` and the new `placed`, matched by
`shape/mod.rs`'s `.min(64)` clamp on the glyph count; `reorder_myanmar` uses
`[false; MAX_CLUSTER_SIZE]` for both, matching its own `ignored` and its `chars`
bound. The fix is internally consistent in both functions. **Do not "unify" the
constants** — the real coupling is `reorder_complex`'s literal `64` ↔
`mod.rs`'s `.min(64)`, and it is a latent trap for whoever tackles upstream #105.

---

## Inherited `unsafe`

`oxitext-swash` is the **first and only** OxiText crate without
`#![forbid(unsafe_code)]`; the other seven all carry it. It inherits **54
`unsafe` sites — 37 blocks and 17 `unsafe fn` — across 8 files**, essentially all
`read_unchecked::<T>()` over untrusted font bytes:

| File | Sites |
|---|---|
| `internal/parse.rs` | 18 |
| `internal/at.rs` | 10 |
| `internal/cmap.rs` | 8 |
| `text/compose.rs` | 7 |
| `text/unicode.rs` | 4 |
| `text/lang.rs` | 3 |
| `internal/aat.rs` | 3 |
| `internal/fixed.rs` | 1 |

None of it was written by OxiText and none of it was rewritten in 0.2.2 — the
absorption's whole claim is "nothing changed except the fixes". What ships
instead is this inventory, the crate-level doc paragraph naming the situation,
and the fuzz target `fuzz/fuzz_targets/shape_untrusted_font.rs`.

**Deliberately NOT added in 0.2.2:** `#![warn(clippy::undocumented_unsafe_blocks)]`
(37 blocks × `-D warnings` = an instantly red gate) and
`#![deny(unsafe_op_in_unsafe_fn)]` (unmeasured; would spread mechanical edits
across `internal/parse.rs` and make a rebase harder for zero 0.2.2 benefit). Both
lints, and de-unsafing `internal/parse.rs`, are 0.2.3 items. See `SECURITY.md`.

Publishing this crate makes that inherited `unsafe` a public commitment: 54
unchecked reads over attacker-controlled bytes, with upstream #123–#126 and #133
already fuzz-found in that layer, and no upstream to escalate to.

## Known-unfixed upstream defects

Carried, not fixed, and not hidden:

* **#105** — clusters exceeding `MAX_CLUSTER_SIZE` produce overlapping source
  ranges. Real, open and adjacent (`add_cluster` clamps `reorder_complex` to
  `.min(64)` glyphs, so a >64-glyph cluster leaves its tail unreordered), but it
  is a parser/cluster-size defect with a different mechanism and a different fix,
  it causes neither defect fixed here, and folding it in would make the shaping
  fix unreviewable.
* **#123, #124, #125, #126, #133** — fuzz-found panics in the inherited parse
  layer. The fuzz target ships; running a campaign and triaging its findings is
  0.2.3. Any smoke-run finding goes to `TODO.md`.

## Standing rules for this directory

* **The 2000-line rule applies here like everywhere else** (user election,
  2026-08-05 — the exemption this file recorded on the day of the fork is
  revoked). Every file is under 2000 lines; the largest are `internal/aat.rs`
  (1 830) and `shape/at.rs` (1 826), both left whole. `splitrs` may be used, but
  read the S9a note in the workspace `TODO.md` first — it was run here and its
  output rejected for a measured reason.
* **Never edit a file that has no `OXITEXT MODIFICATION` header** without adding
  one, and update the divergence table above in the same change. Since
  byte-identity is no longer the audit, those two things are.
* **Never edit `LICENSE-APACHE` or `LICENSE-MIT`.**
* **Keep this file current.** With the divergence table and the `diff -r` audit,
  a future 0.2.11 rebase — or an un-fork — stays tractable. Without them the fork
  becomes unrebasable within one release. Upstream #107 means the shaper may be
  deleted upstream entirely, in which case this becomes permanently OxiText's own
  22.9k-line shaper. That should be a conscious choice, not a surprise.

## Re-running the audit

```sh
# 37 of 61 files must report no difference at all.
diff -r crates/oxitext-swash/src \
  "$CARGO_HOME/registry/src/index.crates.io-<hash>/swash-0.2.10/src"
```
