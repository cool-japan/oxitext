//! SDF atlas packer: bin-packs SDF glyph tiles into a GPU-ready texture.
//!
//! Provides shelf-packing and MaxRects packing for both single-channel
//! ([`SdfAtlas`]) and multi-channel ([`MsdfAtlas`]) distance-field tiles.

use std::collections::HashMap;

use oxitext_core::png_encode::{encode_png, PngColorType};

use crate::edt::SdfError;

/// A single SDF glyph tile produced by the SDF pipeline.
#[derive(Clone, Debug)]
pub struct SdfTile {
    /// Glyph ID within the font.
    pub glyph_id: u16,
    /// Tile width in pixels.
    pub width: u32,
    /// Tile height in pixels.
    pub height: u32,
    /// SDF pixel data (`width × height` bytes, values 0–255).
    pub data: Vec<u8>,
    /// Left bearing in pixels (signed).
    pub bearing_x: i32,
    /// Top bearing in pixels (signed).
    pub bearing_y: i32,
    /// Horizontal advance in pixels.
    pub advance_x: f32,
}

impl SdfTile {
    /// Create an SDF tile from a pre-rasterized floating-point coverage bitmap.
    ///
    /// # Arguments
    /// - `glyph_id` — glyph index within the font.
    /// - `coverage` — greyscale coverage values, `width × height`, values in `[0, 1]`.
    /// - `width`, `height` — bitmap dimensions (must be non-zero).
    /// - `spread` — maximum SDF distance in pixels; maps ±spread to \[0, 255\].
    /// - `bearing_x`, `bearing_y` — glyph bearing in pixels (signed).
    /// - `advance_x` — horizontal advance in pixels.
    ///
    /// # Errors
    /// Returns [`SdfError::ZeroSize`] when `width` or `height` is zero.
    /// Returns [`SdfError::InvalidInput`] when `coverage.len() != width * height`.
    #[allow(clippy::too_many_arguments)]
    pub fn from_coverage(
        glyph_id: u16,
        coverage: &[f32],
        width: usize,
        height: usize,
        spread: f32,
        bearing_x: i32,
        bearing_y: i32,
        advance_x: f32,
    ) -> Result<Self, SdfError> {
        if width == 0 || height == 0 {
            return Err(SdfError::ZeroSize);
        }
        if coverage.len() != width * height {
            return Err(SdfError::InvalidInput(format!(
                "coverage length {} != width({}) * height({})",
                coverage.len(),
                width,
                height
            )));
        }
        // Convert f32 coverage [0, 1] → u8 coverage [0, 255].
        let coverage_u8: Vec<u8> = coverage
            .iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        let sdf_data = crate::edt::compute_sdf(&coverage_u8, width, height, spread, 0)?;
        Ok(Self {
            glyph_id,
            width: width as u32,
            height: height as u32,
            data: sdf_data,
            bearing_x,
            bearing_y,
            advance_x,
        })
    }
}

/// A UV rectangle within the atlas texture (all values in [0, 1]).
#[derive(Clone, Debug)]
pub struct UvRect {
    /// Left edge (U coordinate).
    pub u_min: f32,
    /// Top edge (V coordinate).
    pub v_min: f32,
    /// Right edge (U coordinate).
    pub u_max: f32,
    /// Bottom edge (V coordinate).
    pub v_max: f32,
}

// ─── Packing options and statistics ─────────────────────────────────────────

/// Packing algorithm selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackingAlgorithm {
    /// Left-to-right shelf packing (existing behavior).
    #[default]
    Shelf,
    /// Best Short-Side Fit (BSSF) MaxRects algorithm.
    MaxRects,
    /// Skyline algorithm: tracks a fill-level skyline for better utilization.
    Skyline,
}

/// Options for atlas packing.
#[derive(Debug, Clone)]
pub struct AtlasOptions {
    /// Atlas texture size in pixels (width and height).
    pub atlas_size: u32,
    /// Pixel padding between tiles.
    pub padding: u32,
    /// Optional maximum atlas size (used for dynamic growth).
    pub max_size: Option<u32>,
    /// Bin-packing algorithm to use.
    pub algorithm: PackingAlgorithm,
}

impl Default for AtlasOptions {
    fn default() -> Self {
        Self {
            atlas_size: 512,
            padding: 1,
            max_size: None,
            algorithm: PackingAlgorithm::Shelf,
        }
    }
}

/// Statistics returned from an atlas packing operation.
#[derive(Debug, Clone, Default)]
pub struct AtlasStats {
    /// Number of tiles successfully packed.
    pub tiles_packed: usize,
    /// Number of tiles that did not fit and were dropped.
    pub tiles_dropped: usize,
    /// Fraction of the atlas area that is occupied (0.0 – 1.0).
    pub utilization: f32,
    /// Number of unused pixels (total atlas area minus packed tile pixels).
    pub wasted_pixels: u32,
}

// ─── Internal packing result ──────────────────────────────────────────────────

struct PackResult {
    texture: Vec<u8>,
    uv_map: HashMap<u16, UvRect>,
    /// Number of tiles that were dropped (overflow).
    dropped: usize,
    /// Sum of packed tile pixel areas (width × height for each placed tile).
    used_pixels: u32,
    /// Shelf state for incremental `add_tile` calls (only meaningful for Shelf algorithm).
    cursor_x: u32,
    shelf_y: u32,
    shelf_max_h: u32,
}

// ─── Shelf packing ────────────────────────────────────────────────────────────

/// Core shelf-packing routine shared by `SdfAtlas::pack` and `SdfAtlas::pack_with_options`.
///
/// `atlas_w`/`atlas_h` are the final dimensions of the texture.
/// `padding` adds a gap (in pixels) between tiles both horizontally and vertically.
fn pack_inner(tiles: &[SdfTile], atlas_w: u32, atlas_h: u32, padding: u32) -> PackResult {
    let tex_len = atlas_w as usize * atlas_h as usize;
    let mut texture = vec![0u8; tex_len];
    let mut uv_map: HashMap<u16, UvRect> = HashMap::new();

    let mut cx: u32 = padding;
    let mut cy: u32 = padding;
    let mut row_h: u32 = 0;
    let mut dropped: usize = 0;
    let mut used_pixels: u32 = 0;

    for tile in tiles {
        let needed_w = tile.width + padding;
        // Try to start a new shelf if the tile doesn't fit horizontally.
        if cx + tile.width > atlas_w.saturating_sub(padding) {
            cx = padding;
            cy += row_h + padding;
            row_h = 0;
        }

        // Drop tiles that overflow the atlas height.
        if cy + tile.height > atlas_h.saturating_sub(padding) {
            dropped += 1;
            continue;
        }

        // Blit single-channel tile data into the atlas.
        for y in 0..tile.height {
            for x in 0..tile.width {
                let src_idx = (y * tile.width + x) as usize;
                let dst_idx = ((cy + y) * atlas_w + (cx + x)) as usize;
                if dst_idx < texture.len() && src_idx < tile.data.len() {
                    texture[dst_idx] = tile.data[src_idx];
                }
            }
        }

        uv_map.insert(
            tile.glyph_id,
            UvRect {
                u_min: cx as f32 / atlas_w as f32,
                v_min: cy as f32 / atlas_h as f32,
                u_max: (cx + tile.width) as f32 / atlas_w as f32,
                v_max: (cy + tile.height) as f32 / atlas_h as f32,
            },
        );

        used_pixels += tile.width * tile.height;
        cx += needed_w;
        row_h = row_h.max(tile.height);
    }

    PackResult {
        texture,
        uv_map,
        dropped,
        used_pixels,
        cursor_x: cx,
        shelf_y: cy,
        shelf_max_h: row_h,
    }
}

