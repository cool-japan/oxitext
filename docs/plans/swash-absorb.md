# swash absorption — vendor `swash` 0.2.10 as `oxitext-swash` 0.2.2 and fix its Indic reordering defects (synthesized 2026-08-05)

Three-lens Opus design (root-cause / vendoring-and-packaging / integration-and-downstream),
synthesized on the 2-of-3-plus-evidence rule. Scope: absorb `swash` 0.2.10 into the OxiText
workspace as a published member crate and land the proven fix for the two Indic defects that
block OxiGIS print v1.4 item 1. Target repo `I:\rust\oxitext`, branch `0.2.2`, base commit
`5c82218`, tree clean at synthesis time. **Nothing in this plan is committed, pushed or
published** — no push authorization exists for oxitext; `MOS`/`WS`/`Win` commits are the user's.
`I:\rust\oxigis` is READ-ONLY for this work; its section here is a record, not a task list.

Every number below was measured by one of the three lenses in the session scratchpad, or
re-verified against the trees during synthesis (marked *[judge]*). No cargo command was run in
either repository during design.

Path shorthands used throughout — expand at use, do not hardcode elsewhere:

- `$SCRATCH` = `C:\Temp\claude\I--rust-oxigis\5bb367ef-1100-4418-a5f9-f24fba197f10\scratchpad`
- `$REG` = `%CARGO_HOME%\registry\src\<crates.io index>\swash-0.2.10`
  (resolved on this machine to `index.crates.io-1949cf8c6b5b557f`; read-only, never modify)

## Evidence artifacts (existence re-verified 2026-08-05 *[judge]*)

