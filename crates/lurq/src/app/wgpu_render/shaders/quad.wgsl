// Rounded-corner quad pipeline (elliptical, optional ring).
//
// Vertex buffer 0 (per-vertex):  unit quad corner in [0,1]^2.
// Vertex buffer 1 (per-instance): pos / size in pixels, linear RGBA color,
//                                 horizontal corner radii (TL, TR, BR, BL),
//                                 vertical corner radii (same order),
//                                 per-side ring thickness (top, right,
//                                 bottom, left).
// Bind group 0 / binding 0: viewport size in pixels.
//
// Two modes selected by whether any stroke component is > 0:
//   - Filled:    paint the entire (rounded) box with `color`.
//   - Stroked:   paint only the ring between the outer rounded box and
//                an inner rounded box inset on each side by the matching
//                stroke width.
// In both modes a ~1-pixel anti-alias band keeps edges smooth.
//
// Box-shadow instances (no stroke, `pattern.x` = 1 outer / 2 inset) paint a
// Gaussian-blurred rounded rect instead; see `box_shadow_alpha`. For them
// `radii_h` are the element box's radii, `radii_v` the shadow shape's,
// `pattern.yz` the shape offset, `pattern.w` the spread and `shadow_sigma`
// the Gaussian's standard deviation.
//
// The corner zone uses a gradient-corrected ellipse SDF so corners can
// be elliptical (h != v); when h == v it reduces to the usual circular
// case.

// `Globals` is a per-clip-range uniform block — `prepare` writes one
// entry per `DisplayList::clips` slot and `record` rebinds it via a
// dynamic offset. Vertex stage reads `viewport.xy` for NDC mapping;
// fragment stage uses `clip_*` to discard fragments outside the
// active rounded clip.
struct Globals {
    viewport:     vec4<f32>,
    clip_rect:    vec4<f32>,   // x, y, w, h
    clip_radii_h: vec4<f32>,
    clip_radii_v: vec4<f32>,
    clip_active:  vec4<f32>,   // x = 1.0 to enable SDF discard
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<storage, read> gradients: array<vec4<f32>>;

struct VsIn {
    @location(0) corner:     vec2<f32>,
    @location(1) pos:        vec2<f32>,
    @location(2) size:       vec2<f32>,
    @location(3) color:      vec4<f32>,
    @location(4) radii_h:    vec4<f32>,  // TL, TR, BR, BL
    @location(5) radii_v:    vec4<f32>,  // TL, TR, BR, BL
    @location(6) stroke:     vec4<f32>,  // top, right, bottom, left
    @location(7) pattern:    vec4<f32>,  // kind, dash, gap, _
    @location(8) transform:     vec4<f32>,  // 2x2 matrix: a, b, c, d
    @location(9) xf_origin:    vec2<f32>,  // transform origin relative to rect top-left
    @location(10) shadow_sigma: f32,
    @location(11) gradient_offset: f32,
};

struct VsOut {
    @builtin(position) clip:      vec4<f32>,
    @location(0)       color:     vec4<f32>,
    /// Pixel offset from the box's centre.
    @location(1)       local:     vec2<f32>,
    @location(2)       half_size: vec2<f32>,
    @location(3)       radii_h:   vec4<f32>,
    @location(4)       radii_v:   vec4<f32>,
    @location(5)       stroke:       vec4<f32>,
    @location(6)       pattern:      vec4<f32>,
    @location(7)       shadow_sigma: f32,
    @location(8)       gradient_offset: f32,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    // Local position within the quad (before transform). The mesh is
    // expanded slightly so transformed edges have fragments on both
    // sides of the SDF boundary; otherwise the rasterizer clips away
    // the outer half of the AA band.
    var aa_outset = 2.0;
    let max_stroke = max(max(in.stroke.x, in.stroke.y), max(in.stroke.z, in.stroke.w));
    if (max_stroke <= 0.0 && in.pattern.x > 0.5 && in.pattern.x < 1.5) {
        // An outer shadow paints beyond its element box.
        aa_outset = aa_outset + max(in.pattern.w, 0.0) + max(abs(in.pattern.y), abs(in.pattern.z))
            + SHADOW_EXTENT_SIGMAS * in.shadow_sigma;
    }
    let local_px = in.corner * (in.size + vec2<f32>(aa_outset * 2.0, aa_outset * 2.0))
                 - vec2<f32>(aa_outset, aa_outset);
    // Apply 2x2 transform around xf_origin.
    let centered = local_px - in.xf_origin;
    let rotated = vec2<f32>(
        in.transform.x * centered.x + in.transform.z * centered.y,
        in.transform.y * centered.x + in.transform.w * centered.y,
    );
    let px = in.pos + rotated + in.xf_origin;