// ─── MaxRects packing ─────────────────────────────────────────────────────────

/// Axis-aligned rectangle used internally by the MaxRects packer.
#[derive(Clone, Copy, Debug)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Rect {
    /// Returns `true` if `other` is entirely contained within `self`.
    fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.w <= self.x + self.w
            && other.y + other.h <= self.y + self.h
    }
}

/// Mutable state for the MaxRects packer.
struct MaxRectsState {
    free_rects: Vec<Rect>,
}

impl MaxRectsState {
    /// Create state initialised with a single free rect covering the whole atlas.
    fn new(atlas_w: u32, atlas_h: u32) -> Self {
        Self {
            free_rects: vec![Rect {
                x: 0,
                y: 0,
                w: atlas_w,
                h: atlas_h,
            }],
        }
    }
}

/// Attempt to insert a tile of `tile_w × tile_h` (plus padding on each side)
/// into the MaxRects state.
///
/// Uses the Best Short-Side Fit (BSSF) heuristic: among all free rectangles
/// large enough to hold the tile, choose the one where
/// `min(free.w - tile_w, free.h - tile_h)` is smallest (i.e. wastes the least
/// space along the shorter axis).
///
/// Returns the placed [`Rect`] on success, or `None` when no free rect is large
/// enough.
fn insert_maxrects(
    state: &mut MaxRectsState,
    tile_w: u32,
    tile_h: u32,
    padding: u32,
) -> Option<Rect> {
    // The region we actually need (tile + padding on every side).
    let needed_w = tile_w + padding;
    let needed_h = tile_h + padding;

    // Find the free rect that minimises the "short side fit" score.
    let best_idx = state
        .free_rects
        .iter()
        .enumerate()
        .filter(|(_, r)| r.w >= needed_w && r.h >= needed_h)
        .min_by_key(|(_, r)| {
            let leftover_w = r.w.saturating_sub(needed_w);
            let leftover_h = r.h.saturating_sub(needed_h);
            leftover_w.min(leftover_h)
        })
        .map(|(i, _)| i);

    let idx = best_idx?;
    let chosen = state.free_rects[idx];

    // The placed rect occupies only the tile pixels (no padding baked in).
    let placed = Rect {
        x: chosen.x,
        y: chosen.y,
        w: tile_w,
        h: tile_h,
    };

    // Split the chosen free rect into up to 4 new free rects around the
    // placed tile (with padding absorbed into the split).  We generate the
    // classic two-split variants (horizontal and vertical) and keep all four
    // non-degenerate results.
    let mut new_rects: Vec<Rect> = Vec::with_capacity(4);

    // Right of placed tile (+ padding)
    if chosen.x + chosen.w > placed.x + needed_w {
        let nr = Rect {
            x: placed.x + needed_w,
            y: chosen.y,
            w: chosen.x + chosen.w - (placed.x + needed_w),
            h: chosen.h,
        };
        if nr.w > 0 && nr.h > 0 {
            new_rects.push(nr);
        }
    }
    // Below placed tile (+ padding)
    if chosen.y + chosen.h > placed.y + needed_h {
        let nr = Rect {
            x: chosen.x,
            y: placed.y + needed_h,
            w: chosen.w,
            h: chosen.y + chosen.h - (placed.y + needed_h),
        };
        if nr.w > 0 && nr.h > 0 {
            new_rects.push(nr);
        }
    }
    // Left of placed tile (padding from atlas edge)
    if placed.x > chosen.x {
        let nr = Rect {
            x: chosen.x,
            y: chosen.y,
            w: placed.x - chosen.x,
            h: chosen.h,
        };
        if nr.w > 0 && nr.h > 0 {
            new_rects.push(nr);
        }
    }
    // Above placed tile (padding from atlas edge)
    if placed.y > chosen.y {
        let nr = Rect {
            x: chosen.x,
            y: chosen.y,
            w: chosen.w,
            h: placed.y - chosen.y,
        };
        if nr.w > 0 && nr.h > 0 {
            new_rects.push(nr);
        }
    }

    // Remove the chosen rect (it is now consumed) and append the new splits.
    state.free_rects.remove(idx);
    state.free_rects.extend_from_slice(&new_rects);

    // Prune free rects that are fully contained inside another free rect.
    // We do a pairwise check — this is O(n²) but n stays small in practice.
    let mut to_remove: Vec<bool> = vec![false; state.free_rects.len()];
    let len = state.free_rects.len();
    for i in 0..len {
        if to_remove[i] {
            continue;
        }
        for j in 0..len {
            if i == j || to_remove[j] {
                continue;
            }
            if state.free_rects[j].contains(&state.free_rects[i]) {
                to_remove[i] = true;
                break;
            }
        }
    }
    let mut keep_idx = 0;
    state.free_rects.retain(|_| {
        let keep = !to_remove[keep_idx];
        keep_idx += 1;
        keep
    });

    Some(placed)
}

/// MaxRects packing routine.
fn pack_inner_maxrects(tiles: &[SdfTile], atlas_w: u32, atlas_h: u32, padding: u32) -> PackResult {
    let tex_len = atlas_w as usize * atlas_h as usize;
    let mut texture = vec![0u8; tex_len];
    let mut uv_map: HashMap<u16, UvRect> = HashMap::new();
    let mut dropped: usize = 0;
    let mut used_pixels: u32 = 0;

    let mut state = MaxRectsState::new(atlas_w, atlas_h);

    for tile in tiles {
        match insert_maxrects(&mut state, tile.width, tile.height, padding) {
            None => {
                dropped += 1;
            }
            Some(placed) => {
                // Blit tile data into the atlas texture.
                for y in 0..tile.height {
                    for x in 0..tile.width {
                        let src_idx = (y * tile.width + x) as usize;
                        let dst_idx = ((placed.y + y) * atlas_w + (placed.x + x)) as usize;
                        if dst_idx < texture.len() && src_idx < tile.data.len() {
                            texture[dst_idx] = tile.data[src_idx];
                        }
                    }
                }

                uv_map.insert(
                    tile.glyph_id,
                    UvRect {
                        u_min: placed.x as f32 / atlas_w as f32,
                        v_min: placed.y as f32 / atlas_h as f32,
                        u_max: (placed.x + placed.w) as f32 / atlas_w as f32,
                        v_max: (placed.y + placed.h) as f32 / atlas_h as f32,
                    },
                );

                used_pixels += tile.width * tile.height;
            }
        }
    }

    PackResult {
        texture,
        uv_map,
        dropped,
        used_pixels,
        cursor_x: 0,
        shelf_y: 0,
        shelf_max_h: 0,
    }
}