| Artifact | What it is | Re-verification |
|---|---|---|
| `$SCRATCH\swash-fix.patch` | The root-cause lens's VERIFIED diff to `src/shape/buffer.rs` | Present, 1 465 lines. It is a **whole-file** diff (single `@@ -1,708 +1,754 @@` hunk) produced without `--strip-trailing-cr`; the true semantic delta is **7 hunks / ~120 lines**, obtained with `diff --strip-trailing-cr -u`. Use the patched **file**, not this patch, to apply (D5). |
| `$SCRATCH\swash-probe\swash\` | Patched tree (the authoritative fix content) | Present. `diff -rq --strip-trailing-cr` against `swash-orig` reports **exactly one differing file**: `src/shape/buffer.rs`. Nothing else was touched. |
| `$SCRATCH\swash-probe\swash-orig\` | Pristine 0.2.10 control | Present; byte-equal to `$REG\src` except line endings. |
| `$SCRATCH\NotoSansDevanagari-Regular.ttf` | Redistributable OFL fixture | Present. **244 284 bytes**, SHA-256 `306b53ecfb182a504dd8a7446093c316387d2fd8dc350d0792ed1753fe0996cd` — re-hashed *[judge]*, matches the integration lens exactly. |
| `$SCRATCH\out-orig.txt` / `out-final.txt` | 21 font×script A/B regression sweep | Present (21 691 / 21 477 bytes). |
| `$SCRATCH\swash-design\swash-{rootcause,vendoring,integration}.md` | The three lens reports | Present; all three read in full. |

---

## Decision record

| # | Decision | Evidence |
|---|---|---|
| **D1** | **New workspace member `crates/oxitext-swash`, published to crates.io.** Not a module inside `oxitext-shape`, not a home in `I:\rust\oxifont`, not a shape-only subset. | 3/3 lenses. Two consumers in two crates (`oxitext-shape` unconditional; `oxitext-raster` behind `swash-backend`) — a module in `-shape` would force `-raster` to depend on `-shape` to reach a rasterizer, inverting `shape → … → raster`. `oxifont` is a separate repo on a separate release train (oxitext consumes published `oxifont 0.2.1`); routing an OxiGIS-blocking fix through an oxifont release adds a second cross-repo dependency to the critical path. A shape-only subset still drags `internal/` (~4.4k lines), `font.rs`, `strike.rs`, `metrics.rs`, `text/` — ~80 % of the crate — and keeps a second copy on the registry side. `oxitext-shape`/`-raster` are published, so a path-only dep is impossible: `cargo publish` rejects a `path` dep without `version`. Name free: `https://crates.io/api/v1/crates/oxitext-swash` → **404** (verified by two lenses independently). |
| **D2** | **Dependency alias, zero import rewrites in oxitext-owned code.** Root `[workspace.dependencies]` gains `swash = { package = "oxitext-swash", path = "crates/oxitext-swash", version = "0.2.2", default-features = false, features = ["std"] }`; the `swash = { version = "0.2.9" }` line is deleted. | 3/3 lenses; two built it. `crates/oxitext-shape/Cargo.toml:24` (`swash = { workspace = true }`) stays **byte-unchanged**; `crates/oxitext-raster/Cargo.toml:52` gains only `features = ["scale", "render"]`. Verified twice: a scratch consumer with verbatim `use swash::shape::{Direction, ShapeContext}; use swash::text::{Language, Script}; use swash::{tag_from_bytes, FontRef};` builds; and the full oxitext workspace copy passes **797 / 0 / 20** with the vendored+fixed crate and **not one line changed in any oxitext-owned source file**. Rewriting ~40 `swash` mentions across 9 files to `oxitext_swash::` is pure churn and would stop the crate being a drop-in for third parties. |
| **D3** | **`[lib] name` is the default (`oxitext_swash`); do NOT set `[lib] name = "swash"`.** Cost: **17 doctests** in 3 files (`font.rs`, `shape/mod.rs`, `scale/mod.rs`) whose `use swash::` becomes `use oxitext_swash::`. | Contradiction between lenses, ruled on hard evidence: the integration lens is the only one that ran doctests **after** the rename — 17 fail with `unresolved import 'swash'`, and the 3-file rewrite gives **17 passed / 0 failed**. The root-cause lens's "17/17 pass" was measured on a copy still named `swash`, so it is not contrary evidence. `[lib] name = "swash"` would let a published crate squat another crate's lib name and collide for anyone who legitimately depends on both. **`cargo nextest` does not run doctests** — hence D20's mandatory `cargo test --doc`. |
| **D4** | **Feature mapping: `default-features = false, features = ["std"]` on the WORKSPACE line; members opt back in.** The vendored crate keeps upstream's `default = ["std", "scale", "render"]` in its own manifest. | 3/3 on the split; the placement is a measured Cargo gotcha (integration): `default-features = false` on a **member** line is silently ignored when the dep is inherited — `yazi`/`zeno` stayed in `cargo tree` until it moved to the workspace line. Verified effect: the default oxitext graph becomes `oxitext-shape → oxitext-swash → skrifa` only; `cargo tree -p … -i yazi` → *nothing to print*. `yazi 0.2.1` and `zeno 0.3.3` are in `I:\rust\oxigis\Cargo.lock` **today** (lines 5780, 5906) purely as unused freight. Keeping upstream's `default` in the member manifest preserves drop-in semantics for third parties; the lean graph is taken at the workspace entry, which is ours to set. Consumer needs (grep-verified, both lenses agree): `-shape` = `std` only; `-raster/swash-backend` = `std + scale + render`. |
| **D5** | **The fix is the root-cause lens's verified patch, applied by copying `$SCRATCH\swash-probe\swash\src\shape\buffer.rs` over the vendored file — byte-for-byte, not retyped.** | The 440 000-string stress result, the 21-case A/B sweep and the end-to-end oxitext-shape run were all measured against *that exact file*. Retyping risks a transcription defect in code whose entire value is that it was proven. *[judge]*: the file differs from pristine in 7 hunks; the only cosmetic delta is three removed commented-out `println!` debug lines in `reorder_complex` — record them in `PROVENANCE.md`, do not "restore" them. |
| **D6** | **The fix covers BOTH `reorder_complex` and `reorder_myanmar`.** | Contradiction (root-cause: both; integration: complex only), ruled for the root-cause lens on evidence. `reorder_myanmar` shares the same never-cleared `state.order` (`shape/mod.rs:604`) and its consumer `push_order` indexes `chars` with it (`buffer.rs:131`, `:144`), so the identical staleness corrupts character order there. It also carries two of the four length-as-range-end sites. The A/B sweep included 6 Myanmar strings (kinzi `သင်္ချိုင်`, medial-ra) and the patched output is **byte-identical to pristine** — patching Myanmar is verified non-regressive. The fix additionally removes the crate's `anus.take().unwrap()` (`buffer.rs:530`), one of the 13 unwraps (D13). |
| **D7** | **Fixture: `tests/fixtures/NotoSansDevanagari-Regular.ttf` at the WORKSPACE ROOT**, registered in `tests/fixtures/README.md`'s existing table with bytes / SHA-256 / source / licence and a `curl` regeneration recipe. Nirmala.ttc is investigation-only and never ships. | Integration lens, hard evidence, and it is the only lens that proved the defects reproduce on a redistributable face. Source `notofonts/notofonts.github.io` → `fonts/NotoSansDevanagari/hinted/ttf/NotoSansDevanagari-Regular.ttf`, blob `8dd3ec4a`; 244 284 B; SHA-256 `306b53ec…96cd` (re-hashed *[judge]*); OFL-1.1, already on `I:\rust\oxigis\deny.toml`'s licence allow-list. Smaller than the existing `test-font.ttf` (569 208 B). The workspace-root location and the **skip-gracefully-when-absent** contract are `tests/fixtures/README.md`'s own stated rules (*[judge]*, read in full), and keep 244 KB out of every published `.crate`. `oxifont-bundled 0.2.1 {bundled-noto}` (a dev-dep of `-raster`/`oxitext`) is **not** used as the fixture: its Devanagari coverage is unverified and it would tie a shaping regression test to another repo's release train. |
| **D8** | **Every glyph-id golden must be RE-MEASURED against the Noto fixture with the FINAL (D5) patch before it is written into a test.** The integration lens's Noto numbers are the *expected* values, not authorities. | The Noto numbers were measured with a completion-only patch; D5's patch additionally fixes four length-as-range-end sites, two of which are `VPre`/`VMPre` in `reorder_complex` — and `दिल्ली` contains a VPre matra (ि). On Nirmala the first four glyphs of `दिल्ली मार्ग` were unchanged by the full patch, which is reassuring but is not proof for Noto. Any deviation from the expected values below is a **finding to investigate as a VPre-range effect**, not a number to silently record. |
| **D9** | **Code quality: full clippy cleanup, exactly TWO documented crate-level allows, no `#[allow]` wall, no `-A clippy::all`.** Keep `#![allow(clippy::too_many_arguments)]` (7 sites in the OpenType layout engine's internal recursion) and add `#![allow(clippy::large_const_arrays)]` (5 sites, generated tables). Delete `float_cmp`, `many_single_char_names`, `needless_lifetimes`, `redundant_static_lifetimes`. | 3-way contradiction, ruled by the gate: `CONTRIBUTING.md` mandates `cargo clippy --all-targets -- -D warnings` = **zero** (*[judge]*, read in full), so the root-cause lens's "defer the 42 warnings" is not an option — the workspace gate would be red. Between the other two: the vendoring lens **measured** that removing the five upstream allows adds 16 warnings of which `float_cmp` and `many_single_char_names` fire **zero** times (vestigial) and 6 are machine-applicable lifetime lints; that beats the integration lens's unmeasured "keep all five". `large_const_arrays` originally got a crate-level allow to keep the generated tables byte-identical to upstream; **AMENDED by the D12 user election**: byte-identity for those tables is no longer binding, so S9 takes clippy's `const`→`static` conversion where it fires and DROPS the `large_const_arrays` allow if the crate is then clean (measure; keep only `too_many_arguments`, whose refactor stays 0.2.3-at-earliest). `cargo clippy --fix` is verified to work end to end (~20 fixes applied, exactly 3 residuals: `scale/mod.rs:556 unused_mut`, `scale/bitmap/mod.rs:53` and `shape/at.rs:915 explicit_counter_loop`; `cargo fmt -p oxitext-swash` cleared the 9 fmt diffs it left; consumer build Finished). |
| **D9b** | **Delete `crates/oxitext-swash/.clippy.toml`; fold `doc-valid-idents = ["ClearType", "HarfBuzz", "OpenType", "PostScript", ".."]` into the workspace `clippy.toml` next to `msrv = "1.89"`.** If `doc_markdown` still fires after the fold, backtick the 2 sites (`internal/head.rs:70`, `shape/mod.rs:236`). | A crate-local `.clippy.toml` does **not** inherit the workspace one — it would silently drop `msrv = "1.89"` for the 22.7k vendored lines, which is exactly where an MSRV-raising clippy suggestion could land. The vendored manifest's `[lints.clippy] doc_markdown = "warn"` + `semicolon_if_nothing_returned = "warn"` is kept verbatim (*[judge]*: both present in `$REG\Cargo.toml`), and under `-D warnings` those *are* gate-relevant. |
| **D10** | **USER ELECTION 2026-08-05 (「Apache で統一で」): `crates/oxitext-swash/Cargo.toml` declares `license.workspace = true` (= `Apache-2.0`), like every other member.** OxiText redistributes Chad Brokaw's dual-licensed (`Apache-2.0 OR MIT`) work under its Apache-2.0 arm — that sentence ships in `NOTICE` and `PROVENANCE.md`. Both `LICENSE-APACHE` and `LICENSE-MIT` still ship verbatim (including `Copyright (c) 2020 Chad Brokaw`): `LICENSE-MIT` is retained as provenance of upstream's offer, not as a grant we extend. Every other crate and the root `LICENSE` unchanged. | The synthesis originally ruled `Apache-2.0 OR MIT` on the member (2/3 lenses) with an explicit USER-OVERRIDE point; the user elected Apache-2.0 unification the same day. Per the recorded override recipe the change is the manifest license line plus the `NOTICE`/`PROVENANCE.md` sentence — nothing else in this plan moves. Electing the Apache arm is exactly what upstream's `OR` permits. |
| **D11** | **Provenance file set** under `crates/oxitext-swash/`: `LICENSE-APACHE`, `LICENSE-MIT` (both verbatim, never edited), `NOTICE`, `PROVENANCE.md`, `README.md` (ours, opening with the fork statement), and a `//!` provenance block at the top of `src/lib.rs`. **Apache-2.0 §4(b):** every file we modify carries `// OXITEXT MODIFICATION (oxitext 0.2.2): <one line>. See ../PROVENANCE.md.` Files we do not modify stay **byte-identical** to upstream. | 3/3 lenses. `PROVENANCE.md` must state: upstream crate `swash`, version `0.2.10` (crates.io, 2026-07-17), repo `https://github.com/dfrg/swash`, upstream commit `7773843` "Bump version number to 0.2.10 (#132)", author Chad Brokaw, licence `Apache-2.0 OR MIT`, fork date, the reason (issue #93 open 15 months; upstream issue #107 records swash as dormant), a **per-file divergence table with file:line**, the inherited-`unsafe` inventory (D14), the known-unfixed upstream defects (#105, #123-#126, #133), and the 2000-line exemption (D12). `diff -r crates/oxitext-swash/src $REG/src` was to stay a readable audit; **AMENDED by the D12/D16 user election (2026-08-05)**: house style supersedes byte-identity, so the diff will grow — the per-file §4(b) modification headers and the `PROVENANCE.md` divergence table remain MANDATORY and must be updated to cover every S9 change, because they are now the audit. |
| **D12** | **USER ELECTION 2026-08-05 (「COOLJAPAN 流儀にしてしまって構わない — 2000行ルール」): the exemption is REVOKED. `crates/oxitext-swash` obeys the 2000-line rule like every other crate.** The one offender, `src/text/unicode_data.rs` (**5 491**), is split into `< 2000`-line submodules under `src/text/unicode_data/` (prefer `splitrs`; a manual module split of the 5 const tables is acceptable with the reason recorded — they are generated data with no logic). Public paths (`super::unicode_data::…`) must stay unchanged so no consumer edits follow. Runners-up `internal/aat.rs` (1 830) and `shape/at.rs` (1 826) are already under and stay whole. The `PROVENANCE.md`/`CONTRIBUTING.md` exemption records written in S7 are REPLACED by the split record (file → submodule table). | The user owns the fork decision: the rebase-path argument the exemption rested on is void — upstream is dormant (issue #107) and the code is ours now. House style wins. |
| **D13** | **Convert all 13 `.unwrap()` sites; no blanket `#[allow]`.** After D5 removes `shape/buffer.rs:530`, 12 remain: `cache.rs:13,64`, `feature/at.rs:260`, `metrics.rs:197`, `shape/at.rs:473`, `shape/mod.rs:602,620,817,831,835,847`, `shape/partition.rs:214`. Six are the identical `self.store.unwrap()` idiom → `let Some(s) = self.store else { return; }`. Zero `.expect()`, zero `panic!`, zero `assert!` in the crate. | 3/3 lenses agree on "convert all". `CONTRIBUTING.md` (*[judge]*) forbids `.unwrap()`/`.expect()`/`panic!()`/`assert!()` **on data derived from untrusted input** outside test code — a font parser is exactly that. 13 sites in 22 748 SLoC is small enough for the rule to apply literally. Gate the result with a crate-level `#![deny(clippy::unwrap_used, clippy::expect_used)]` so it cannot regress. **`debug_assert_eq!(j, len)` in the fix is retained** and is not a CONTRIBUTING violation: the completion sweep makes the condition unreachable, and `debug_assert` is a release no-op — its job is to keep a future logic leak loud in dev/test builds. Record that ruling in `PROVENANCE.md`. |
| **D14** | **`unsafe`: quarantine and document in 0.2.2; add NO new unsafe lints.** `oxitext-swash` is the first oxitext crate without `#![forbid(unsafe_code)]` — 54 sites (37 blocks + 17 `unsafe fn`) in 8 files, essentially `read_unchecked::<T>()` over untrusted font bytes. Ship: a crate-level doc paragraph naming it as inherited upstream code with a de-unsafing backlog, the `PROVENANCE.md` inventory, and the D15 fuzz target. **Do not** add `#![warn(clippy::undocumented_unsafe_blocks)]` (37 blocks × `-D warnings` = instant red gate) or `#![deny(unsafe_op_in_unsafe_fn)]` (unmeasured; would spread mechanical edits across `internal/parse.rs` and make the rebase harder for zero 0.2.2 benefit). Both, plus de-unsafing `internal/parse.rs`, are 0.2.3. | Integration lens verified `#![forbid(unsafe_code)]` on all 7 existing crates (*[judge]*: confirmed, all 7 `lib.rs`) and counted the sites; the "add lints now" half is ruled out by the `-D warnings` gate and by the minimal-delta/rebase argument that root-cause and vendoring both make. |
| **D15** | **`fuzz/fuzz_targets/shape_untrusted_font.rs` ships, but fuzzing is NOT a gate and findings are NOT triaged in 0.2.2.** Arbitrary bytes → `FontRef::from_index` → `ShapeContext` shaping of fixed Devanagari/Arabic/Latin strings under `Script::Devanagari`. | Integration lens: `fuzz/` already exists as its own workspace (`publish = false`, empty `[workspace]` table) documented as existing "only to fuzz the untrusted-input parsers in the main workspace", with `cbdt_bitmap`/`png_decode`/`sdf_atlas_from_bytes` targets. Writing the target is cheap and correct; *running* a campaign will very likely reproduce upstream's own open fuzz panics (#123-#126, #133) in the inherited parse layer, and triaging those inside the absorption would balloon it far past "nothing changed except the fixes". Any smoke-run finding goes to `TODO.md` as a 0.2.3 item. |
| **D16** | **USER ELECTION 2026-08-05 (「oxiarc に置き換えるという箇所は今回やってかまわない」): the yazi → `oxiarc-deflate` swap happens IN THIS ROUND (stage S9).** `yazi` appears in exactly 4 lines of one file (`src/scale/bitmap/png.rs:31,32,213,214` — zlib inflate of the IDAT stream of PNG-embedded CBDT/sbix strikes). The streaming-vs-one-shot API mismatch is resolved by **collecting the concatenated IDAT payload bytes and inflating once** — the zlib stream spans concatenated IDAT chunk payloads by the PNG spec, so concatenate-then-inflate is semantically exact, and memory is bounded by the embedded strike size (small by construction). `oxiarc-deflate 0.4` is ALREADY a workspace dependency (oxitext-core's PNG path uses it — reuse its idioms). `yazi` is removed from `[workspace.dependencies]` and from the vendored manifest's `scale` feature dep list; the decode path gains a deterministic regression test (in-test-synthesized or fixture PNG through the actual inflate seam, asserting decoded pixels). `deny.toml` MAY gain a `yazi` ban next to the swash ratchet (record either way). | User direction supersedes the 2/3 deferral. The mismatch argument was about schedule risk, not feasibility — and the absorption gates (S3/S4 corpus, G-battery) are already green, so the swap lands on a proven base and is gated separately in S9. |
| **D17** | **`deny.toml` gains the `{ name = "swash" }` ratchet** (with a comment tying it to dfrg/swash#93), added in S7 **after** the vendoring is proven. No other `deny.toml` change; nothing new is banned. | Vendoring lens recommends it, integration says "no change required" — both are right; the ratchet is optional protection, and cargo-deny matches on **package name**, so it cannot hit our `oxitext-swash`. Verified nothing else in either graph pulls swash: `cargo tree -i swash` in oxigis prints exactly one root (`swash 0.2.10 → oxitext-shape 0.2.1 → oxitext 0.2.1 → {oxigis-render, oxigis-ui, oxiui-text 0.2.1 → oxiui-egui 0.2.1}`). If a future third-party legitimately needs upstream swash, relax it then — with a recorded reason. |
| **D18** | **Fix upstream's broken feature matrix while we own it**, in `src/lib.rs` and 3 cfg sites. (a) `#[cfg(not(any(feature = "std", feature = "libm")))] compile_error!("oxitext-swash requires exactly one of the `std` or `libm` features");` (b) `#[cfg(feature = "libm")]` → `#[cfg(all(feature = "libm", not(feature = "std")))]` at `strike.rs:9`, `scale/mod.rs:238`, `scale/bitmap/mod.rs:7`. | Vendoring lens, measured, and (b) is **gate-mandatory**: `--all-features` emits 3 `unused import: core_maths::CoreFloat` warnings, and the oxitext test gate runs `--all-features`, so `clippy --all-targets --all-features -- -D warnings` would be red without it. `docs.rs` also builds with `all-features = true` (upstream's own `[package.metadata.docs.rs]`, kept). (a) is cheap and turns a confusing `read-fonts` compile error into a named one. |
| **D19** | **Pin `skrifa = "0.44"` (with `default-features = false`) in `[workspace.dependencies]`** and inherit it in the vendored manifest, replacing upstream's `>= 0.31.1, <= 0.44`. Same treatment for `zeno 0.3.3`, `core_maths 0.1.1` (COOLJAPAN workspace-dep rule; `CONTRIBUTING.md` mandates it); `yazi` is REMOVED entirely by the D16 user election (S9). Raising past 0.44 to 0.45.1 is **0.2.3**. | Root-cause recommends the pin; vendoring documents the range risk (13 minor versions a downstream lockfile can move under us) and that oxigis's lockfile carries **two** skrifa versions today (0.42.1, 0.44.0). 0.44 is the top of upstream's range, is what oxitext resolves today, and is what the end-to-end proof ran on. |
| **D20** | **Gate battery** (D-numbered so stages can cite it): **G1** `cargo build --workspace`; **G2** `cargo nextest run --workspace --exclude oxitext-bench --all-features` — baseline **827 passing** + 16 ignored/env-gated, and *827 must not move*; **G3** `cargo test --doc --workspace` (nextest skips doctests — D3); **G4** `cargo clippy --all-targets -- -D warnings` = 0, and again with `--all-features`; **G5** `cargo fmt --all --check`; **G6** `bash scripts/ffi-audit.sh`; **G7** `cargo deny check bans`; **G8** graph assertions: `cargo tree --manifest-path crates/oxitext-shape/Cargo.toml -e normal \| grep -E 'yazi\|zeno'` **empty**, `cargo tree -p oxitext-raster --features swash-backend -i zeno` **present**, `cargo tree -e no-dev -i swash` **empty**, and (after S9) `cargo tree -i yazi` **empty under every feature combination** — yazi is gone from the workspace graph entirely; **G9** `cargo publish --dry-run` per crate in D21 order; **G10** `cargo +1.89 check -p oxitext-swash`. | *[judge]*, verified against the tree: `CONTRIBUTING.md` mandates G1/G2(base)/G4/G5 and says "nextest is required; do not rely on `cargo test` alone"; `TODO.md:5` records the 827 figure under exactly the G2 command line; `scripts/` contains only `ffi-audit.sh`; `.github/` contains only `FUNDING.yml` (**there is no CI — every gate is local and manual**). G3 and G10 are additions this absorption forces. |
| **D21** | **Semver: 0.2.1 → 0.2.2 stays a non-breaking patch release. Publish order:** `oxitext-swash` → `oxitext-core` → {`oxitext-shape`, `oxitext-layout`, `oxitext-raster`, `oxitext-sdf`, `oxitext-icu`} → `oxitext`. (`oxitext-bench` is `publish = false`.) | 2/3 lenses, grep-verified by both: **no swash type crosses oxitext's public API** — no `pub use swash` anywhere; `oxitext-raster` re-exports only `pub use swash_backend::SwashRaster` whose public items (`new`, `with_hint`, `RasterBackend::rasterize*`) expose no swash type and whose `ScaleContext` is a private field. So the swap is invisible to `oxiui-text`/`oxiui-egui`/`oxigis`, and the behaviour change (correct rephs, no panic) is a bugfix — precisely what `## [0.2.2] - Unreleased` is for. `oxitext-swash` has **zero in-workspace dependencies**, so it is a leaf and goes first; `cargo package --list` on the renamed copy produced a clean 61-source-file listing plus both LICENSE files, no path-dep complaint. |
| **D22** | **All work on branch `0.2.2`; NOTHING is committed, pushed or published.** The tree is left dirty for the user. No version bump (the branch name carries the version). No `.github/workflows` changes. | CLAUDE.md branch/commit policy; 3/3 lenses; the task brief. |

---

## Contradictions between lenses — re-verified and ruled

| Contradiction | Ruling | Basis |
|---|---|---|
| Fix content: root-cause's `emit!`-macro patch (both functions, 4 range sites, explicit reph, completion sweep, `debug_assert`) vs integration's ~15-line completion-only patch in `reorder_complex`. | **Root-cause's, verbatim (D5).** | Superset, not conflict: integration's ascending completion sweep and root-cause's explicit-reph-then-sweep coincide on the single-base case (the reph is the low index). Root-cause additionally fixes the four length-as-range-end sites and `reorder_myanmar`, and carries the stronger evidence (440k-string stress with the assertion armed; 21-case A/B sweep). |
| Myanmar in or out. | **IN (D6).** | Root-cause's code reading is confirmed by the A/B sweep showing zero Myanmar output change; the same never-cleared buffer reaches `push_order`, where a stale index corrupts *character* order. |
| Licence `Apache-2.0` (root-cause, and the task brief) vs `Apache-2.0 OR MIT` (vendoring, integration). | Synthesis ruled dual on the member with an explicit user-override path; **the user elected the override on 2026-08-05 → `Apache-2.0` unified (D10, amended)**. | User direction 「Apache で統一で」; the recorded override recipe applied verbatim. |
| `[lib] name`: keep `swash` (root-cause's open question, vendoring implicit) vs rename + fix doctests (integration). | **Rename; 17 doctests rewritten (D3).** | Only integration measured post-rename doctests; 17 red → 17 green in 3 files. Root-cause's "17/17 pass" was measured pre-rename and is not contrary. |
| Clippy: defer the 42 warnings (root-cause) vs full cleanup with 1 allow (vendoring) vs mechanical fixes keeping all 5 upstream allows (integration). | **Full cleanup, 2 documented allows (D9).** | `CONTRIBUTING.md`'s zero-warning gate rules out deferral; vendoring's measurement (float_cmp and many_single_char_names fire **zero** times) rules out keeping all five; the second allow (`large_const_arrays`) is added to keep the two generated tables byte-identical, closing vendoring's own recorded risk. |
| yazi swap in 0.2.2 (root-cause stage 3) vs 0.2.3/stage 5 (vendoring, integration). | **0.2.3, out of this plan (D16).** | 2/3 plus the measured API-shape mismatch (streaming vs one-shot inflate) and the fact that D4 removes yazi from the default graph anyway. |
| Warning counts: 35 sites / 20 kinds (vendoring, `--features scale,render`) vs 40 default / 43 all-features (integration). | **Both stand; neither is load-bearing.** The gate is "zero after the S6 pass", measured on the toolchain in use at implementation time. | Different flag sets (`--all-targets`/`--tests` inclusion) and different clippy builds; the two agree on composition (div_ceil/is_multiple_of dominant, then large_const_arrays, derivable_impls, unused_enumerate_index). |
| SLoC: 22 748 code / 25 101 lines (vendoring, `tokei`) vs 26 528 raw lines (integration, `wc -l`). | **Cite `tokei`: 61 files, 22 748 code SLoC.** The `wc -l` figure counts differently and is not used for any decision. | Neither number changes a ruling; both agree on the file count and on the single >2000-line file. |
| `MAX_CLUSTER_SIZE` "coincidentally equal to 64" (root-cause's open question). | **False as stated — corrected *[judge]*.** `MAX_CLUSTER_SIZE = 32` (`src/text/cluster/cluster.rs:11`); `reorder_complex` uses `[false; 64]` for both `ignored` and the new `placed`, matched by `shape/mod.rs:657`'s `.min(64)` clamp; `reorder_myanmar` uses `[false; MAX_CLUSTER_SIZE]` for both, matching its own `ignored` and its `chars` bound. The patch is internally consistent in both functions. | Read directly from `$REG/src`. The real coupling is `reorder_complex`'s literal `64` ↔ `mod.rs:657`'s `.min(64)`, and it is a latent trap for whoever tackles upstream #105 — record it in `PROVENANCE.md`, do not "unify" the constants in 0.2.2. |

---

## The defect and the fix (carried from the root-cause lens)

**One bug, two symptoms.** `swash::shape::buffer::reorder_complex` builds a permutation into
`order`, a scratch `Vec<usize>` owned by `shape::mod::State` (declared `shape/mod.rs:337`,
`Vec::new()` at `:349`) that `State::reset()` (`:357-361`) **never clears** — it clears only
`buffer`, `features`, `disable_kern`. `reorder_complex` grows it with `order.resize(len, 0)`
(grow-only) and slices `&mut order[..len]`. On many ordinary Devanagari syllables the fill loop
terminates with **`j < len`**, so the tail of `order[..len]` holds indices written by a *previous
cluster*. Then, at `buffer.rs:679-681`:

```rust
buf.copy_from_slice(glyphs);
for (i, j) in order.iter().enumerate() {
    glyphs[i] = buf[*j];        // <-- buffer.rs:680
}
```

- stale index `< len` → a glyph is **duplicated** and the intended one **dropped** → **defect (a)**, the reph replaced by a copy of a neighbour;
- stale index `>= len` → **`index out of bounds`** at exactly `buffer.rs:680` → **defect (b)**.

`मार्ग` is correct on a *fresh* shaper only by accident: `Vec::resize(len, 0)` fills the hole
with `0`, which for `[Reph, Halant, Base]` happens to be the reph's own index. `स्वर्ग`'s first
cluster leaves `order == [0,1,2]`, so the second cluster's hole inherits `2` (the base) instead.
Instrumented trace, pristine 0.2.10 + Nirmala face 0, `Script::Devanagari`, `Ltr`:

```
### मार्ग   [reorder] len=3 j=2 order=[2, 1, 0]  classes=[Reph, Halant, Base]  -> [273, 301, 250, 330]  (accidentally right)
### स्वर्ग  [reorder] len=3 j=2 order=[2, 1, 2]  classes=[Reph, Halant, Base]  -> [738, 250, 250]       (reph 330 lost, ga 250 duplicated)
```

**Why `j < len` — three independent defects:**

1. **The dropped reph (dominant).** `buffer.rs:578-583` marks the reph `ignored[i] = true`. It is re-emitted only at `:632` (`if last_base.is_none()`) or at `:651` (`if Some(i) == last_base`, inside the sweep). But `last_base` is assigned only at a `Base` glyph, and the **first** base is itself marked `ignored` as `first_base` (`:569`). For any single-base syllable — `र्ग र्क र्य र्व र्ष र्ण`, the whole reph vocabulary — `first_base == last_base`, the sweep `continue`s past it, the hook never fires, and the reph is **silently dropped**. A permanent property, not a race.
2. **A length used as an exclusive range end**, at four sites: `buffer.rs:592` (VPre), `:599` (VMPre), `:473` (Myanmar Anusvara), `:495` (Myanmar VPre), all `Some(r) => Some(r.start..i - r.start + 1)`. Right by coincidence when `r.start == 0`; for `r.start = 2, i = 3` it yields the empty range `2..2` — two glyphs marked `ignored`, zero emitted, two stale slots. Correct form: `r.start..i + 1`.
3. **No invariant.** Nothing asserted that `order[..len]` is a permutation of `0..len`.

**The patch** (7 hunks in `src/shape/buffer.rs`, nothing else in the crate):

1. **Range end is an index, not a length** at all four sites, each with an explanatory comment.
2. **A placement-tracking emitter** in both functions: every `order[j] = i; j += 1;` becomes `emit!(i)`, declared once per function immediately before the first emission —
   ```rust
   let mut placed = [false; 64];          // reorder_myanmar: [false; MAX_CLUSTER_SIZE]
   macro_rules! emit {
       ($index:expr) => {{
           let index = $index;
           if index < len && !placed[index] && j < len {
               placed[index] = true;
               order[j] = index;
               j += 1;
           }
       }};
   }
   ```
   Three guards, three jobs: `index < len` kills OOB writes; `!placed[index]` kills duplication (this is what makes the now-possibly-overlapping VPre ranges from (1) safe); `j < len` kills overflow of `order[..len]`. A `macro_rules!` rather than a closure because the body mutates three locals the surrounding code also reads — it costs nothing at runtime and keeps the original control flow line-for-line reviewable against upstream.
3. **Close the hole, then assert the invariant**: an explicit `if let Some(i) = reph { emit!(i); }` (the *correctness* fix — it names the single-base case and places the glyph deliberately, at the end of the syllable, which is where swash's accidentally-correct path put it), then `for i in 0..len { emit!(i); }` (the *safety* fix — a leaked index degrades to an identity fallback instead of a wasm-fatal panic or a silent duplication), then `debug_assert_eq!(j, len, "reorder_complex must produce a full permutation")`. `reorder_myanmar` gets the same treatment plus a trailing `anus.take()` drain, and loses its `anus.take().unwrap()` to an `if let` on the way.

**Rejected alternatives, both empirically:** clearing `order` in `State::reset()` — insufficient, because the staleness is cluster-to-cluster *inside a single `add_str`* (`स्वर्ग` is wrong on a brand-new `ShapeContext`), and it leaves the reph genuinely dropped. Seeding `order` with the identity permutation — kills the panic and makes output deterministic but **breaks the fresh-shaper baseline** (`मार्ग` → `[273,301,250,250]`); determinism is not correctness. Bounds-checking line 680 alone — stops the panic, cements defect (a).

### Proof already on record (do not re-run; re-measure only what D8 requires)

| Measurement | Pristine 0.2.10 | Patched |
|---|---|---|
| 24-word Hindi corpus, one reused shaper (Nirmala) | **4 panics** (`पूर्ण`, `वर्तमान`, `आदर्श`, `संघर्ष`) + silent reph loss in `सूर्य`, `पूर्व`, `वर्षा`, `कार्यक्रम` | **0 panics**, every reph-bearing word recovers gid 330 |
| Same corpus (Noto Sans Devanagari) | same 4 panics; fresh ≠ reused across the 24 | 0 panics; fresh **==** reused for all 24 |
| `बर्नार्ड` (upstream issue #93's reproducer) | PANIC `buffer.rs:680` | `[271, 267, 301, 330, 260, 330]` |
| A/B sweep: 21 font×script cases, ~90 strings, fresh **and** reused, comparing glyph id + x/y + advance | — | **6 changed lines total, every one a fix**: 4 Devanagari gain reph 330, Bengali `র্ক` `885 885`→`885 954`, Oriya `ର୍କ` `1201 1201`→`1201 1258`. **Zero** change to Latin (incl. kerning and `fi/fl/ffi/ffl`), Cyrillic, Greek, Arabic, Hebrew, Thai, Japanese, Han, Hangul, **Myanmar**, Tamil, Telugu, Gujarati, Gurmukhi, Malayalam, Kannada, Sinhala, Javanese |
| Randomized stress, 440 000 strings, 1-24 chars, 14 script/font pairs, shared reused `ShapeContext`, dev profile (assertion armed) | **12 panics** | **0 panics, 0 `debug_assert_eq!` failures** |
| oxigis canary transcribed verbatim through `oxitext-shape`'s real public API, assertions inverted | — | **6/6 PASS** |
| Whole oxitext workspace copy, vendored+renamed+fixed, default features | — | **797 passed / 0 failed / 20 ignored**, zero oxitext-owned source edits |

**Structural argument that the blast radius is bounded:** `reorder_complex` is reached only from
the `EngineMode::Complex` arm of `Shaper::add_cluster` (`shape/mod.rs:659`), and mode selection is
`if gsub.lang != 0 && script.is_complex() { Myanmar or Complex } else { Simple }`
(`shape/engine.rs:57-65`). Latin, Arabic, Hebrew, CJK and Hangul — everything OxiText and OxiGIS
ship on screen and in PDF today — take `EngineMode::Simple` and never enter either changed
function. The A/B sweep is the empirical half of the same claim.

**Credit obligation.** The fix is original, but **dfrg/swash#93 "Panic While Shaping" (open since
2025-04-20)** is the prior public report of defect (b) — same function, same line
(`swash-0.2.2/src/shape/buffer.rs:680:21`), same message shape, reported via parley/vello_editor
on `बर्नार्ड`. It must be cited in `CHANGELOG.md`, in `PROVENANCE.md`, and in any future upstream PR.

---

## Verified facts the implementation depends on

**Upstream.** swash **0.2.10** (2026-07-17) is the newest release on crates.io — no 0.2.11 exists.
`dfrg/swash` HEAD **is** that release commit (`7773843`). `src/shape/buffer.rs` has had no
substantive change since 2021 (`28d5604` 2025-02 feature caching, `f37dd86` 2024-09 no_std,
`73aa8df`/`53e698f` 2024-10 clippy, then `614ca27`/`6acf85d` 2021-07, `8dc25d8` initial). Issue
**#93** open 15 months, no fix, no PR, no assignee. Issue **#107 "Migrating to harfrust"** (open
2025-08-17) records swash as dormant — the author's time is on fontations/HarfRust. Defect (a)
has no upstream issue (GitHub issue search for "devanagari" over dfrg/swash: `total_count 0`).
Open PRs #135 (embolden winding) and #130 (`Bytes::check_range` overflow) touch neither defect.
**There is nothing to ride.**

**The vendored crate.** 61 Rust files, **22 748 code SLoC** (`tokei`), which roughly doubles the
oxitext workspace's Rust code (104 files / 26 863 SLoC today). `license = "Apache-2.0 OR MIT"`,
`authors = ["Chad Brokaw <cbrokaw@gmail.com>"]`, both LICENSE files in the `.crate`. Deps:
`skrifa >= 0.31.1, <= 0.44` (default-features off), `yazi 0.2.1` (opt, `scale`), `zeno 0.3.3`
(opt), `core_maths 0.1.1` (opt, `libm`) — all pure Rust, no `-sys`, no `build.rs`, no C.
Features `default = ["std","scale","render"]`, `std`, `scale`, `render`, `libm`. Manifest also
carries `[lints.clippy] doc_markdown = "warn"`, `semicolon_if_nothing_returned = "warn"` and
`[package.metadata.docs.rs] all-features = true` — keep all three (*[judge]*, read from `$REG`).
Pristine swash is **already rustfmt-clean** at edition 2021 (`cargo fmt --check`: 0 diffs), which
is exactly what `rustfmt.toml` pins — **no repo-wide reformat**; keep the existing targeted
`#[rustfmt::skip]` on `JOIN_STATES`. Files to delete from the copy: `.cargo-ok`,
`.cargo_vcs_info.json`, `Cargo.lock`, `Cargo.toml.orig`, `.gitignore`, `.github/`, `.typos.toml`,
and `.clippy.toml` (folded per D9b). Upstream's `README.md` is **not** vendored — ours replaces it.

**The consumers (complete surface; grep-verified by two lenses).**
`oxitext-shape/src/lib.rs`: `swash::shape::{Direction, ShapeContext}` (`:90`), `swash::FontRef`
(`:91`, `from_index`), `swash::tag_from_bytes` (`:576`), `swash::text::{Language, Script}`
(`:577`, `Script::from_opentype`, `Language::parse`), the builder chain
`.size/.direction/.script/.language/.features/.variations/.build`, `Shaper::{add_str, shape_with}`,
and `cluster.source.{start,end}` / `cluster.glyphs` / `glyph.{id,advance,x,y,info.is_mark()}`;
`ShapeFeature` is passed as `&([u8;4], u16)` relying on `From<&([u8;4], T)> for Setting<T>`.
`oxitext-raster/src/swash_backend.rs`: `swash::scale::{Render, ScaleContext, Source, StrikeWith}`
(`:15`), **`swash::zeno::Format::Alpha`** (`:116`), `swash::scale::image::Content::Color` (`:179`).
**The `pub use zeno` re-export (`src/lib.rs:35`, `#[cfg(feature = "scale")]`) is a hard constraint
— the vendored crate must keep it.**

**Existing coverage this must not disturb.** `oxitext-shape` 73 lib tests + `tests/alt_backend.rs`
(notably `variational.rs` `test_devanagari_conjunct_swash_no_panic` and the `wght=700 vs 400`
advance assertions at `:191-224`; `tests_inline.rs` cluster monotonicity `:109` and the two
`swash_backend_*_arc` cache-key tests `:948`, `:973`); `oxitext-raster` 154 lib tests + 6
integration files. Note what is **not** covered today: oxitext has **no Devanagari test against a
real Devanagari font** — `test_devanagari_conjunct_swash_no_panic` runs `क्ष` through the **Latin**
`tests/fixtures/test-font.ttf` and accepts "Ok or Err". The corpus below is genuinely new coverage.

---

## Staged plan (branch `0.2.2`, nothing committed)

### S0 — preflight, no edits
Confirm branch `0.2.2`, `git status --porcelain` empty. Run the **full D20 battery on the
untouched tree** and record every number in the scratch notes: G2's exact passing count (expected
827 + 16 ignored/env-gated), G3's doctest count, G4/G5/G6/G7 clean. This baseline is what "827
must not move" is measured against; without it, a later regression is unattributable.
**Gate:** all of G1-G7 green on the pristine tree, numbers recorded.

### S1 — vendor verbatim and wire it in (zero behaviour change)
1. `crates/oxitext-swash/src/` ← byte copy of `$REG/src/` (use `$SCRATCH\swash-probe\swash-orig\src` if the registry copy is unavailable — they are identical). `LICENSE-APACHE`, `LICENSE-MIT` ← verbatim from `$REG`. Delete the files listed above.
2. Write `crates/oxitext-swash/Cargo.toml`: `name = "oxitext-swash"`; `version/edition/rust-version/authors/repository` = `*.workspace = true`; `license.workspace = true` (= Apache-2.0; user election, D10); `description = "Vendored fork of swash 0.2.10 (dfrg/swash) with Indic reordering fixes, for OxiText"`; upstream `[features]` verbatim; `[dependencies]` inherited from the workspace (D19); keep `[lints.clippy]` and `[package.metadata.docs.rs]`. No `[lib]` section (D3).
3. Root `Cargo.toml`: add `"crates/oxitext-swash"` to `members` (first — it is a leaf); delete `swash = { version = "0.2.9" }`; add the D2 alias entry with the comment explaining that `default-features = false` here is what keeps `yazi`/`zeno` out of the default graph, and that any future crate reaching for `swash::scale::*` must add `features = ["scale","render"]` to its own line or get a confusing "no module `scale`" error. Add `skrifa`/`yazi`/`zeno`/`core_maths` to `[workspace.dependencies]` (D19).
4. `crates/oxitext-raster/Cargo.toml:52` → `swash = { workspace = true, optional = true, features = ["scale", "render"] }`. `crates/oxitext-shape/Cargo.toml:24` **unchanged**.
5. Fold `doc-valid-idents` into the workspace `clippy.toml` (D9b).

**Gate:** G1, G2, G8. G2 must be green **with zero source changes in any oxitext-owned crate** —
this is the checkpoint that proves the alias works before any fix lands. G4/G5 are *not* yet
expected clean (S6 owns them).

### S2 — doctest rename
`use swash::` → `use oxitext_swash::` in `src/font.rs`, `src/shape/mod.rs`, `src/scale/mod.rs`
(17 doctests). Each file gets the §4(b) modification header (D11).
**Gate:** G3 → `cargo test -p oxitext-swash --doc` = 17 passed / 0 failed.

### S3 — fixture and regression corpus, RED against the unfixed vendored crate
1. `tests/fixtures/NotoSansDevanagari-Regular.ttf` (D7) + the `tests/fixtures/README.md` table row + regeneration recipe + an `OFL.txt` for the face's licence text.
2. `crates/oxitext-shape/tests/devanagari_reorder.rs`, the six cases in the test plan below, written to assert the **fixed** expectations.
3. Run them. Record the **observed pre-fix** output of every case verbatim in the commit-message draft / `PROVENANCE.md` working notes — that is the red-before evidence, and it is the only chance to capture it.

**Gate:** the new tests are RED for the documented reason (wrong gid / panic), and **nothing else
moved**: G2's 827 unchanged.

### S4 — the fix
Copy `$SCRATCH\swash-probe\swash\src\shape\buffer.rs` over
`crates/oxitext-swash/src/shape/buffer.rs` (D5 — byte copy, do not retype), then prepend the
§4(b) modification header naming the mechanism (`order` is `ShapeContext` scratch, only grown,
`j < len` leaves a stale tail), both symptoms, and **dfrg/swash#93**. Add the `PROVENANCE.md`
divergence rows. Add the `buffer.rs` unit tests (permutation invariant; see test plan).
**Re-measure every glyph-id golden against the Noto fixture (D8)** and reconcile with the expected
values; any deviation is investigated as a VPre/VMPre range-fix effect and written up, not
silently accepted.

**Gate:** the S3 corpus GREEN; the new unit tests green; **full D20 battery G1-G8**; G2's 827
unchanged plus the new tests. The tree state at the end of S4 is the **upstream-offerable diff**
(fix only, no conformance churn) — note that in `PROVENANCE.md` so a later PR, if the user
authorizes one, can be cut from it.

### S5 — feature-matrix hygiene (D18)
`compile_error!` guard in `src/lib.rs`; three `cfg(all(feature = "libm", not(feature = "std")))`
corrections. §4(b) headers on all four files.
**Gate:** all six combos build — `{--no-default-features --features std}`,
`{--no-default-features --features libm}`, `{--no-default-features --features libm,scale,render}`,
`{default}`, `{--features scale,render}`, `{--all-features}` — and bare `--no-default-features`
fails with **our** `compile_error!` message. `--all-features` clippy is warning-free. G10
(`cargo +1.89 check -p oxitext-swash`).

### S6 — COOLJAPAN conformance (D9, D13)
1. `cargo clippy --fix` at `--features scale,render`, then `--no-default-features --features std`, then `--features libm,scale,render` (three passes to reach cfg'd code); then `cargo fmt -p oxitext-swash`.
2. Hand-fix the 3 known residuals (`scale/mod.rs:556 unused_mut`, `scale/bitmap/mod.rs:53`, `shape/at.rs:915`).
3. Prune `src/lib.rs`'s allows to the two of D9; add `#![allow(clippy::large_const_arrays)]` with its rationale comment and **revert any `const`→`static` rewrite clippy applied to `text/unicode_data.rs` / `text/lang_data.rs`** so both stay byte-identical to upstream.
4. Convert the 12 remaining `.unwrap()` sites (D13); add `#![deny(clippy::unwrap_used, clippy::expect_used)]`.
5. §4(b) header on every file touched in this stage; `PROVENANCE.md` divergence table brought fully up to date; `diff -r crates/oxitext-swash/src $REG/src` re-run and its file list pasted into `PROVENANCE.md` as the audit appendix.

**Gate:** full D20 battery G1-G8, with G4 = **zero** warnings at both default and `--all-features`.
G2's 827 + new tests unchanged.

### S7 — provenance, licence, docs, deny, fuzz
`NOTICE` (Apache-2.0 §4(d) shape: *"This product includes software developed by Chad Brokaw
(swash, https://github.com/dfrg/swash)"* + the COOLJAPAN modification line); `PROVENANCE.md`
finalised (D11); `crates/oxitext-swash/README.md`; `src/lib.rs` `//!` provenance block; root
`README.md` §License gains a third-party paragraph naming swash/Chad Brokaw and pointing at
`crates/oxitext-swash/LICENSE-*`; root `CHANGELOG.md` `## [0.2.2] - Unreleased` gains an
`### Added` entry for the crate (credit + dfrg/swash#93 link + the `cargo tree` graph assertions,
in the style the existing 0.2.2 entries already use) and `### Fixed` entries for both defects;
`CONTRIBUTING.md` gains the vendored-code rules (2000-line exemption scoped to vendored third-party
code, the review rule that unmodified files stay byte-identical, the §4(b) header requirement);
`SECURITY.md` gains a line that `crates/oxitext-swash` is vendored third-party code carrying
inherited `unsafe`; `TODO.md` gains the status line and the 0.2.3 backlog (yazi→oxiarc-deflate,
skrifa 0.45.1, de-unsafing, unsafe lints, upstream PRs #130/#135, issue #133); `deny.toml` gains
the `{ name = "swash" }` ratchet (D17); `fuzz/fuzz_targets/shape_untrusted_font.rs` + its
`fuzz/Cargo.toml` target entry (D15 — written, not run as a gate).
**Gate:** G7 (`cargo deny check bans`) passes; G6 (`ffi-audit.sh`) passes; full battery re-run.

### S8 — publish rehearsal only
`cargo publish --dry-run` per crate in the D21 order. **No publish, no commit, no push.**
**Gate:** G9 dry-run clean for every member; `git status` shows exactly the intended dirty set and
nothing else.

---

## Test plan (shipped, not scratch)

**`crates/oxitext-shape/tests/devanagari_reorder.rs`** — the corpus, over the D7 fixture, skipping
gracefully when it is absent. The 24-word `HINDI_CORPUS` is transcribed from the oxigis canary
(`crates/oxigis-ui/src/print/shape.rs:631-655`, read-only):

```
कर्म धर्म वर्ष मार्ग सूर्य पूर्व कार्य दर्शन पर्वत सर्व गर्व अर्थ
स्वर्ग निर्माण पूर्ण वर्तमान वर्षा आदर्श संघर्ष उत्तर दिल्ली हिन्दी भारत कि
```

| # | Test | Expected (Noto, **re-measure per D8**) | Unfixed |
|---|---|---|---|
| 1 | `reph_survives_a_fresh_shaper` — `स्वर्ग` | `[256, 84, 58, 506]` | `[256, 84, 58, 58]` — fails on a **fresh** shaper, so it needs no state setup: the strongest case |
| 2 | `reph_survives_two_words_in_one_call` — `"दिल्ली मार्ग"` | ends `…, 58, 506` | ends `…, 58, 58` |
| 3 | `four_word_phrase_does_not_panic` — `"सूर्य पूर्व वर्षा मार्ग"` | 19 glyphs, a reph in each word | **panics** |
| 4 | `reused_shaper_matches_fresh_shaper_over_the_corpus` — all 24 words, one shaper vs one per word | **equal** | disagree |
| 5 | `corpus_shapes_without_panicking` — 24 words through ONE shaper | 0 panics | 4 panic (`पूर्ण`, `वर्तमान`, `आदर्श`, `संघर्ष`) |
| 6 | `latin_arabic_cjk_are_byte_identical` — golden over the existing Simple-mode paths | unchanged | unchanged (asserts the D12-shaped no-behaviour-change claim) |

**Measured negative — do not write this test:** the oxigis canary's `दिल्ली` → `मार्ग` **two-call**
pair does **not** reproduce on Noto (the reused shaper gives the correct `[80, 31, 58, 506]`); the
stale index depends on the previous cluster's length, which is font-specific. It reproduces on
Nirmala only, and it stays where it is — a local Nirmala probe in OxiGIS. Use the **one-call**
phrase (case 2).

**Font-independent assertions (preferred, because they survive a fixture update):** for every
reph-bearing corpus word assert (i) no panic, (ii) the reph gid appears **exactly once**, (iii) **no
adjacent duplicate gid** — the duplicate-adjacency check is what would have caught defect (a) on
day one. Cases 4 and 5 are pure properties and need no reference implementation at all.

**`crates/oxitext-swash/src/shape/buffer.rs` unit tests** (font-independent, survive any font
update): `reorder_complex` and `reorder_myanmar` emit a **complete permutation of `0..len`** for
synthesised `[GlyphData]` class sequences — including `[Reph, Halant, Base]` (the single-base
case), a two-VPre cluster (the range bug, otherwise unreachable from text), and a **deliberately
pre-poisoned `order`** seeded with out-of-range indices, proving staleness can no longer escape.
Plus: a `ShapeContext` pre-loaded with a longer `order` still produces the fresh result.

**Non-Devanagari Complex smoke coverage (recommended, cheap):** `reorder_complex` serves every
`EngineMode::Complex` script. Add fresh-==-reused + no-panic cases for at least Bengali and Tamil
(no glyph-id goldens needed — the purity property needs no reference implementation). If no
redistributable fixture is added for them, record the gap in `TODO.md` rather than pretending the
coverage exists. The A/B sweep already shows Bengali `র্ক` and Oriya `ର୍କ` recovering their reph.

**Feature-matrix sweep** (S5's gate, scripted) and **`cargo test --doc`** (S2's gate) are tests in
their own right — neither is reachable through `cargo nextest`.

---

## Explicit OUT rulings

| Out of scope | Reason |
|---|---|
| **Upstream issue #105** (clusters exceeding `MAX_CLUSTER_SIZE` producing overlapping source ranges) | Real, open, adjacent — `add_cluster` clamps `reorder_complex` to `.min(64)` glyphs (`shape/mod.rs:657`), so a >64-glyph cluster leaves its tail unreordered — but it is a **parser/cluster-size defect with a different mechanism and a different fix**, it causes neither reported defect, and folding it in would make the shaping fix unreviewable. 3/3 lenses agree. File it in `PROVENANCE.md` as known-unfixed, together with the `64` ↔ `.min(64)` ↔ `MAX_CLUSTER_SIZE = 32` constant coupling note. |
| **yazi → `oxiarc-deflate` swap** | 0.2.3 (D16). Not banned, not impure, and after D4 not in the default graph. The streaming-vs-one-shot API mismatch makes it a real piece of work, and it touches the `scale` feature `oxitext-raster` consumes. |
| **Splitting any vendored file with `splitrs`** | Permanently out (D12). One-way door; destroys the rebase path for zero readability gain on generated tables. |
| **Rewriting the 54 inherited `unsafe` sites, and adding `unsafe_op_in_unsafe_fn` / `undocumented_unsafe_blocks`** | 0.2.3 (D14). The lints would either break the `-D warnings` gate immediately or spread mechanical edits across the parser and make the rebase harder for no 0.2.2 benefit. Flagged in `PROVENANCE.md` and `SECURITY.md`, not silently ignored. |
| **Raising `skrifa` past swash's `<= 0.44` ceiling to 0.45.1** (and collapsing the 0.42.1/0.44.0 duplicate in oxigis's lockfile) | A genuine benefit the fork unlocks, but a dependency bump with its own blast radius. 0.2.3. |
| **Making `skrifa` optional for std-only shape builds** | Its only non-`scale` use is `internal/var.rs:236` (`avar`, inside `adjust_axis`), which `shape_with_variations` needs; removing it would silently change variable-font shaping. No current consumer needs the saving. TODO item. |
| **Running a fuzz campaign / triaging its findings** | D15. The target ships; the campaign is 0.2.3. |
| **Posting anything to dfrg/swash** — the fix PR against #93, a comment confirming the reproducer, or the `--all-features` dead-import fix | Requires explicit user approval (`docs/upstream-reports.md` issues 17-18 in oxigis are marked DRAFTS-do-not-post, and that embargo plausibly extends to a PR). S4's tree state is preserved as the PR-able diff so the option stays open at zero cost. |
| **Any modification to `I:\rust\oxigis`** | Read-only for this work. The downstream section below is a record its own future commit executes. |
| **Any commit, push, publish, or version bump** | D22. |

---

## DOWNSTREAM — record only, no oxigis work in this plan

**How oxigis consumes.** swash reaches OxiGIS through exactly one root: `oxitext = "0.2.1"` and
`oxitext-raster = "0.2.1"` (root `Cargo.toml:132,140`, used by `oxigis-render` and `oxigis-ui`),
plus transitively `oxiui-egui 0.2.1 → oxiui-text 0.2.1 → oxitext 0.2.1`. Both root
`Cargo.toml:139` and `crates/oxigis-ui/Cargo.toml:64` state a **hard invariant: `cargo tree -d |
grep oxitext` must stay empty.**

- **Route A (recommended) — wait for `oxitext 0.2.2` on crates.io**, then bump both direct deps. Cargo unifies with `oxiui-text`'s `^0.2.1` requirement: one oxitext, invariant intact, `deny.toml` untouched, `[sources] unknown-git = "deny"` untouched, wasm bundle reproducible from crates.io alone. **`oxiui-text 0.2.1` needs no republish** — this is the whole duplication consequence, and it resolves itself.
- **Route B (only if OxiGIS cannot wait) — `[patch.crates-io]`, never a plain git dep.** A git dep would leave `oxiui-text`'s registry edge at 0.2.1 (git and registry sources never unify, even at equal versions), giving the map two shapers with different Indic behaviour drawing the same scene — a boundary no type rule can police, unlike the precedented `oxifont-subset` case. A `[patch]` rewrites `oxiui-text`'s edge too and is the only git-based route that preserves the invariant; it must patch **all five** (`oxitext`, `-core`, `-shape`, `-layout`, `-raster`), needs an `allow-git` entry beside the existing oxifont one, and needs the identical **TEMPORARY** comment block deleted in the same commit that returns to the registry version. A patch is invisible in `cargo tree` unless you read the source column — the comment must say so.

**The canary flip protocol.** `crates/oxigis-ui/src/print/shape.rs:707` —
`swash_still_garbles_and_panics_on_indic_a_canary_not_a_complaint` — is `#[ignore]`d, reads
`C:/Windows/Fonts/Nirmala.ttc`, and **asserts the defects exist**; its own comment sets the rule:
*"If it FAILS, swash fixed them and item 1 reopens."* **Whoever lands the bump must be told the
red is the success signal**, or they will "fix" the canary by re-pinning it to the broken values.
Invert it in the **same commit** as the version bump, never before. Keep `HINDI_CORPUS`,
`IndicOutcome`, `shape_devanagari`, `catch_unwind` and the silenced panic hook verbatim — the
harness is the asset, and it now proves absence. Rename to `swash_no_longer_garbles_or_panics_on_indic`,
keep `#[ignore]` with the reason string reduced to "reads C:/Windows/Fonts/Nirmala.ttc", and
invert each assertion: fresh `मार्ग` `[273,301,250,330]` **unchanged** (it was always the correct
baseline); reused `मार्ग` becomes **equal** to it (`assert_ne!` → `assert_eq!`); one-call
`"दिल्ली मार्ग"` tail becomes `…, 250, 330`; the four-word phrase becomes `IndicOutcome::Glyphs(_)`
and the `panic!("… — fixed?")` arm becomes the failure arm; corpus panics `4` → **`0`**; `स्वर्ग`
fresh → `[738, 250, 330]`.

**Reopened items (v1.6 candidates in the oxigis TODO — NOT stages here).**

- **LTR complex-script itemisation.** Print v1.4 item 1 was deferred *solely* on these two defects. `print/shape.rs::has_complex_ltr` currently feeds only an aggregated log and `print/font.rs:786-800` emits the `tracing::warn!` saying Indic prints under the Latin tag. Itemisation means `shape_request(Ltr, <indic tag>)` actually ships and the warn shrinks to the residual cases. **The honesty log must keep warning about Myanmar** — `reorder_myanmar`'s staleness is fixed (D6) but Myanmar shaping correctness is otherwise unvalidated (the 6 A/B strings were byte-identical, i.e. unchanged, not verified-correct) — and about Khmer and Thai.
- **Screen-side Indic parity.** `oxigis-render/src/label/engine.rs` goes through `oxitext::Pipeline`, which takes no script tag. This is a real multi-crate feature (per-run script tags, label-cache keys extended by script), not a flip, and it must not be bundled with the print change: doing so makes a one-line-per-assertion canary flip hostage to a five-crate render change. Accept for one milestone that the PDF gets correct conjuncts while the map still shows logical-order Devanagari.
- **`docs/upstream-reports.md` issues 17-18** get a resolution line: *fixed in oxitext 0.2.2's vendored `oxitext-swash`; upstream swash 0.2.10 and dfrg/swash HEAD remain affected; upstream issue #93 is the same defect.* Whether to post them, or a PR, remains a user call.
- **Do not sweep in** the N'Ko `nkoo → None` latent note (`print/bidi.rs:78-80`).
- **Tripwire for the interval:** add the ignored canary to whatever `--run-ignored all` sweep OxiGIS runs, so the date it goes red (the date oxitext 0.2.2 entered the graph) is recorded rather than discovered.

---

## Risks carried forward (record; none blocks 0.2.2)

- **Reph placement is not cross-validated against HarfBuzz.** For `last_base == first_base` the fix places the reph at the END of the syllable — where swash's accidentally-correct path put it, what the oxigis canary records as the correct baseline, and what Nirmala and Noto render correctly. But dev2's spec position is `REPH_POS_BEFORE_POST`, so a single-base syllable *also* carrying post-base matras may differ. No such word appeared in the 24-word corpus or the A/B sweep. **`rustybuzz` is already an optional `oxitext-shape` dep (`rustybuzz-backend`) and is pure Rust** — cross-validating there is the cheap settlement if the question is ever raised. It does not gate 0.2.2.
- **The VPre/VMPre range fix assumes contiguity.** `r.start..i + 1` spans from the first VPre to the current one; a non-contiguous VPre run sweeps any interloper along with it. The `placed` guard makes it safe (no duplication, no OOB, invariant held across 440k stress strings), but the *typographic* correctness of moving an interloper is unverified because no natural text exercised it. Pre-fix behaviour was strictly worse (empty range, glyphs dropped).
- **The completion sweep can mask a future logic error in release builds** — deliberately, because a wasm panic aborts the whole OxiGIS map. `debug_assert_eq!` catches leaks in dev/test only, so **CI-equivalent local gates must run the shaping tests in dev profile** and the stress harness must stay armed.
- **The 22.7k vendored SLoC roughly double the workspace's Rust code.** Every future `clippy -D warnings`, `fmt --check`, `nextest` and toolchain bump carries that mass; a new lint in a future rustc can turn the workspace red for reasons unrelated to OxiText's own code. Budget the recurring tax.
- **Forking forgoes upstream fixes.** `PROVENANCE.md`'s divergence table and the `diff -r` audit are the only things that keep a future 0.2.11 (or an un-fork) tractable. If either goes stale the fork becomes unrebasable within one release. Upstream #107 means the shaper may be deleted upstream entirely — in which case this is permanently OxiText's own 22.7k-line shaper. That should be a conscious choice, not a surprise.
- **Inherited `unsafe` becomes a public commitment** the moment `oxitext-swash` is published: 54 unchecked reads over attacker-controlled font bytes, with upstream #123-#126/#133 already fuzz-found in that layer, and no upstream to escalate to.
- **Glyph-id goldens are font-version-specific.** A Noto update breaks cases 1-3 and 6 with no behavioural regression; the SHA-256 pins the file, and the property assertions (fresh == reused, no panic, no adjacent duplicate) are the half that survives.
- **MSRV 1.89 was never verified with a 1.89 toolchain** — hence G10. One clippy suggestion (`is_multiple_of`, stabilised 1.87) points at recently stabilised APIs; `div_ceil` (1.73) and `is_multiple_of` are both MSRV-safe, but the S6 `--fix` pass must not be assumed so.
- **`oxitext-swash` is unclaimed on crates.io but nothing reserves it** between now and publish.
- **(void after the D10 user election)** The member now inherits `license.workspace = true` like every other crate, so the tidy-up risk no longer exists. The load-bearing artefact is instead the `NOTICE`/`PROVENANCE.md` sentence recording that OxiText redistributes Chad Brokaw's dual-licensed work under its Apache-2.0 arm — do not drop it in a docs tidy-up.

---

## S9 — COOLJAPAN-ification (added by user election 2026-08-05, after S8 completed)

The user directed, after S0–S8 landed: 「swash を取り込んだなら、COOLJAPAN 流儀にしてしまって構わない（2000行ルール、minimum dependencies etc.)、それと、oxiarc に置き換えるという箇所は今回やってかまわない。」 Amended rulings: D9 (allow policy), D11 (audit basis), D12 (2000-line rule applies), D16 (yazi swap now), D19 (yazi unpinned), G8 (yazi absent everywhere).

* **S9a — split `text/unicode_data.rs`** (5 491 → `< 2000`-line submodules under `src/text/unicode_data/`, unchanged public paths, prefer `splitrs`, manual acceptable with recorded reason). Re-measure clippy: take `const`→`static` where `large_const_arrays` fires and drop that allow if clean.
* **S9b — yazi → `oxiarc-deflate`** in `scale/bitmap/png.rs` per amended D16 (concatenate-IDAT-then-one-shot-inflate; reuse oxitext-core's oxiarc idioms; deterministic decode regression test through the real seam; `yazi` removed from both manifests; optional deny ban recorded either way).
* **S9c — records**: PROVENANCE.md divergence table + §4(b) headers extended to every touched file; the S7 exemption records replaced per amended D12; CHANGELOG entries; TODO 0.2.3 backlog pruned of the now-done yazi item; this stage's gate = the FULL D20 battery (G1–G8, G10; G9 dry-run re-run for `oxitext-swash` only) with G2's 841 not shrinking and the S3/S4 corpus + goldens still green — the shaping fix must be provably undisturbed by the restyle.

Not in S9 (unchanged rulings): skrifa stays pinned at 0.44 (the fix was proven on it; 0.45.1 is 0.2.3), zeno/core_maths stay, D14's unsafe quarantine stays (de-unsafing is 0.2.3), fuzz campaigns stay untriaged, upstream posting still needs user approval.

## Do-not-touch and deviation rule

`MOS`/`WS`/`Win` commits are the user's — never amend, rebase or revert them. **Nothing is
committed, pushed or published by the implementation of this plan**; the tree is left dirty for
the user, and `cargo publish` appears only as `--dry-run` (S8). No push authorization exists for
oxitext.

Confine edits to: `crates/oxitext-swash/**` (new), `crates/oxitext-shape/tests/devanagari_reorder.rs`
(new), `tests/fixtures/` (the Noto face, its `OFL.txt`, the README row), `fuzz/` (the new target +
its manifest entry), and these existing files, in the ways named above only — root `Cargo.toml`,
`crates/oxitext-raster/Cargo.toml`, `clippy.toml`, `deny.toml`, `CHANGELOG.md`, `README.md`,
`CONTRIBUTING.md`, `SECURITY.md`, `TODO.md`, and this plan file. **`crates/oxitext-shape/Cargo.toml`
and every `.rs` file in every oxitext-owned crate stay unchanged** — that is D2's whole claim and
S1's gate. No `.github/workflows` changes (COOLJAPAN policy: only `pypi-publish.yml` /
`npm-publish.yml` would ever be allowed, and oxitext has neither). No version bumps — the branch
name carries the version. `I:\rust\oxigis` and `I:\rust\oxifont` are read-only. The registry
checkout under `$REG` and every artifact under `$SCRATCH` are read-only inputs; copy, never modify.

Any departure from this document is recorded **at the moment it is taken** as a dated `DEVIATION`
line in `I:\rust\oxitext\TODO.md`, with the measured reason. None is carried forward silently.