    // `viewport.zw` is the target's origin in window pixels (non-zero when
    // painting into an opacity layer).
    let viewport = globals.viewport.xy;
    let target_px = px - globals.viewport.zw;
    let ndc = vec2<f32>(
        (target_px.x / viewport.x) * 2.0 - 1.0,
        1.0 - (target_px.y / viewport.y) * 2.0,
    );

    var out: VsOut;
    out.clip      = vec4<f32>(ndc, 0.0, 1.0);
    out.color     = in.color;
    out.half_size = in.size * 0.5;
    // SDF local coords stay in un-rotated quad space.
    out.local     = local_px - in.size * 0.5;
    out.radii_h   = in.radii_h;
    out.radii_v   = in.radii_v;
    out.stroke       = in.stroke;
    out.pattern      = in.pattern;
    out.shadow_sigma     = in.shadow_sigma;
    out.gradient_offset  = in.gradient_offset;
    return out;
}

// Strokes. The same formulas as `layout/stroke.rs`, which describes the
// shape, and `quad.hlsl`; keep the three in step.

/// How far the hole of a stroke reaches past an edge whose side has no
/// stroke, so the ring has no coverage there.
const HOLE_MARGIN: f32 = 1.0;

fn stroke_inset(width: f32) -> f32 {
    if (width > 0.0) { return width; }
    return -HOLE_MARGIN;
}

/// How much nearer the second of two edges that meet at a corner has to be
/// for a point to be taken as that edge's.
const CORNER_BIAS: f32 = 0.01;

/// Whether a point `first` away from one edge of a corner and `second` from
/// the other is taken as the first edge's: the nearer one, and always the one
/// that has a stroke where the other has none.
fn first_edge(first: f32, second: f32, first_width: f32, second_width: f32) -> bool {
    if (second_width <= 0.0) { return true; }
    if (first_width <= 0.0) { return false; }
    return first <= second + CORNER_BIAS;
}

/// Where along the rect's outline the part of it nearest `p` is, in pixels
/// clockwise from the start of the top edge (`.x`), and the outline's whole
/// length (`.y`). `radii` are circular, in the order TL, TR, BR, BL; `stroke`
/// is the widths of the sides (top, right, bottom, left).
fn outline_position(p: vec2<f32>, hs: vec2<f32>, radii: vec4<f32>, stroke: vec4<f32>) -> vec2<f32> {
    let limit = max(min(hs.x, hs.y), 0.0);
    let r = clamp(radii, vec4<f32>(0.0), vec4<f32>(limit));
    let quarter = 1.57079632679;
    let top = max(2.0 * hs.x - r.x - r.y, 0.0);
    let right = max(2.0 * hs.y - r.y - r.z, 0.0);
    let bottom = max(2.0 * hs.x - r.z - r.w, 0.0);
    let left = max(2.0 * hs.y - r.w - r.x, 0.0);
    // Where each part of the outline starts.
    let right_arc = top;
    let right_edge = right_arc + quarter * r.y;
    let bottom_arc = right_edge + right;
    let bottom_edge = bottom_arc + quarter * r.z;
    let left_arc = bottom_edge + bottom;
    let left_edge = left_arc + quarter * r.w;
    let top_arc = left_edge + left;
    let total = top_arc + quarter * r.x;

    let on_top = clamp(p.x + hs.x - r.x, 0.0, top);
    let on_right = right_edge + clamp(p.y + hs.y - r.y, 0.0, right);
    let on_bottom = bottom_edge + clamp(hs.x - r.z - p.x, 0.0, bottom);
    let on_left = left_edge + clamp(hs.y - r.w - p.y, 0.0, left);
    var position: f32;
    if (p.y < 0.0) {
        if (p.x < 0.0) {
            let c = vec2<f32>(-hs.x + r.x, -hs.y + r.x);
            if (p.x < c.x && p.y < c.y) {
                position = top_arc + atan2(c.y - p.y, c.x - p.x) * r.x;
            } else if (first_edge(p.x + hs.x, p.y + hs.y, stroke.w, stroke.x)) {
                position = on_left;
            } else {
                position = on_top;
            }
        } else {
            let c = vec2<f32>(hs.x - r.y, -hs.y + r.y);
            if (p.x > c.x && p.y < c.y) {
                position = right_arc + atan2(p.x - c.x, c.y - p.y) * r.y;
            } else if (first_edge(p.y + hs.y, hs.x - p.x, stroke.x, stroke.y)) {
                position = on_top;
            } else {
                position = on_right;
            }
        }
    } else {
        if (p.x < 0.0) {
            let c = vec2<f32>(-hs.x + r.w, hs.y - r.w);
            if (p.x < c.x && p.y > c.y) {
                position = left_arc + atan2(c.x - p.x, p.y - c.y) * r.w;
            } else if (first_edge(hs.y - p.y, p.x + hs.x, stroke.z, stroke.w)) {
                position = on_bottom;
            } else {
                position = on_left;
            }
        } else {
            let c = vec2<f32>(hs.x - r.z, hs.y - r.z);
            if (p.x > c.x && p.y > c.y) {
                position = bottom_arc + atan2(p.y - c.y, p.x - c.x) * r.z;
            } else if (first_edge(hs.x - p.x, hs.y - p.y, stroke.y, stroke.z)) {
                position = on_right;
            } else {
                position = on_bottom;
            }
        }
    }
    return vec2<f32>(position, total);
}

/// How much of a dashed stroke shows at `position` along an outline of
/// length `total`: 1 in a dash, 0 in a gap, a ramp of a pixel between. The
/// period is fitted to the outline a whole number of times.
fn dash_coverage(position: f32, total: f32, dash: f32, gap: f32) -> f32 {
    if (dash <= 0.0 || gap <= 0.0 || total <= 0.0) {
        return 1.0;
    }
    let period = dash + gap;
    let fitted = total / max(floor(total / period + 0.5), 1.0);
    let on = dash * fitted / period;
    let phase = position - floor(position / fitted) * fitted;
    var d: f32;
    if (phase <= on) {
        d = -min(phase, on - phase);
    } else {
        d = min(phase - on, fitted - phase);
    }
    return clamp(0.5 - d, 0.0, 1.0);
}

/// Pick the (h, v) radius pair for whichever quadrant `p` lies in.
/// Order: TL, TR, BR, BL (matches CSS `border-radius` longhand order).
fn pick_radius(p: vec2<f32>, rh: vec4<f32>, rv: vec4<f32>) -> vec2<f32> {
    if (p.y < 0.0) {
        if (p.x < 0.0) { return vec2<f32>(rh.x, rv.x); } // TL
        else           { return vec2<f32>(rh.y, rv.y); } // TR
    } else {
        if (p.x < 0.0) { return vec2<f32>(rh.w, rv.w); } // BL
        else           { return vec2<f32>(rh.z, rv.z); } // BR
    }
}

/// SDF of a box with elliptical corners (radius `r = (rx, ry)` per
/// quadrant). `p` is centre-relative pixel coords; `half_size` is the
/// box half-extent.
///
/// - Corner zone (both q components > 0) → gradient-corrected ellipse
///   SDF. Reduces to the exact circular formula `length(q) - r` when
///   rx == ry; smooth approximation otherwise.
/// - Edge band / interior → rectangle distance with the corner radii
///   subtracted on each axis: `max(q.x - rx, q.y - ry)`.
fn sd_rounded_box(p: vec2<f32>, half_size: vec2<f32>, r: vec2<f32>) -> f32 {
    let safe_r = max(r, vec2<f32>(0.0, 0.0));
    let q = abs(p) - half_size + safe_r;

    if (q.x > 0.0 && q.y > 0.0) {
        // Corner zone.
        if (safe_r.x <= 0.0 || safe_r.y <= 0.0) {
            return max(q.x, q.y);
        }
        let pn = q / safe_r;
        let l = length(pn);
        if (l <= 1e-6) {
            return -min(safe_r.x, safe_r.y);
        }
        let g = max(length(pn / safe_r), 1e-6);
        // Euclidean distance estimate = (length(pn) - 1) / |grad|, where
        // |grad| = length(pn / r) / length(pn). The factor of `l` in the
        // numerator below absorbs the `length(pn)` in the denominator
        // of |grad|. Reduces to `length(q) - r` for circular corners.
        return (l - 1.0) * l / g;
    }

    // Edge band or interior. The `- safe_r.{x,y}` subtraction undoes the
    // `+ safe_r` we added when computing `q`, which keeps the corner
    // zone consistent with the rounded-box shape.
    return max(q.x - safe_r.x, q.y - safe_r.y);
}

fn aa_width(d: f32) -> f32 {
    return max(fwidth(d), 1.0);
}

/// Ramp width for one of the four half-pixel subsamples below. Each
/// subsample stands for a 0.5 px square, so its ramp is half a pixel wide:
/// a full-pixel ramp per subsample blurred every edge twice, leaving pixels
/// fully inside an axis-aligned edge at 87.5% coverage and bleeding 12.5%
/// into the pixel outside (a faint line against the clear colour at the
/// window edge).
fn subsample_aa_width(d: f32) -> f32 {
    return 0.5 * aa_width(d);
}

fn rounded_fill_alpha(local: vec2<f32>, half_size: vec2<f32>, radii_h: vec4<f32>, radii_v: vec4<f32>) -> f32 {
    let radius = pick_radius(local, radii_h, radii_v);
    let dist = sd_rounded_box(local, half_size, radius);
    return clamp(0.5 - dist / subsample_aa_width(dist), 0.0, 1.0);
}

/// The ring of a stroke: inside the outer edge and outside the hole.
/// `stroke` is the widths (top, right, bottom, left); `radii_at` says which
/// edge of the stroke the radii are the corners of (0 outer, 1 the hole's).
fn rounded_stroke_alpha(
    local: vec2<f32>,
    half_size: vec2<f32>,
    radii_h: vec4<f32>,
    radii_v: vec4<f32>,
    stroke: vec4<f32>,
    radii_at: f32,
) -> f32 {
    let widths = max(stroke, vec4<f32>(0.0));
    let radius = pick_radius(local, radii_h, radii_v);
    // The sides that meet at this quadrant's corner.
    var across = widths.y;
    if (local.x < 0.0) { across = widths.w; }
    var along = widths.z;
    if (local.y < 0.0) { along = widths.x; }

    var outer_r = vec2<f32>(0.0, 0.0);
    var hole_r = vec2<f32>(0.0, 0.0);
    if (radius.x > 0.0) {
        outer_r.x = radius.x + radii_at * across;
        hole_r.x = max(outer_r.x - stroke_inset(across), 0.0);
    }
    if (radius.y > 0.0) {
        outer_r.y = radius.y + radii_at * along;
        hole_r.y = max(outer_r.y - stroke_inset(along), 0.0);
    }
    let outer_dist = sd_rounded_box(local, half_size, outer_r);

    let top = stroke_inset(widths.x);
    let right = stroke_inset(widths.y);
    let bottom = stroke_inset(widths.z);
    let left = stroke_inset(widths.w);
    let hole_half = max(vec2<f32>(
        half_size.x - 0.5 * (left + right),
        half_size.y - 0.5 * (top + bottom),
    ), vec2<f32>(0.0, 0.0));
    let hole_centre = vec2<f32>(0.5 * (left - right), 0.5 * (top - bottom));
    let hole_dist = sd_rounded_box(local - hole_centre, hole_half, hole_r);
    let dist = max(outer_dist, -hole_dist);
    return clamp(0.5 - dist / subsample_aa_width(dist), 0.0, 1.0);
}

fn supersampled_fill_alpha(local: vec2<f32>, half_size: vec2<f32>, radii_h: vec4<f32>, radii_v: vec4<f32>) -> f32 {
    return (
        rounded_fill_alpha(local + vec2<f32>(-0.25, -0.25), half_size, radii_h, radii_v) +
        rounded_fill_alpha(local + vec2<f32>(0.25, -0.25), half_size, radii_h, radii_v) +
        rounded_fill_alpha(local + vec2<f32>(-0.25, 0.25), half_size, radii_h, radii_v) +
        rounded_fill_alpha(local + vec2<f32>(0.25, 0.25), half_size, radii_h, radii_v)
    ) * 0.25;
}

fn supersampled_stroke_alpha(
    local: vec2<f32>,
    half_size: vec2<f32>,
    radii_h: vec4<f32>,
    radii_v: vec4<f32>,
    stroke: vec4<f32>,
    radii_at: f32,
) -> f32 {
    return (
        rounded_stroke_alpha(local + vec2<f32>(-0.25, -0.25), half_size, radii_h, radii_v, stroke, radii_at) +
        rounded_stroke_alpha(local + vec2<f32>(0.25, -0.25), half_size, radii_h, radii_v, stroke, radii_at) +
        rounded_stroke_alpha(local + vec2<f32>(-0.25, 0.25), half_size, radii_h, radii_v, stroke, radii_at) +
        rounded_stroke_alpha(local + vec2<f32>(0.25, 0.25), half_size, radii_h, radii_v, stroke, radii_at)
    ) * 0.25;
}

// Box shadows. The same formula as `blurred_rounded_rect_coverage` in
// `layout/box_shadow.rs` and `quad.hlsl`; keep the three in step.
const SHADOW_Y_SAMPLES: i32 = 8;
const SHADOW_EXTENT_SIGMAS: f32 = 3.0;
const SHADOW_MIN_SIGMA: f32 = 0.1;

fn shadow_erf(x: vec2<f32>) -> vec2<f32> {
    let s = sign(x);
    let a = abs(x);
    var r = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    r = r * r;
    return s - s / (r * r);
}

fn shadow_gaussian(x: f32, sigma: f32) -> f32 {
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (2.5066283 * sigma);
}

/// A row of the rounded rect at height `y`, convolved along x with the
/// Gaussian: the difference of two `erf`s at the row's curved ends.
fn shadow_blur_along_x(x: f32, y: f32, sigma: f32, corner: f32, half: vec2<f32>) -> f32 {
    let delta = min(half.y - corner - abs(y), 0.0);
    let curved = half.x - corner + sqrt(max(corner * corner - delta * delta, 0.0));
    let integral = 0.5 + 0.5 * shadow_erf((vec2<f32>(x, x) + vec2<f32>(-curved, curved)) * (0.70710678 / sigma));
    return integral.y - integral.x;
}

/// Coverage of a rounded rect (half extent `half`, circular corner `radii`)
/// blurred by `sigma`, at centre-relative `p`; the y convolution is a
/// midpoint sum over +/- 3 sigma.
fn blurred_rounded_box(p: vec2<f32>, half: vec2<f32>, radii: vec4<f32>, sigma: f32) -> f32 {
    if (half.x <= 0.0 || half.y <= 0.0) {
        return 0.0;
    }
    let corner = max(min(pick_radius(p, radii, radii).x, min(half.x, half.y)), 0.0);
    let extent = SHADOW_EXTENT_SIGMAS * sigma;
    let start = clamp(-extent, p.y - half.y, p.y + half.y);
    let end = clamp(extent, p.y - half.y, p.y + half.y);
    let step = (end - start) / f32(SHADOW_Y_SAMPLES);
    var y = start + step * 0.5;
    var coverage = 0.0;
    for (var i: i32 = 0; i < SHADOW_Y_SAMPLES; i = i + 1) {
        coverage = coverage + shadow_blur_along_x(p.x, p.y - y, sigma, corner, half) * shadow_gaussian(y, sigma) * step;
        y = y + step;
    }
    return clamp(coverage, 0.0, 1.0);
}

fn shadow_shape_alpha(p: vec2<f32>, half: vec2<f32>, radii: vec4<f32>, sigma: f32) -> f32 {
    if (sigma < SHADOW_MIN_SIGMA) {
        if (half.x <= 0.0 || half.y <= 0.0) {
            return 0.0;
        }
        return supersampled_fill_alpha(p, half, radii, radii);
    }
    return blurred_rounded_box(p, half, radii, sigma);
}

/// An outer shadow covers its shape outside the element box; an inset one
/// covers the box outside its shape.
fn box_shadow_alpha(
    local: vec2<f32>,
    half_size: vec2<f32>,
    box_radii: vec4<f32>,
    shape_radii: vec4<f32>,
    params: vec4<f32>,
    sigma: f32,
) -> f32 {
    let offset = params.yz;
    let spread = params.w;
    let box_alpha = supersampled_fill_alpha(local, half_size, box_radii, box_radii);
    if (params.x < 1.5) {
        let shape_half = max(half_size + vec2<f32>(spread, spread), vec2<f32>(0.0, 0.0));
        return shadow_shape_alpha(local - offset, shape_half, shape_radii, sigma) * (1.0 - box_alpha);
    }
    let shape_half = max(half_size - vec2<f32>(spread, spread), vec2<f32>(0.0, 0.0));
    return box_alpha * (1.0 - shadow_shape_alpha(local - offset, shape_half, shape_radii, sigma));
}

/// Evaluate a CSS-like gradient for a fragment at centre-relative pixel
/// `local`, within a box of half-extent `half`. `off` is the vec4 index of
/// the gradient header in the `gradients` storage buffer. Layout:
///   [count, kind, flags, from_angle]
///   [dir.x, dir.y, center.x, center.y]
///   then per stop: [r, g, b, a], [position, _, _, _]
/// kind: 0 = linear, 1 = radial (flags bit0 = ellipse), 2 = conic.
fn sample_gradient(off: i32, local: vec2<f32>, half: vec2<f32>) -> vec4<f32> {
    let h0 = gradients[off];
    let h1 = gradients[off + 1];
    let count = i32(h0.x);
    let kind = i32(h0.y);
    let pi = 3.14159265359;

    var t: f32;
    if (kind == 0) {
        // Linear: project onto the unit direction, normalized by half the
        // gradient-line length so 0/1 reach the corners (CSS behaviour).
        let dir = h1.xy;
        let hl = abs(half.x * dir.x) + abs(half.y * dir.y);
        t = (dot(local, dir) + hl) / (2.0 * max(hl, 1e-5));
    } else if (kind == 1) {
        // Radial, farthest-corner.
        let center = (h1.zw * 2.0 - vec2<f32>(1.0, 1.0)) * half;
        if (h0.z > 0.5) {
            // Ellipse fitted to the box.
            let cn = h1.zw * 2.0 - vec2<f32>(1.0, 1.0);
            let p = (local - center) / max(half, vec2<f32>(1e-3, 1e-3));
            let radius = max(
                max(length(vec2<f32>(-1.0, -1.0) - cn), length(vec2<f32>(1.0, -1.0) - cn)),
                max(length(vec2<f32>(-1.0, 1.0) - cn), length(vec2<f32>(1.0, 1.0) - cn)),
            );
            t = length(p) / max(radius, 1e-5);
        } else {
            let radius = max(
                max(length(vec2<f32>(-half.x, -half.y) - center), length(vec2<f32>(half.x, -half.y) - center)),
                max(length(vec2<f32>(-half.x, half.y) - center), length(vec2<f32>(half.x, half.y) - center)),
            );
            t = length(local - center) / max(radius, 1e-5);
        }
    } else {
        // Conic: angle clockwise from the top.
        let center = (h1.zw * 2.0 - vec2<f32>(1.0, 1.0)) * half;
        let d = local - center;
        let ang = (atan2(d.x, -d.y) - h0.w) / (2.0 * pi);
        t = ang - floor(ang);
    }

    if (kind != 2) {
        t = clamp(t, 0.0, 1.0);
    }

    let stop_base = off + 2;
    let last = count - 1;
    var color = gradients[stop_base + 2 * last];
    for (var i: i32 = 0; i < last; i = i + 1) {
        let pb = gradients[stop_base + 2 * (i + 1) + 1].x;
        if (t <= pb) {
            let pa = gradients[stop_base + 2 * i + 1].x;
            let ca = gradients[stop_base + 2 * i];
            let cb = gradients[stop_base + 2 * (i + 1)];
            let span = max(pb - pa, 1e-5);
            color = mix(ca, cb, clamp((t - pa) / span, 0.0, 1.0));
            break;
        }
    }
    return color;
}

// The render target is not sRGB, so the fixed-function blend mixes
// sRGB-encoded values, as CSS and design tools do. `fs_main` works in linear
// light like the rest of the pipeline and encodes each result it returns.
fn encode_srgb(color: vec4<f32>) -> vec4<f32> {
    let c = clamp(color.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return vec4<f32>(select(high, low, c <= vec3<f32>(0.0031308)), color.a);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var clip_alpha = 1.0;
    // Rounded-clip discard. The active clip's rect + radii are
    // supplied by the per-range `Globals` uniform; we recover the
    // fragment's screen-space pixel position from the @builtin
    // (which the rasteriser fills as the un-normalised pixel
    // coord) and reuse the same SDF helper as the box itself uses
    // for its own corners.
    if (globals.clip_active.x > 0.5) {
        let frag_pos = in.clip.xy;
        let cr = globals.clip_rect;
        let half = vec2<f32>(cr.z, cr.w) * 0.5;
        let centre = vec2<f32>(cr.x + half.x, cr.y + half.y);
        let local_clip = frag_pos - centre;
        let r = pick_radius(local_clip, globals.clip_radii_h, globals.clip_radii_v);
        let d = sd_rounded_box(local_clip, half, r);
        let aa = aa_width(d);
        clip_alpha = clamp(0.5 - d / aa, 0.0, 1.0);
        if (clip_alpha <= 0.0) {
            discard;
        }
    }

    var base_color = in.color;
    if (in.gradient_offset >= 0.0) {
        let off = i32(in.gradient_offset + 0.5);
        if (i32(gradients[off].x) >= 1) {
            base_color = sample_gradient(off, in.local, in.half_size);
        }
    }

    let max_stroke = max(max(in.stroke.x, in.stroke.y), max(in.stroke.z, in.stroke.w));

    if (max_stroke <= 0.0) {
        if (in.pattern.x > 0.5) {
            let shadow_alpha = box_shadow_alpha(in.local, in.half_size, in.radii_h, in.radii_v, in.pattern, in.shadow_sigma);
            if (shadow_alpha <= 0.0) { discard; }
            return encode_srgb(vec4<f32>(base_color.rgb, base_color.a * shadow_alpha * clip_alpha));
        }
        let alpha = supersampled_fill_alpha(in.local, in.half_size, in.radii_h, in.radii_v);
        // Filled mode.
        if (alpha <= 0.0) { discard; }
        return encode_srgb(vec4<f32>(base_color.rgb, base_color.a * alpha * clip_alpha));
    }

    // A stroke's pattern holds its dash (y), its gap (z) and the edge its radii are at (w).
    var alpha = supersampled_stroke_alpha(in.local, in.half_size, in.radii_h, in.radii_v, in.stroke, in.pattern.w);
    if (alpha > 0.0 && in.pattern.y > 0.0 && in.pattern.z > 0.0) {
        let place = outline_position(in.local, in.half_size, in.radii_h, in.stroke);
        alpha = alpha * dash_coverage(place.x, place.y, in.pattern.y, in.pattern.z);
    }

    if (alpha <= 0.0) {
        discard;
    }
    return encode_srgb(vec4<f32>(base_color.rgb, base_color.a * alpha * clip_alpha));
}