// ─── Skyline packing ──────────────────────────────────────────────────────────

/// A single segment of the skyline, tracking the current height at a horizontal range.
struct SkylineSegment {
    /// Left-most x coordinate of this segment.
    x: u32,
    /// Current fill height at this x range (top = highest occupied y + 1).
    y_top: u32,
    /// Width of this segment.
    width: u32,
}

/// Mutable state for the Skyline packer.
struct SkylineState {
    segments: Vec<SkylineSegment>,
    atlas_w: u32,
    atlas_h: u32,
}

impl SkylineState {
    /// Initialise with a single flat skyline at y = 0.
    fn new(atlas_w: u32, atlas_h: u32) -> Self {
        Self {
            segments: vec![SkylineSegment {
                x: 0,
                y_top: 0,
                width: atlas_w,
            }],
            atlas_w,
            atlas_h,
        }
    }
}

/// Attempt to insert a tile of `tile_w × tile_h` (plus inter-tile `padding`)
/// using the Skyline best-fit heuristic.
///
/// Returns `Some((x, y))` (top-left corner of the placed tile, in pixel coords)
/// or `None` when no position is feasible.
fn insert_skyline(
    state: &mut SkylineState,
    tile_w: u32,
    tile_h: u32,
    padding: u32,
) -> Option<(u32, u32)> {
    let needed_w = tile_w + padding;
    let n = state.segments.len();

    // For each starting segment, check whether a run of segments wide enough
    // to hold `needed_w` exists and is below the atlas ceiling.
    let mut best_y = u32::MAX;
    let mut best_x = 0u32;

    'outer: for i in 0..n {
        let x_start = state.segments[i].x;
        // Tile must not start past the atlas right edge (minus a padding gap).
        if x_start + needed_w > state.atlas_w.saturating_sub(padding) {
            continue;
        }

        let mut max_y = 0u32;
        let mut covered_w = 0u32;

        for j in i..n {
            let seg = &state.segments[j];
            max_y = max_y.max(seg.y_top);
            covered_w += seg.width;

            if covered_w >= needed_w {
                // Verify it fits vertically.
                if max_y + tile_h + padding <= state.atlas_h && max_y < best_y {
                    best_y = max_y;
                    best_x = x_start;
                }
                continue 'outer;
            }
        }
    }

    if best_y == u32::MAX {
        return None; // No valid placement found.
    }

    let place_x = best_x;
    let place_y = best_y;
    let new_top = best_y + tile_h + padding;
    let tile_right = place_x + needed_w;

    // Build the updated skyline: replace all segments that the tile spans with
    // a single segment at the new height.
    let mut new_segments: Vec<SkylineSegment> = Vec::with_capacity(state.segments.len() + 2);

    for seg in &state.segments {
        let seg_right = seg.x + seg.width;
        if seg_right <= place_x || seg.x >= tile_right {
            // Segment is entirely outside the placed tile's horizontal span.
            new_segments.push(SkylineSegment {
                x: seg.x,
                y_top: seg.y_top,
                width: seg.width,
            });
        } else {
            // This segment overlaps the tile.  Emit left/right remnants (if any)
            // and a new raised segment at `new_top`.
            // Left remnant
            if seg.x < place_x {
                new_segments.push(SkylineSegment {
                    x: seg.x,
                    y_top: seg.y_top,
                    width: place_x - seg.x,
                });
            }
        }
    }

    // Insert the new raised segment for the placed tile.
    new_segments.push(SkylineSegment {
        x: place_x,
        y_top: new_top,
        width: needed_w,
    });

    // Emit right remnants of partially-covered segments.
    for seg in &state.segments {
        let seg_right = seg.x + seg.width;
        if seg.x < tile_right && seg_right > tile_right {
            new_segments.push(SkylineSegment {
                x: tile_right,
                y_top: seg.y_top,
                width: seg_right - tile_right,
            });
        }
    }

    // Sort by x to maintain the invariant.
    new_segments.sort_by_key(|s| s.x);

    // Merge adjacent segments at the same height.
    let mut merged: Vec<SkylineSegment> = Vec::with_capacity(new_segments.len());
    for seg in new_segments {
        if let Some(last) = merged.last_mut() {
            if last.y_top == seg.y_top && last.x + last.width == seg.x {
                last.width += seg.width;
                continue;
            }
        }
        merged.push(seg);
    }

    state.segments = merged;
    Some((place_x, place_y))
}

/// Skyline packing routine.
fn pack_inner_skyline(tiles: &[SdfTile], atlas_w: u32, atlas_h: u32, padding: u32) -> PackResult {
    let tex_len = atlas_w as usize * atlas_h as usize;
    let mut texture = vec![0u8; tex_len];
    let mut uv_map: HashMap<u16, UvRect> = HashMap::new();
    let mut dropped: usize = 0;
    let mut used_pixels: u32 = 0;

    let mut state = SkylineState::new(atlas_w, atlas_h);

    for tile in tiles {
        match insert_skyline(&mut state, tile.width, tile.height, padding) {
            None => {
                dropped += 1;
            }
            Some((px, py)) => {
                for y in 0..tile.height {
                    for x in 0..tile.width {
                        let src_idx = (y * tile.width + x) as usize;
                        let dst_idx = ((py + y) * atlas_w + (px + x)) as usize;
                        if dst_idx < texture.len() && src_idx < tile.data.len() {
                            texture[dst_idx] = tile.data[src_idx];
                        }
                    }
                }

                uv_map.insert(
                    tile.glyph_id,
                    UvRect {
                        u_min: px as f32 / atlas_w as f32,
                        v_min: py as f32 / atlas_h as f32,
                        u_max: (px + tile.width) as f32 / atlas_w as f32,
                        v_max: (py + tile.height) as f32 / atlas_h as f32,
                    },
                );

                used_pixels += tile.width * tile.height;
            }
        }
    }

    PackResult {
        texture,
        uv_map,
        dropped,
        used_pixels,
        cursor_x: 0,
        shelf_y: 0,
        shelf_max_h: 0,
    }
}

// ─── Multi-page atlas ─────────────────────────────────────────────────────────

/// An atlas that spans multiple pages when a single texture is not large enough.
///
/// Each page is an independent [`SdfAtlas`] of the same `page_size`.  Tiles are
/// distributed greedily: when the current page is full a new one is opened.
pub struct MultiPageAtlas {
    /// Individual atlas pages (same dimensions, different tiles).
    pub pages: Vec<SdfAtlas>,
    /// Pixel size of each page (width == height).
    pub page_size: u32,
}

