//! Fuzz target for `oxitext-core`'s hand-written PNG reader,
//! `png_decode::decode_png_rgba8`, over untrusted bytes.
//!
//! This is the parser behind every PNG-encoded `CBDT`/`sbix` colour-bitmap
//! strike (an emoji font is attacker-controllable input for anything that
//! renders user-supplied documents), and it is entirely in-tree: chunk walk +
//! CRC check, zlib inflate via `oxiarc-deflate`, scanline unfiltering,
//! sub-byte sample unpacking, palette/`tRNS` lookup and Adam7 pass geometry.
//!
//! It must never panic: every rejection path is a typed
//! [`oxitext_core::png_decode::PngDecodeError`], and the declared geometry is
//! validated against the inflated length before any output buffer is
//! allocated, so a crafted `IHDR` cannot turn a few bytes of input into a huge
//! allocation.
//!
//! Run with (requires the nightly toolchain `cargo-fuzz` uses internally):
//! ```text
//! cargo +nightly fuzz run png_decode
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Both shapes matter: a raw blob (which almost always fails at the
    // signature check) and the same blob behind a valid signature, which
    // pushes the fuzzer straight into the chunk walker.
    let _ = oxitext_core::png_decode::decode_png_rgba8(data);

    let mut signed = Vec::with_capacity(data.len() + 8);
    signed.extend_from_slice(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']);
    signed.extend_from_slice(data);
    let _ = oxitext_core::png_decode::decode_png_rgba8(&signed);
});
