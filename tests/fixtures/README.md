# Test font fixtures

Fonts vendored here are used by the integration tests in
`crates/*/tests/`.  Every entry records where the file came from, under which
licence it is redistributed, and its SHA-256 so that an accidental
re-encoding is caught in review.

Tests must locate fixtures relative to `CARGO_MANIFEST_DIR`
(`../../tests/fixtures/<name>`) and skip gracefully when a fixture is absent, so
that a source checkout without the binaries still builds.

| File | Bytes | SHA-256 | Source | Licence |
|---|---|---|---|---|
| `test-font.ttf` | 569208 | `b85c38ecea8a7cfb39c24e395a4007474fa5a4fc864f6ee33309eb4948d232d5` | Generic Latin TrueType face used by the layout/raster smoke tests | See upstream distribution |
| `twemoji_smiley-glyf_colr_1.ttf` | 7420 | `b462e4de616a38979b053a49b0b5a5f2dd72b6d5f55be43c5b1eb812fd438dbc` | [googlefonts/color-fonts](https://github.com/googlefonts/color-fonts) `fonts/twemoji_smiley-glyf_colr_1.ttf` | Apache-2.0 |
| `noto_handwriting-glyf_colr_1.ttf` | 5072 | `a9067375e2d48a7a085311291d6931fbdae6f461ee8ed1db930f417c7ea727a4` | [googlefonts/color-fonts](https://github.com/googlefonts/color-fonts) `fonts/noto_handwriting-glyf_colr_1.ttf` | Apache-2.0 |
| `test_glyphs-glyf_colr_1.ttf` | 21568 | `8aa611b1ca97044ac6f13dc982fde29256612f0a5acc6ef47ca541a7a5b99b28` | [googlefonts/color-fonts](https://github.com/googlefonts/color-fonts) `fonts/test_glyphs-glyf_colr_1.ttf` | Apache-2.0 |

## COLR fixtures

The three `*-glyf_colr_1.ttf` files are COLRv1 fonts with TrueType (`glyf`)
contours, built by the upstream `googlefonts/color-fonts` repository to
exercise colour-font toolchains.  They cover complementary parts of the format:

* **`twemoji_smiley-glyf_colr_1.ttf`** — 15 real Twemoji smiley emoji
  (U+263A, U+1F601, U+1F603–U+1F608, U+1F60A, U+1F60D–U+1F60F, U+1F619,
  U+1F642, U+1F970).  Uses
  `PaintColrLayers`, `PaintGlyph`, `PaintSolid`, `PaintTransform` and a
  `ClipList`.  This is the "does a real emoji come out in colour" fixture.
* **`noto_handwriting-glyf_colr_1.ttf`** — the Noto Emoji writing-hand
  ✍️ (U+270D) alone, built from the same sources as the full Noto COLRv1 emoji
  font.  Adds `PaintLinearGradient` and `PaintRadialGradient` on top of the
  above, so it is the smallest fixture that proves the gradient paths work with
  genuine Noto data.
* **`test_glyphs-glyf_colr_1.ttf`** — 201 synthetic glyphs covering the whole
  paint-format matrix: linear, radial and sweep gradients in all three
  `Extend` modes, the full transform family, `PaintColrGlyph` (including
  deliberate recursion cycles), and all 28 `PaintComposite` modes.

### Regenerating

```sh
curl -L -o color-fonts.tar.gz \
  https://codeload.github.com/googlefonts/color-fonts/tar.gz/refs/heads/main
tar xzf color-fonts.tar.gz --strip-components=2 \
  color-fonts-main/fonts/twemoji_smiley-glyf_colr_1.ttf \
  color-fonts-main/fonts/noto_handwriting-glyf_colr_1.ttf \
  color-fonts-main/fonts/test_glyphs-glyf_colr_1.ttf
shasum -a 256 *.ttf
```

### Testing against the complete Noto COLRv1 emoji font

The full font (`noto-glyf_colr_1.ttf` in the same upstream repository, or
`Noto-COLRv1.ttf` from the `googlefonts/noto-emoji` releases) is ~4.6 MB and is
deliberately *not* vendored.  Point `OXITEXT_TEST_COLR_FONT` at a local copy to
run the opt-in whole-font sweep:

```sh
OXITEXT_TEST_COLR_FONT=/path/to/noto-glyf_colr_1.ttf \
  cargo nextest run -p oxitext-raster colr
```