impl MultiPageAtlas {
    /// Pack `tiles` across as many pages as necessary.
    ///
    /// Tiles that are too large to fit a fresh page on their own are silently
    /// dropped to avoid an infinite loop.  All other tiles are packed with the
    /// Shelf algorithm.
    pub fn pack(tiles: &[SdfTile], page_size: u32, padding: u32) -> Self {
        let mut pages: Vec<SdfAtlas> = Vec::new();
        let atlas_size = page_size.next_power_of_two().max(64);
        // Work with an owned queue so we can mutate between iterations.
        let mut queue: Vec<SdfTile> = tiles.to_vec();

        while !queue.is_empty() {
            let tex_len = atlas_size as usize * atlas_size as usize;
            let mut page = SdfAtlas {
                width: atlas_size,
                height: atlas_size,
                texture: vec![0u8; tex_len],
                uv_map: HashMap::new(),
                cursor_x: padding,
                shelf_y: padding,
                shelf_max_h: 0,
                padding,
            };

            let mut any_packed = false;
            let mut leftover: Vec<SdfTile> = Vec::new();

            for tile in &queue {
                // Tiles that would never fit in any page are permanently dropped.
                if tile.width + 2 * padding > atlas_size || tile.height + 2 * padding > atlas_size {
                    continue;
                }
                match page.add_tile(tile) {
                    Some(_) => {
                        any_packed = true;
                    }
                    None => {
                        leftover.push(tile.clone());
                    }
                }
            }

            pages.push(page);

            if !any_packed {
                // Nothing fit even on a fresh page — avoid an infinite loop.
                break;
            }

            queue = leftover;
        }

        Self { pages, page_size }
    }

    /// Find the UV rect for a glyph across all pages.
    ///
    /// Returns `(page_index, &UvRect)` if found, or `None`.
    pub fn lookup(&self, glyph_id: u16) -> Option<(usize, &UvRect)> {
        self.pages
            .iter()
            .enumerate()
            .find_map(|(i, page)| page.uv_map.get(&glyph_id).map(|uv| (i, uv)))
    }
}

// ─── Dynamic atlas growth ─────────────────────────────────────────────────────

/// Pack tiles into an atlas that grows automatically until all tiles fit or
/// `max_size` is reached.
///
/// # Algorithm
/// 1. Start with `current_size = initial_size`.
/// 2. Call [`SdfAtlas::pack_with_options`] with the given `algorithm`.
/// 3. If any tiles were dropped *and* `current_size < max_size`, double
///    `current_size` (capped at `max_size`) and retry.
/// 4. Return the final atlas and statistics once either all tiles fit or
///    `max_size` is reached.
pub fn pack_growing(
    tiles: &[SdfTile],
    initial_size: u32,
    max_size: u32,
    padding: u32,
    algorithm: PackingAlgorithm,
) -> (SdfAtlas, AtlasStats) {
    let mut current_size = initial_size.max(1);
    loop {
        let options = AtlasOptions {
            atlas_size: current_size,
            padding,
            max_size: Some(max_size),
            algorithm,
        };
        let (atlas, stats) = SdfAtlas::pack_with_options(tiles, &options);
        if stats.tiles_dropped == 0 || current_size >= max_size {
            return (atlas, stats);
        }
        // Double the atlas size, capped at max_size.
        current_size = (current_size.saturating_mul(2)).min(max_size);
    }
}

// ─── SdfAtlas ─────────────────────────────────────────────────────────────────

/// A packed atlas of SDF glyph tiles, ready for GPU upload.
///
/// The atlas texture is stored as a single-channel byte slice (`texture`),
/// where each byte is an SDF value (< 128 = outside, ≈ 128 = outline, > 128 = inside).
///
/// Shelf state (`cursor_x`, `shelf_y`, `shelf_max_h`) is maintained for incremental
/// tile insertion via [`SdfAtlas::add_tile`].
pub struct SdfAtlas {
    /// Atlas width in pixels.
    pub width: u32,
    /// Atlas height in pixels.
    pub height: u32,
    /// Raw single-channel texture data (`width × height` bytes).
    pub texture: Vec<u8>,
    /// UV coordinates for each glyph ID.
    pub uv_map: HashMap<u16, UvRect>,
    /// Current X cursor position for the active shelf.
    cursor_x: u32,
    /// Y coordinate of the current shelf baseline.
    shelf_y: u32,
    /// Tallest tile seen in the current shelf row.
    shelf_max_h: u32,
    /// Inter-tile padding applied during packing.
    padding: u32,
}

// ─── Binary serialization constants ──────────────────────────────────────────

const MAGIC: &[u8; 4] = b"SDFA";
const VERSION: u32 = 1;
/// Bytes per entry in the binary format.
const ENTRY_SIZE: usize = 28;
/// Byte offset where entries start.
const ENTRIES_OFFSET: usize = 20;

impl SdfAtlas {
    /// Create a blank atlas of the given dimensions, pre-allocated with zeroed texture data.
    ///
    /// The texture is zero-filled (`width × height` bytes).
    /// Shelf state is reset to the origin (no padding).
    pub fn new(width: u32, height: u32) -> Self {
        let capacity = width as usize * height as usize;
        Self {
            width,
            height,
            texture: vec![0u8; capacity],
            uv_map: HashMap::new(),
            cursor_x: 0,
            shelf_y: 0,
            shelf_max_h: 0,
            padding: 0,
        }
    }

    /// Pre-allocate texture capacity for a known number of tiles.
    ///
    /// Uses `average_tile_area` as the estimated pixels per tile when reserving
    /// additional texture capacity beyond what `new` already allocates.
    ///
    /// The actual texture length stays at `width × height`; only the internal
    /// Vec capacity is grown, so no extra zeroing cost is paid for unused space.
    pub fn with_capacity(
        width: u32,
        height: u32,
        tile_count_hint: usize,
        average_tile_area: usize,
    ) -> Self {
        let capacity = (tile_count_hint * average_tile_area).min((width * height) as usize);
        let mut atlas = Self::new(width, height);
        atlas
            .texture
            .reserve(capacity.saturating_sub(atlas.texture.len()));
        atlas
    }

    /// Pack a set of SDF tiles into a power-of-2 atlas texture using a shelf-packing algorithm.
    ///
    /// Tiles are packed left-to-right. When a row is full a new shelf begins below.
    /// Atlas dimensions are rounded up to the next power of two (minimum 256 × 256).
    ///
    /// If `tiles` is empty, returns a minimal 1×1 atlas.
    pub fn pack(tiles: &[SdfTile]) -> Self {
        if tiles.is_empty() {
            return Self {
                width: 1,
                height: 1,
                texture: vec![0],
                uv_map: HashMap::new(),
                cursor_x: 0,
                shelf_y: 0,
                shelf_max_h: 0,
                padding: 0,
            };
        }

        // Estimate atlas dimensions using a square grid layout.
        let tile_w = tiles[0].width;
        let tile_h = tiles[0].height;
        let count = tiles.len() as u32;
        let cols = (count as f32).sqrt().ceil() as u32;
        let rows = count.div_ceil(cols);
        let atlas_w = (cols * tile_w).next_power_of_two().max(256);
        let atlas_h = (rows * tile_h).next_power_of_two().max(256);

        let res = pack_inner(tiles, atlas_w, atlas_h, 0);

        Self {
            width: atlas_w,
            height: atlas_h,
            texture: res.texture,
            uv_map: res.uv_map,
            cursor_x: res.cursor_x,
            shelf_y: res.shelf_y,
            shelf_max_h: res.shelf_max_h,
            padding: 0,
        }
    }

