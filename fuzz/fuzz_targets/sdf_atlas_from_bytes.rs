//! Fuzz target for `SdfAtlas::from_bytes`, the binary atlas deserializer.
//!
//! This is the parser that shipped an integer-overflow out-of-bounds read in
//! 0.2.1 (`expected_len` computed with plain `usize` arithmetic from
//! attacker-controlled `atlas_w`/`atlas_h`/`num_entries` header fields could
//! wrap and sail past the length guard -- see CHANGELOG.md's `[0.2.1]` entry
//! and the `from_bytes_rejects_overflowing_header_without_panicking`
//! regression test in `crates/oxitext-sdf/src/atlas.rs`). A fuzz target here
//! is the general form of that regression test: it exercises arbitrary
//! malformed headers rather than one specific crafted overflow case.
//!
//! `from_bytes` must never panic on any input -- only return `Ok` or `Err`.
//!
//! Run with (requires the nightly toolchain `cargo-fuzz` uses internally):
//! ```text
//! cargo +nightly fuzz run sdf_atlas_from_bytes
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use oxitext_sdf::SdfAtlas;

fuzz_target!(|data: &[u8]| {
    // The return value is deliberately discarded: the only property under
    // test is "does not panic" (no OOB slice index, no arithmetic overflow
    // panic in a debug build, no unwrap/expect anywhere on this path).
    let _ = SdfAtlas::from_bytes(data);
});
