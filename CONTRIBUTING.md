# Contributing to OxiText

Thank you for your interest in contributing. OxiText is part of the
**COOLJAPAN ecosystem** (https://github.com/cool-japan/oxitext), a
family of Pure Rust libraries that replace common C/C++/Fortran-backed
crates with FFI-free implementations. This workspace provides the
`shape → bidi → line-break → layout → rasterize` text pipeline across
the `oxitext-core`, `oxitext-shape`, `oxitext-layout`,
`oxitext-raster`, `oxitext-sdf`, `oxitext-icu`, and `oxitext` crates.

## Building and testing

```bash
# Build the workspace
cargo build --workspace

# Run the test suite (nextest is required; do not rely on `cargo test` alone)
cargo nextest run --workspace

# Lint — this MUST produce zero warnings before a change is accepted
cargo clippy --all-targets -- -D warnings

# Format
cargo fmt --all
```

## Project rules

These rules are enforced in review and, where possible, in CI:

- **Pure Rust by default.** No new C/C++/Fortran dependency, and no
  non-default C feature, may be added without an explicit, documented
  exception. Prefer existing COOLJAPAN replacements over `-sys` crates.
- **No panics on untrusted input.** Do not add `.unwrap()`, `.expect()`,
  `panic!()`, `unreachable!()`, or `assert!()` on data derived from
  untrusted input outside of test code; return the crate's existing
  typed error (`OxiTextError` / `SdfError`) instead.
- **Zero clippy warnings.** `cargo clippy --all-targets -- -D warnings`
  must pass cleanly with default features.
- **Workspace dependency inheritance.** Shared dependencies are
  declared once in the workspace `[workspace.dependencies]` table and
  pulled in via `dep.workspace = true`; do not pin ad hoc versions in a
  member crate's `Cargo.toml` when the workspace already centralizes
  that dependency.
- **File size.** Keep individual source files under 2000 lines; split
  oversized files into focused modules. This applies to vendored code
  too — see the vendored-code rules below.
- **Latest crates.** Prefer the latest versions available on crates.io
  for new or updated dependencies.
- **No hardcoded absolute paths.** Tests and examples must use
  `std::env::temp_dir()` (or an equivalent relative/portable path) for
  any temporary file handling.

## Vendored third-party code

`crates/oxitext-swash` is a vendored fork of `swash` 0.2.10 by Chad
Brokaw, carrying OxiText's fix for two Indic reordering defects. It is
the only vendored crate in the workspace and it follows different
rules, because its value depends on staying comparable with upstream.
Read `crates/oxitext-swash/PROVENANCE.md` before touching it.

- **Every modified file carries an Apache-2.0 §4(b) header** of the form
  `// OXITEXT MODIFICATION (oxitext 0.2.2): <what changed>. See
  <...>PROVENANCE.md.`, and the `PROVENANCE.md` divergence table is
  updated in the same change. Since the crate was brought under house
  style, those two things — not byte-identity with upstream — **are**
  the audit. 37 of the 61 vendored files are still byte-identical, and
  a reviewer should still ask why when one of them changes.
- **Never edit `LICENSE-APACHE` or `LICENSE-MIT`** in that directory.
- **The house rules apply in full**: the 2000-line limit (upstream's
  5 491-line `text/unicode_data.rs` is split into a directory module
  with its public paths preserved), zero clippy warnings, no
  `.unwrap()`/`.expect()` outside tests, workspace dependency
  inheritance, and the COOLJAPAN dependency-replacement table
  (upstream's `yazi` inflater is now `oxiarc-deflate`). The crate is
  clean under all of them today; keep it that way.

## Submitting changes

Open a pull request against the appropriate version branch (not
directly against a release branch, unless the project is pre-0.1.0).
Describe what changed and why, and make sure the build/test/lint
commands above all pass locally first.