    /// Pack tiles with configurable options, returning both the atlas and packing statistics.
    ///
    /// Respects [`AtlasOptions::padding`] to add spacing between tiles. The atlas size is
    /// fixed at [`AtlasOptions::atlas_size`] (rounded up to the next power of two).
    ///
    /// Tiles that do not fit are counted in [`AtlasStats::tiles_dropped`].
    pub fn pack_with_options(tiles: &[SdfTile], options: &AtlasOptions) -> (Self, AtlasStats) {
        let atlas_size = options.atlas_size.next_power_of_two().max(64);

        if tiles.is_empty() {
            let atlas = Self {
                width: atlas_size,
                height: atlas_size,
                texture: vec![0u8; (atlas_size as usize) * (atlas_size as usize)],
                uv_map: HashMap::new(),
                cursor_x: options.padding,
                shelf_y: options.padding,
                shelf_max_h: 0,
                padding: options.padding,
            };
            return (
                atlas,
                AtlasStats {
                    tiles_packed: 0,
                    tiles_dropped: 0,
                    utilization: 0.0,
                    wasted_pixels: atlas_size * atlas_size,
                },
            );
        }

        let res = match options.algorithm {
            PackingAlgorithm::Shelf => pack_inner(tiles, atlas_size, atlas_size, options.padding),
            PackingAlgorithm::MaxRects => {
                pack_inner_maxrects(tiles, atlas_size, atlas_size, options.padding)
            }
            PackingAlgorithm::Skyline => {
                pack_inner_skyline(tiles, atlas_size, atlas_size, options.padding)
            }
        };

        let total = atlas_size * atlas_size;
        let tiles_packed = tiles.len() - res.dropped;
        let utilization = res.used_pixels as f32 / total as f32;
        let wasted_pixels = total.saturating_sub(res.used_pixels);

        let stats = AtlasStats {
            tiles_packed,
            tiles_dropped: res.dropped,
            utilization,
            wasted_pixels,
        };

        let atlas = Self {
            width: atlas_size,
            height: atlas_size,
            texture: res.texture,
            uv_map: res.uv_map,
            cursor_x: res.cursor_x,
            shelf_y: res.shelf_y,
            shelf_max_h: res.shelf_max_h,
            padding: options.padding,
        };

        (atlas, stats)
    }

    /// Pack tiles into an atlas that doubles in size until all tiles fit or
    /// `max_size` is reached, using the default Shelf algorithm.
    ///
    /// This is a convenience wrapper around the free function [`pack_growing`].
    pub fn pack_growing(tiles: &[SdfTile], initial_size: u32, max_size: u32) -> (Self, AtlasStats) {
        pack_growing(tiles, initial_size, max_size, 1, PackingAlgorithm::Shelf)
    }

    /// Incrementally add a single tile to the atlas without a full repack.
    ///
    /// Returns the [`UvRect`] of the inserted tile, or `None` if the atlas is full.
    ///
    /// Shelf state (`cursor_x`, `shelf_y`, `shelf_max_h`) is updated in-place so that
    /// subsequent calls continue packing from where the last one left off.
    pub fn add_tile(&mut self, tile: &SdfTile) -> Option<UvRect> {
        let pad = self.padding;
        let atlas_w = self.width;
        let atlas_h = self.height;

        // Try to start a new shelf if the tile doesn't fit horizontally.
        if self.cursor_x + tile.width > atlas_w.saturating_sub(pad) {
            self.cursor_x = pad;
            self.shelf_y += self.shelf_max_h + pad;
            self.shelf_max_h = 0;
        }

        // Return None if the tile would overflow the atlas height.
        if self.shelf_y + tile.height > atlas_h.saturating_sub(pad) {
            return None;
        }

        let cx = self.cursor_x;
        let cy = self.shelf_y;

        // Blit tile data into the existing texture.
        for y in 0..tile.height {
            for x in 0..tile.width {
                let src_idx = (y * tile.width + x) as usize;
                let dst_idx = ((cy + y) * atlas_w + (cx + x)) as usize;
                if dst_idx < self.texture.len() && src_idx < tile.data.len() {
                    self.texture[dst_idx] = tile.data[src_idx];
                }
            }
        }

        let uv = UvRect {
            u_min: cx as f32 / atlas_w as f32,
            v_min: cy as f32 / atlas_h as f32,
            u_max: (cx + tile.width) as f32 / atlas_w as f32,
            v_max: (cy + tile.height) as f32 / atlas_h as f32,
        };

        self.uv_map.insert(tile.glyph_id, uv.clone());
        self.cursor_x += tile.width + pad;
        self.shelf_max_h = self.shelf_max_h.max(tile.height);

        Some(uv)
    }

    /// Soft-delete a tile from the atlas by glyph ID.
    ///
    /// Removes the UV mapping so the glyph will no longer be found in the atlas.
    /// The texture pixels are *not* cleared or reclaimed; the space is "wasted"
    /// until a full repack is performed.
    ///
    /// Returns `true` if the glyph was present and has been removed.
    pub fn remove_tile(&mut self, glyph_id: u16) -> bool {
        self.uv_map.remove(&glyph_id).is_some()
    }

    // ─── Binary serialization ─────────────────────────────────────────────────

    /// Serialise the atlas to a compact binary representation.
    ///
    /// # Format (little-endian throughout)
    /// ```text
    /// [0..4]   magic:       b"SDFA"
    /// [4..8]   version:     u32 = 1
    /// [8..12]  atlas_w:     u32
    /// [12..16] atlas_h:     u32
    /// [16..20] num_entries: u32
    /// [20 + i*28 .. 20 + (i+1)*28] per entry:
    ///     glyph_id:   u16  (2 bytes)
    ///     _pad:       u16  (2 bytes, zeroed)
    ///     uv_u_min:   u32  (f32 bits)
    ///     uv_v_min:   u32  (f32 bits)
    ///     uv_u_max:   u32  (f32 bits)
    ///     uv_v_max:   u32  (f32 bits)
    ///     _reserved:  u64  (8 bytes, zeroed)
    /// [20 + num_entries*28 ..] texture bytes (atlas_w * atlas_h bytes)
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        let num_entries = self.uv_map.len() as u32;
        let texture_len = self.width as usize * self.height as usize;
        let total = ENTRIES_OFFSET + num_entries as usize * ENTRY_SIZE + texture_len;
        let mut buf = Vec::with_capacity(total);

        // Header
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&VERSION.to_le_bytes());
        buf.extend_from_slice(&self.width.to_le_bytes());
        buf.extend_from_slice(&self.height.to_le_bytes());
        buf.extend_from_slice(&num_entries.to_le_bytes());

        // Entries — sort by glyph_id for deterministic output.
        let mut entries: Vec<(&u16, &UvRect)> = self.uv_map.iter().collect();
        entries.sort_by_key(|(gid, _)| *gid);

        for (glyph_id, uv) in entries {
            buf.extend_from_slice(&glyph_id.to_le_bytes()); // 2 bytes
            buf.extend_from_slice(&0u16.to_le_bytes()); // 2 bytes pad
            buf.extend_from_slice(&uv.u_min.to_bits().to_le_bytes()); // 4 bytes
            buf.extend_from_slice(&uv.v_min.to_bits().to_le_bytes()); // 4 bytes
            buf.extend_from_slice(&uv.u_max.to_bits().to_le_bytes()); // 4 bytes
            buf.extend_from_slice(&uv.v_max.to_bits().to_le_bytes()); // 4 bytes
            buf.extend_from_slice(&0u64.to_le_bytes()); // 8 bytes reserved
        }

        // Texture
        buf.extend_from_slice(&self.texture);

        buf
    }

    /// Deserialise an atlas from the binary format produced by [`SdfAtlas::to_bytes`].
    ///
    /// # Errors
    /// Returns [`SdfError::InvalidData`] if:
    /// - The magic bytes are wrong.
    /// - The version is not `1`.
    /// - The buffer is too short for the declared number of entries and texture.
    pub fn from_bytes(data: &[u8]) -> Result<Self, SdfError> {
        // Need at least the fixed header.
        if data.len() < ENTRIES_OFFSET {
            return Err(SdfError::InvalidData(format!(
                "buffer too short: need at least {ENTRIES_OFFSET} bytes, got {}",
                data.len()
            )));
        }

        // Magic
        if &data[0..4] != MAGIC {
            return Err(SdfError::InvalidData(format!(
                "bad magic: expected {:?}, got {:?}",
                MAGIC,
                &data[0..4]
            )));
        }

        // Version
        let version = u32::from_le_bytes(
            data[4..8]
                .try_into()
                .map_err(|_| SdfError::InvalidData("cannot read version".into()))?,
        );
        if version != VERSION {
            return Err(SdfError::InvalidData(format!(
                "unsupported version {version}, expected {VERSION}"
            )));
        }

        let atlas_w = u32::from_le_bytes(
            data[8..12]
                .try_into()
                .map_err(|_| SdfError::InvalidData("cannot read atlas_w".into()))?,
        );
        let atlas_h = u32::from_le_bytes(
            data[12..16]
                .try_into()
                .map_err(|_| SdfError::InvalidData("cannot read atlas_h".into()))?,
        );
        let num_entries = u32::from_le_bytes(
            data[16..20]
                .try_into()
                .map_err(|_| SdfError::InvalidData("cannot read num_entries".into()))?,
        ) as usize;

        // Validate total length. All three components come straight from an
        // untrusted header, so compute the sum with checked arithmetic: a
        // wraparound here would shrink `expected_len` below the real
        // requirement and let the length guard below pass, leading to an
        // out-of-bounds slice panic further down.
        let texture_len = (atlas_w as usize)
            .checked_mul(atlas_h as usize)
            .ok_or_else(|| SdfError::InvalidData("atlas_w * atlas_h overflows".into()))?;
        let entries_len = num_entries
            .checked_mul(ENTRY_SIZE)
            .ok_or_else(|| SdfError::InvalidData("num_entries * entry size overflows".into()))?;
        let expected_len = ENTRIES_OFFSET
            .checked_add(entries_len)
            .and_then(|len| len.checked_add(texture_len))
            .ok_or_else(|| SdfError::InvalidData("declared atlas size overflows".into()))?;
        if data.len() < expected_len {
            return Err(SdfError::InvalidData(format!(
                "buffer too short: expected {expected_len} bytes, got {}",
                data.len()
            )));
        }

        // Read UV map entries.
        let mut uv_map: HashMap<u16, UvRect> = HashMap::with_capacity(num_entries);
        for i in 0..num_entries {
            let base = ENTRIES_OFFSET + i * ENTRY_SIZE;
            let glyph_id = u16::from_le_bytes(
                data[base..base + 2]
                    .try_into()
                    .map_err(|_| SdfError::InvalidData(format!("entry {i}: bad glyph_id")))?,
            );
            // Skip 2-byte pad at base+2..base+4
            let u_min = f32::from_bits(u32::from_le_bytes(
                data[base + 4..base + 8]
                    .try_into()
                    .map_err(|_| SdfError::InvalidData(format!("entry {i}: bad u_min")))?,
            ));
            let v_min = f32::from_bits(u32::from_le_bytes(
                data[base + 8..base + 12]
                    .try_into()
                    .map_err(|_| SdfError::InvalidData(format!("entry {i}: bad v_min")))?,
            ));
            let u_max = f32::from_bits(u32::from_le_bytes(
                data[base + 12..base + 16]
                    .try_into()
                    .map_err(|_| SdfError::InvalidData(format!("entry {i}: bad u_max")))?,
            ));
            let v_max = f32::from_bits(u32::from_le_bytes(
                data[base + 16..base + 20]
                    .try_into()
                    .map_err(|_| SdfError::InvalidData(format!("entry {i}: bad v_max")))?,
            ));
            // Skip 8-byte reserved at base+20..base+28
            uv_map.insert(
                glyph_id,
                UvRect {
                    u_min,
                    v_min,
                    u_max,
                    v_max,
                },
            );
        }

        // Read texture. `entries_len` and `texture_len` were already validated
        // above (via `expected_len`) not to overflow and to fit within `data`.
        let tex_start = ENTRIES_OFFSET + entries_len;
        let texture = data[tex_start..tex_start + texture_len].to_vec();

        Ok(Self {
            width: atlas_w,
            height: atlas_h,
            texture,
            uv_map,
            cursor_x: 0,
            shelf_y: 0,
            shelf_max_h: 0,
            padding: 0,
        })
    }

    /// Construct an [`SdfAtlas`] from statically-embedded bytes, e.g. produced by `include_bytes!`.
    ///
    /// This is the companion to [`SdfAtlas::to_bytes`] for build-time atlas pre-computation.
    /// The `'static` annotation signals to callers that this is intended for embedded data;
    /// internally it delegates to [`SdfAtlas::from_bytes`] since only the parsed structure is needed.
    ///
    /// # Example (in a build-generated file)
    /// ```rust,ignore
    /// static ATLAS_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/font_atlas.bin"));
    /// let atlas = SdfAtlas::from_static(ATLAS_BYTES).expect("parse atlas");
    /// ```
    ///
    /// # Errors
    /// Propagates any [`SdfError::InvalidData`] returned by [`SdfAtlas::from_bytes`].
    pub fn from_static(data: &'static [u8]) -> Result<Self, SdfError> {
        Self::from_bytes(data)
    }

    /// Export the atlas texture as a PNG file for visualization.
    ///
    /// Each pixel value is the SDF distance encoded as a greyscale byte
    /// (`0` = far outside, `128` = boundary, `255` = far inside).
    ///
    /// # Errors
    /// Returns [`SdfError::Io`] if the texture length does not match
    /// `width × height`, if PNG encoding fails, or if the file cannot be
    /// written.
    pub fn export_png(&self, path: &std::path::Path) -> Result<(), SdfError> {
        let bytes = encode_png(
            self.width,
            self.height,
            PngColorType::Grayscale8,
            &self.texture,
        )
        .map_err(|e| SdfError::Io(e.to_string()))?;
        std::fs::write(path, bytes).map_err(|e| SdfError::Io(e.to_string()))
    }
}

// ─── MSDF atlas ───────────────────────────────────────────────────────────────

/// A packed atlas of MSDF glyph tiles, ready for GPU upload.
///
/// The atlas texture is stored as an RGB byte slice (`texture`), three bytes
/// per pixel. Each pixel encodes signed distances in the R, G, and B channels
/// for the corresponding colored edge of the glyph outline.
pub struct MsdfAtlas {
    /// Atlas width in pixels.
    pub width: u32,
    /// Atlas height in pixels.
    pub height: u32,
    /// Raw RGB texture data: `width * height * 3` bytes.
    pub texture: Vec<u8>,
    /// UV coordinates for each glyph ID within the atlas.
    pub uv_map: HashMap<u16, UvRect>,
}

impl MsdfAtlas {
    /// Pack a set of MSDF tiles into a fixed-size atlas texture.
    ///
    /// Uses the same left-to-right shelf-packing algorithm as [`SdfAtlas`],
    /// adapted for 3-bytes-per-pixel (RGB) tiles.
    ///
    /// If `tiles` is empty, returns a minimal 1×1 atlas.
    pub fn pack(tiles: &[crate::msdf::MsdfTile], atlas_size: u32) -> Self {
        if tiles.is_empty() {
            return Self {
                width: 1,
                height: 1,
                texture: vec![0u8; 3],
                uv_map: HashMap::new(),
            };
        }

        let atlas_w = atlas_size.next_power_of_two().max(64);
        let atlas_h = atlas_size.next_power_of_two().max(64);
        let tex_len = atlas_w as usize * atlas_h as usize * 3;
        let mut texture = vec![0u8; tex_len];
        let mut uv_map = HashMap::new();

        let mut cx = 0u32;
        let mut cy = 0u32;
        let mut row_h = 0u32;

        for tile in tiles {
            // Start a new shelf when the current tile would exceed atlas width.
            if cx + tile.width > atlas_w {
                cx = 0;
                cy += row_h;
                row_h = 0;
            }

            // Skip tiles that would overflow the atlas height.
            if cy + tile.height > atlas_h {
                continue;
            }

            // Blit 3-channel tile data into the atlas.
            for y in 0..tile.height {
                for x in 0..tile.width {
                    let src_base = (y * tile.width + x) as usize * 3;
                    let dst_base = ((cy + y) * atlas_w + (cx + x)) as usize * 3;
                    if dst_base + 2 < texture.len() && src_base + 2 < tile.data.len() {
                        texture[dst_base] = tile.data[src_base];
                        texture[dst_base + 1] = tile.data[src_base + 1];
                        texture[dst_base + 2] = tile.data[src_base + 2];
                    }
                }
            }

            uv_map.insert(
                tile.glyph_id,
                UvRect {
                    u_min: cx as f32 / atlas_w as f32,
                    v_min: cy as f32 / atlas_h as f32,
                    u_max: (cx + tile.width) as f32 / atlas_w as f32,
                    v_max: (cy + tile.height) as f32 / atlas_h as f32,
                },
            );

            cx += tile.width;
            row_h = row_h.max(tile.height);
        }

        Self {
            width: atlas_w,
            height: atlas_h,
            texture,
            uv_map,
        }
    }

    /// Export the atlas texture as a PNG file for visualization.
    ///
    /// The texture is stored as RGB (3 bytes per pixel). Each channel encodes
    /// a signed distance for the corresponding colored edge of the glyph outline.
    ///
    /// # Errors
    /// Returns [`SdfError::Io`] if the texture length does not match
    /// `width × height × 3`, if PNG encoding fails, or if the file cannot be
    /// written.
    pub fn export_png(&self, path: &std::path::Path) -> Result<(), SdfError> {
        let bytes = encode_png(self.width, self.height, PngColorType::Rgb8, &self.texture)
            .map_err(|e| SdfError::Io(e.to_string()))?;
        std::fs::write(path, bytes).map_err(|e| SdfError::Io(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msdf::MsdfTile;

    fn make_sdf_tile(glyph_id: u16, w: u32, h: u32) -> SdfTile {
        SdfTile {
            glyph_id,
            width: w,
            height: h,
            data: vec![128u8; (w * h) as usize],
            bearing_x: 0,
            bearing_y: 0,
            advance_x: w as f32,
        }
    }

    #[test]
    fn msdf_atlas_packs_many_tiles() {
        let tiles: Vec<MsdfTile> = (0..20u16)
            .map(|i| MsdfTile {
                glyph_id: i,
                width: 16,
                height: 16,
                data: vec![128u8; 16 * 16 * 3],
                bearing_x: 0.0,
                bearing_y: 0.0,
                advance_x: 16.0,
            })
            .collect();
        let atlas = MsdfAtlas::pack(&tiles, 256);
        assert_eq!(atlas.uv_map.len(), 20);
        for uv in atlas.uv_map.values() {
            assert!(uv.u_min >= 0.0 && uv.u_max <= 1.0);
            assert!(uv.v_min >= 0.0 && uv.v_max <= 1.0);
        }
    }

    #[test]
    fn pack_with_options_returns_stats() {
        let tiles: Vec<SdfTile> = (0..10u16).map(|i| make_sdf_tile(i, 16, 16)).collect();
        let opts = AtlasOptions {
            atlas_size: 128,
            padding: 1,
            ..Default::default()
        };
        let (atlas, stats) = SdfAtlas::pack_with_options(&tiles, &opts);
        assert_eq!(stats.tiles_packed + stats.tiles_dropped, 10);
        assert!(
            stats.utilization > 0.0 && stats.utilization <= 1.0,
            "utilization out of range: {}",
            stats.utilization
        );
        assert_eq!(atlas.uv_map.len(), stats.tiles_packed);
    }

    #[test]
    fn remove_tile_removes_from_map() {
        let tiles = vec![make_sdf_tile(42, 16, 16)];
        let mut atlas = SdfAtlas::pack(&tiles);
        assert!(atlas.uv_map.contains_key(&42));
        assert!(atlas.remove_tile(42));
        assert!(!atlas.uv_map.contains_key(&42));
        // Second remove must return false.
        assert!(!atlas.remove_tile(42));
    }

    #[test]
    fn add_tile_places_new_tile() {
        let tiles: Vec<SdfTile> = (0..4u16).map(|i| make_sdf_tile(i, 16, 16)).collect();
        let opts = AtlasOptions {
            atlas_size: 128,
            padding: 0,
            ..Default::default()
        };
        let (mut atlas, _) = SdfAtlas::pack_with_options(&tiles, &opts);
        let new_tile = make_sdf_tile(99, 16, 16);
        let uv = atlas.add_tile(&new_tile);
        assert!(uv.is_some(), "expected tile to be placed");
        assert!(atlas.uv_map.contains_key(&99));
    }

    #[test]
    fn from_coverage_basic() {
        // Fully-filled 8×8 square: SDF should have center > 128.
        let coverage = vec![1.0f32; 8 * 8];
        let tile =
            SdfTile::from_coverage(7, &coverage, 8, 8, 4.0, 0, 0, 8.0).expect("from_coverage");
        assert_eq!(tile.glyph_id, 7);
        assert_eq!(tile.width, 8);
        assert_eq!(tile.height, 8);
        let center = tile.data[4 * 8 + 4];
        assert!(
            center > 128,
            "center of solid square should be inside, got {center}"
        );
    }

    #[test]
    fn from_coverage_zero_size_errors() {
        let cov = vec![1.0f32; 0];
        assert!(SdfTile::from_coverage(0, &cov, 0, 8, 4.0, 0, 0, 0.0).is_err());
        assert!(SdfTile::from_coverage(0, &cov, 8, 0, 4.0, 0, 0, 0.0).is_err());
    }

    #[test]
    fn test_maxrects_non_overlapping() {
        let tiles: Vec<SdfTile> = (0..20u16)
            .map(|id| SdfTile {
                glyph_id: id,
                width: 16,
                height: 16,
                data: vec![128u8; 256],
                bearing_x: 0,
                bearing_y: 0,
                advance_x: 16.0,
            })
            .collect();
        let options = AtlasOptions {
            atlas_size: 128,
            padding: 1,
            algorithm: PackingAlgorithm::MaxRects,
            ..Default::default()
        };
        let (atlas, stats) = SdfAtlas::pack_with_options(&tiles, &options);
        // Verify no two UV rects overlap.
        let uvs: Vec<_> = atlas.uv_map.values().collect();
        for i in 0..uvs.len() {
            for j in (i + 1)..uvs.len() {
                let a = uvs[i];
                let b = uvs[j];
                let overlap = a.u_min < b.u_max
                    && a.u_max > b.u_min
                    && a.v_min < b.v_max
                    && a.v_max > b.v_min;
                assert!(!overlap, "UV rects {:?} and {:?} overlap", a, b);
            }
        }
        let _ = stats;
    }

    #[test]
    fn test_growing_pack_packs_all() {
        let tiles: Vec<SdfTile> = (0..50u16)
            .map(|id| SdfTile {
                glyph_id: id,
                width: 32,
                height: 32,
                data: vec![128u8; 32 * 32],
                bearing_x: 0,
                bearing_y: 0,
                advance_x: 32.0,
            })
            .collect();
        let (atlas, stats) = pack_growing(&tiles, 64, 1024, 1, PackingAlgorithm::Shelf);
        assert_eq!(stats.tiles_dropped, 0, "all tiles should fit after growing");
        assert_eq!(atlas.uv_map.len(), 50);
    }

    #[test]
    fn test_sdf_atlas_serialization_roundtrip() {
        let tiles: Vec<SdfTile> = (0..5u16)
            .map(|id| SdfTile {
                glyph_id: id,
                width: 8,
                height: 8,
                data: vec![id as u8; 64],
                bearing_x: 0,
                bearing_y: 0,
                advance_x: 8.0,
            })
            .collect();
        let (atlas, _) = SdfAtlas::pack_with_options(
            &tiles,
            &AtlasOptions {
                atlas_size: 64,
                padding: 0,
                ..Default::default()
            },
        );
        let bytes = atlas.to_bytes();
        let restored = SdfAtlas::from_bytes(&bytes).expect("deserialization");
        assert_eq!(restored.width, atlas.width);
        assert_eq!(restored.height, atlas.height);
        assert_eq!(restored.uv_map.len(), atlas.uv_map.len());
        for (gid, uv) in &atlas.uv_map {
            let r = &restored.uv_map[gid];
            assert!((r.u_min - uv.u_min).abs() < 1e-5);
            assert!((r.u_max - uv.u_max).abs() < 1e-5);
        }
    }

    #[test]
    fn test_export_png_produces_valid_file() {
        use std::env::temp_dir;
        let tmp = temp_dir().join("test_sdf_atlas.png");
        let mut atlas = SdfAtlas::new(4, 4);
        atlas.texture = vec![128u8; 16];
        atlas.export_png(&tmp).expect("export should succeed");
        assert!(tmp.exists());
        assert!(tmp.metadata().expect("metadata").len() > 0);
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_msdf_export_png_produces_valid_file() {
        use std::env::temp_dir;
        let tmp = temp_dir().join("test_msdf_atlas.png");
        let atlas = MsdfAtlas {
            width: 2,
            height: 2,
            texture: vec![128u8; 2 * 2 * 3],
            uv_map: Default::default(),
        };
        atlas.export_png(&tmp).expect("msdf export should succeed");
        assert!(tmp.exists());
        assert!(tmp.metadata().expect("metadata").len() > 0);
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_pre_allocated_atlas_capacity() {
        let atlas = SdfAtlas::with_capacity(256, 256, 100, 64);
        assert_eq!(atlas.width, 256);
        assert_eq!(atlas.height, 256);
        assert!(atlas.texture.capacity() >= atlas.texture.len());
    }

    #[test]
    fn from_bytes_rejects_overflowing_header_without_panicking() {
        // Regression test: `expected_len = ENTRIES_OFFSET + num_entries *
        // ENTRY_SIZE + texture_len` is computed from three untrusted header
        // fields. With plain `usize` arithmetic this can overflow, wrap
        // around to a small value, sail past the `data.len() < expected_len`
        // guard, and later panic on an out-of-bounds slice (or, in a debug
        // build, panic immediately on the overflowing addition). It must
        // instead be rejected cleanly via `SdfError::InvalidData`.
        let mut data = Vec::with_capacity(ENTRIES_OFFSET);
        data.extend_from_slice(MAGIC); // magic
        data.extend_from_slice(&VERSION.to_le_bytes()); // version
        data.extend_from_slice(&u32::MAX.to_le_bytes()); // atlas_w
        data.extend_from_slice(&u32::MAX.to_le_bytes()); // atlas_h
        data.extend_from_slice(&u32::MAX.to_le_bytes()); // num_entries
        assert_eq!(data.len(), ENTRIES_OFFSET);

        match SdfAtlas::from_bytes(&data) {
            Err(SdfError::InvalidData(_)) => {}
            Err(other) => panic!("expected InvalidData, got a different SdfError: {other}"),
            Ok(_) => panic!("overflowing header must not be accepted"),
        }
    }

    #[test]
    fn test_sdf_atlas_new_zeroed() {
        let atlas = SdfAtlas::new(8, 8);
        assert_eq!(atlas.width, 8);
        assert_eq!(atlas.height, 8);
        assert_eq!(atlas.texture.len(), 64);
        assert!(atlas.texture.iter().all(|&b| b == 0));
        assert!(atlas.uv_map.is_empty());
    }

    #[test]
    fn test_from_static_roundtrip() {
        // Create a minimal atlas, serialize, then deserialize via from_static.
        // Box::leak gives us a 'static lifetime, simulating include_bytes! semantics.
        let atlas = SdfAtlas::new(64, 64);
        let bytes = atlas.to_bytes();
        let static_bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let roundtrip = SdfAtlas::from_static(static_bytes).expect("from_static should succeed");
        assert_eq!(roundtrip.width, 64);
        assert_eq!(roundtrip.height, 64);
        assert_eq!(roundtrip.uv_map.len(), 0);
    }
}
