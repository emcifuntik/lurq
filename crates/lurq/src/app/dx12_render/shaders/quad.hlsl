cbuffer Globals : register(b0)
{
  float4 viewport;
  float4 clip_rect;
  float4 clip_radii_h;
  float4 clip_radii_v;
  float4 clip_active;
};

// Gradient stop storage. Layout per gradient (each element is a float4):
//   [count, kind, flags, from_angle]
//   [dir.x, dir.y, center.x, center.y]
//   then per stop: [r, g, b, a], [position, _, _, _]
// kind: 0 = linear, 1 = radial (flags bit0 = ellipse), 2 = conic.
StructuredBuffer<float4> gradients : register(t0);

struct VsIn
{
  float2 corner : TEXCOORD0;
  float2 pos : TEXCOORD1;
  float2 size : TEXCOORD2;
  float4 color : TEXCOORD3;
  float4 radii_h : TEXCOORD4;
  float4 radii_v : TEXCOORD5;
  float4 stroke : TEXCOORD6;
  float4 pattern : TEXCOORD7;
  float4 transform : TEXCOORD8;
  float2 xf_origin : TEXCOORD9;
  float shadow_sigma : TEXCOORD10;
  float gradient_offset : TEXCOORD11;
};

struct VsOut
{
  float4 position : SV_POSITION;
  float4 color : COLOR0;
  float2 local : TEXCOORD0;
  float2 half_size : TEXCOORD1;
  float4 radii_h : TEXCOORD2;
  float4 radii_v : TEXCOORD3;
  float4 stroke : TEXCOORD4;
  float gradient_offset : TEXCOORD5;
  float4 pattern : TEXCOORD6;
  float shadow_sigma : TEXCOORD7;
};

// Box shadows. The same formula as `blurred_rounded_rect_coverage` in
// `layout/box_shadow.rs` and `quad.wgsl`; keep the three in step. A shadow
// instance has no stroke and `pattern.x` = 1 (outer) or 2 (inset); `radii_h`
// are the element box's radii, `radii_v` the shadow shape's, `pattern.yz` the
// shape offset, `pattern.w` the spread and `shadow_sigma` the Gaussian's
// standard deviation.
static const int SHADOW_Y_SAMPLES = 8;
static const float SHADOW_EXTENT_SIGMAS = 3.0;
static const float SHADOW_MIN_SIGMA = 0.1;

VsOut vs_main(VsIn input)
{
  float aa_outset = 2.0;
  float max_stroke = max(max(input.stroke.x, input.stroke.y), max(input.stroke.z, input.stroke.w));
  if (max_stroke <= 0.0 && input.pattern.x > 0.5 && input.pattern.x < 1.5)
  {
    // An outer shadow paints beyond its element box.
    aa_outset += max(input.pattern.w, 0.0) + max(abs(input.pattern.y), abs(input.pattern.z))
      + SHADOW_EXTENT_SIGMAS * input.shadow_sigma;
  }
  float2 local_px = input.corner * (input.size + float2(aa_outset * 2.0, aa_outset * 2.0))
    - float2(aa_outset, aa_outset);
  float2 centered = local_px - input.xf_origin;
  float2 transformed = float2(
    input.transform.x * centered.x + input.transform.z * centered.y,
    input.transform.y * centered.x + input.transform.w * centered.y
  );
  float2 px = input.pos + transformed + input.xf_origin;
  // `viewport.zw` is the target's origin in window pixels (non-zero when
  // painting into an opacity layer).
  float2 target_px = px - viewport.zw;
  float2 ndc = float2((target_px.x / viewport.x) * 2.0 - 1.0, 1.0 - (target_px.y / viewport.y) * 2.0);

  VsOut output;
  output.position = float4(ndc, 0.0, 1.0);
  output.color = input.color;
  output.local = local_px - input.size * 0.5;
  output.half_size = input.size * 0.5;
  output.radii_h = input.radii_h;
  output.radii_v = input.radii_v;
  output.stroke = input.stroke;
  output.gradient_offset = input.gradient_offset;
  output.pattern = input.pattern;
  output.shadow_sigma = input.shadow_sigma;
  return output;
}

float4 sample_gradient(int off, float2 local, float2 half_size)
{
  float4 h0 = gradients[off];
  float4 h1 = gradients[off + 1];
  int count = (int)h0.x;
  int kind = (int)h0.y;
  const float PI = 3.14159265359;

  float t;
  if (kind == 0)
  {
    float2 dir = h1.xy;
    float hl = abs(half_size.x * dir.x) + abs(half_size.y * dir.y);
    t = (dot(local, dir) + hl) / (2.0 * max(hl, 1e-5));
  }
  else if (kind == 1)
  {
    float2 center = (h1.zw * 2.0 - float2(1.0, 1.0)) * half_size;
    if (h0.z > 0.5)
    {
      float2 cn = h1.zw * 2.0 - float2(1.0, 1.0);
      float2 p = (local - center) / max(half_size, float2(1e-3, 1e-3));
      float radius = max(
        max(length(float2(-1.0, -1.0) - cn), length(float2(1.0, -1.0) - cn)),
        max(length(float2(-1.0, 1.0) - cn), length(float2(1.0, 1.0) - cn)));
      t = length(p) / max(radius, 1e-5);
    }
    else
    {
      float radius = max(
        max(length(float2(-half_size.x, -half_size.y) - center), length(float2(half_size.x, -half_size.y) - center)),
        max(length(float2(-half_size.x, half_size.y) - center), length(float2(half_size.x, half_size.y) - center)));
      t = length(local - center) / max(radius, 1e-5);
    }
  }
  else
  {
    float2 center = (h1.zw * 2.0 - float2(1.0, 1.0)) * half_size;
    float2 d = local - center;
    float ang = (atan2(d.x, -d.y) - h0.w) / (2.0 * PI);
    t = ang - floor(ang);
  }

  if (kind != 2)
  {
    t = saturate(t);
  }

  int stop_base = off + 2;
  int last = count - 1;
  float4 color = gradients[stop_base + 2 * last];
  for (int i = 0; i < last; i = i + 1)
  {
    float pb = gradients[stop_base + 2 * (i + 1) + 1].x;
    if (t <= pb)
    {
      float pa = gradients[stop_base + 2 * i + 1].x;
      float4 ca = gradients[stop_base + 2 * i];
      float4 cb = gradients[stop_base + 2 * (i + 1)];
      float span = max(pb - pa, 1e-5);
      color = lerp(ca, cb, saturate((t - pa) / span));
      break;
    }
  }
  return color;
}

float2 pick_radius(float2 p, float4 radii_h, float4 radii_v)
{
  if (p.y < 0.0)
  {
    return p.x < 0.0 ? float2(radii_h.x, radii_v.x) : float2(radii_h.y, radii_v.y);
  }
  return p.x < 0.0 ? float2(radii_h.w, radii_v.w) : float2(radii_h.z, radii_v.z);
}

float sd_rounded_box(float2 p, float2 half_size, float2 radius)
{
  float2 safe_radius = max(radius, float2(0.0, 0.0));
  float2 q = abs(p) - half_size + safe_radius;
  if (q.x > 0.0 && q.y > 0.0)
  {
    if (safe_radius.x <= 0.0 || safe_radius.y <= 0.0)
    {
      return max(q.x, q.y);
    }
    float2 pn = q / safe_radius;
    float len_pn = length(pn);
    if (len_pn <= 1e-6)
    {
      return -min(safe_radius.x, safe_radius.y);
    }
    float gradient = max(length(pn / safe_radius), 1e-6);
    return (len_pn - 1.0) * len_pn / gradient;
  }
  return max(q.x - safe_radius.x, q.y - safe_radius.y);
}

float aa_width(float dist)
{
  return max(fwidth(dist), 1.0);
}

// Ramp width for one of the four half-pixel subsamples: half a pixel, so a
// pixel fully inside an axis-aligned edge gets full coverage. Matches quad.wgsl.
float subsample_aa_width(float dist)
{
  return 0.5 * aa_width(dist);
}

float rounded_clip_alpha(float2 frag_pos)
{
  if (clip_active.x <= 0.5)
  {
    return 1.0;
  }

  float2 half_size = clip_rect.zw * 0.5;
  float2 centre = clip_rect.xy + half_size;
  float2 local = frag_pos - centre;
  float dist = sd_rounded_box(local, half_size, pick_radius(local, clip_radii_h, clip_radii_v));
  return saturate(0.5 - dist / aa_width(dist));
}

float rounded_fill_alpha(float2 local, float2 half_size, float4 radii_h, float4 radii_v)
{
  float2 radius = pick_radius(local, radii_h, radii_v);
  float dist = sd_rounded_box(local, half_size, radius);
  return saturate(0.5 - dist / subsample_aa_width(dist));
}

// Strokes. The same formulas as `layout/stroke.rs`, which describes the
// shape, and `quad.wgsl`; keep the three in step.

// How far the hole of a stroke reaches past an edge whose side has no
// stroke, so the ring has no coverage there.
static const float HOLE_MARGIN = 1.0;

float stroke_inset(float width)
{
  return width > 0.0 ? width : -HOLE_MARGIN;
}

// How much nearer the second of two edges that meet at a corner has to be for
// a point to be taken as that edge's.
static const float CORNER_BIAS = 0.01;

// Whether a point `first` away from one edge of a corner and `second` from
// the other is taken as the first edge's: the nearer one, and always the one
// that has a stroke where the other has none.
bool first_edge(float first, float second, float first_width, float second_width)
{
  if (second_width <= 0.0)
  {
    return true;
  }
  if (first_width <= 0.0)
  {
    return false;
  }
  return first <= second + CORNER_BIAS;
}

// Where along the rect's outline the part of it nearest `p` is, in pixels
// clockwise from the start of the top edge (`.x`), and the outline's whole
// length (`.y`). `radii` are circular, in the order TL, TR, BR, BL; `stroke`
// is the widths of the sides (top, right, bottom, left).
float2 outline_position(float2 p, float2 hs, float4 radii, float4 stroke)
{
  float limit = max(min(hs.x, hs.y), 0.0);
  float4 r = clamp(radii, float4(0.0, 0.0, 0.0, 0.0), float4(limit, limit, limit, limit));
  float quarter = 1.57079632679;
  float top = max(2.0 * hs.x - r.x - r.y, 0.0);
  float right = max(2.0 * hs.y - r.y - r.z, 0.0);
  float bottom = max(2.0 * hs.x - r.z - r.w, 0.0);
  float left = max(2.0 * hs.y - r.w - r.x, 0.0);
  // Where each part of the outline starts.
  float right_arc = top;
  float right_edge = right_arc + quarter * r.y;
  float bottom_arc = right_edge + right;
  float bottom_edge = bottom_arc + quarter * r.z;
  float left_arc = bottom_edge + bottom;
  float left_edge = left_arc + quarter * r.w;
  float top_arc = left_edge + left;
  float total = top_arc + quarter * r.x;

  float on_top = clamp(p.x + hs.x - r.x, 0.0, top);
  float on_right = right_edge + clamp(p.y + hs.y - r.y, 0.0, right);
  float on_bottom = bottom_edge + clamp(hs.x - r.z - p.x, 0.0, bottom);
  float on_left = left_edge + clamp(hs.y - r.w - p.y, 0.0, left);
  float position;
  if (p.y < 0.0)
  {
    if (p.x < 0.0)
    {
      float2 c = float2(-hs.x + r.x, -hs.y + r.x);
      if (p.x < c.x && p.y < c.y)
      {
        position = top_arc + atan2(c.y - p.y, c.x - p.x) * r.x;
      }
      else if (first_edge(p.x + hs.x, p.y + hs.y, stroke.w, stroke.x))
      {
        position = on_left;
      }
      else
      {
        position = on_top;
      }
    }
    else
    {
      float2 c = float2(hs.x - r.y, -hs.y + r.y);
      if (p.x > c.x && p.y < c.y)
      {
        position = right_arc + atan2(p.x - c.x, c.y - p.y) * r.y;
      }
      else if (first_edge(p.y + hs.y, hs.x - p.x, stroke.x, stroke.y))
      {
        position = on_top;
      }
      else
      {
        position = on_right;
      }
    }
  }
  else
  {
    if (p.x < 0.0)
    {
      float2 c = float2(-hs.x + r.w, hs.y - r.w);
      if (p.x < c.x && p.y > c.y)
      {
        position = left_arc + atan2(c.x - p.x, p.y - c.y) * r.w;
      }
      else if (first_edge(hs.y - p.y, p.x + hs.x, stroke.z, stroke.w))
      {
        position = on_bottom;
      }
      else
      {
        position = on_left;
      }
    }
    else
    {
      float2 c = float2(hs.x - r.z, hs.y - r.z);
      if (p.x > c.x && p.y > c.y)
      {
        position = bottom_arc + atan2(p.y - c.y, p.x - c.x) * r.z;
      }
      else if (first_edge(hs.x - p.x, hs.y - p.y, stroke.y, stroke.z))
      {
        position = on_right;
      }
      else
      {
        position = on_bottom;
      }
    }
  }
  return float2(position, total);
}

// How much of a dashed stroke shows at `position` along an outline of length
// `total`: 1 in a dash, 0 in a gap, a ramp of a pixel between. The period is
// fitted to the outline a whole number of times.
float dash_coverage(float position, float total, float dash, float gap)
{
  if (dash <= 0.0 || gap <= 0.0 || total <= 0.0)
  {
    return 1.0;
  }
  float period = dash + gap;
  float fitted = total / max(floor(total / period + 0.5), 1.0);
  float on = dash * fitted / period;
  float phase = position - floor(position / fitted) * fitted;
  float d = phase <= on ? -min(phase, on - phase) : min(phase - on, fitted - phase);
  return saturate(0.5 - d);
}

// The ring of a stroke: inside the outer edge and outside the hole. `stroke`
// is the widths (top, right, bottom, left); `radii_at` says which edge of the
// stroke the radii are the corners of (0 outer, 1 the hole's).
float rounded_stroke_alpha(float2 local, float2 half_size, float4 radii_h, float4 radii_v, float4 stroke, float radii_at)
{
  float4 widths = max(stroke, float4(0.0, 0.0, 0.0, 0.0));
  float2 radius = pick_radius(local, radii_h, radii_v);
  // The sides that meet at this quadrant's corner.
  float across = local.x < 0.0 ? widths.w : widths.y;
  float along = local.y < 0.0 ? widths.x : widths.z;

  float2 outer_radius = float2(0.0, 0.0);
  float2 hole_radius = float2(0.0, 0.0);
  if (radius.x > 0.0)
  {
    outer_radius.x = radius.x + radii_at * across;
    hole_radius.x = max(outer_radius.x - stroke_inset(across), 0.0);
  }
  if (radius.y > 0.0)
  {
    outer_radius.y = radius.y + radii_at * along;
    hole_radius.y = max(outer_radius.y - stroke_inset(along), 0.0);
  }
  float outer_dist = sd_rounded_box(local, half_size, outer_radius);

  float top = stroke_inset(widths.x);
  float right = stroke_inset(widths.y);
  float bottom = stroke_inset(widths.z);
  float left = stroke_inset(widths.w);
  float2 hole_half = max(float2(
    half_size.x - 0.5 * (left + right),
    half_size.y - 0.5 * (top + bottom)
  ), float2(0.0, 0.0));
  float2 hole_center = float2(0.5 * (left - right), 0.5 * (top - bottom));
  float hole_dist = sd_rounded_box(local - hole_center, hole_half, hole_radius);
  float dist = max(outer_dist, -hole_dist);
  return saturate(0.5 - dist / subsample_aa_width(dist));
}

float supersampled_fill_alpha(float2 local, float2 half_size, float4 radii_h, float4 radii_v)
{
  return (
    rounded_fill_alpha(local + float2(-0.25, -0.25), half_size, radii_h, radii_v) +
    rounded_fill_alpha(local + float2(0.25, -0.25), half_size, radii_h, radii_v) +
    rounded_fill_alpha(local + float2(-0.25, 0.25), half_size, radii_h, radii_v) +
    rounded_fill_alpha(local + float2(0.25, 0.25), half_size, radii_h, radii_v)
  ) * 0.25;
}

float supersampled_stroke_alpha(float2 local, float2 half_size, float4 radii_h, float4 radii_v, float4 stroke, float radii_at)
{
  return (
    rounded_stroke_alpha(local + float2(-0.25, -0.25), half_size, radii_h, radii_v, stroke, radii_at) +
    rounded_stroke_alpha(local + float2(0.25, -0.25), half_size, radii_h, radii_v, stroke, radii_at) +
    rounded_stroke_alpha(local + float2(-0.25, 0.25), half_size, radii_h, radii_v, stroke, radii_at) +
    rounded_stroke_alpha(local + float2(0.25, 0.25), half_size, radii_h, radii_v, stroke, radii_at)
  ) * 0.25;
}

float2 shadow_erf(float2 x)
{
  float2 s = sign(x);
  float2 a = abs(x);
  float2 r = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
  r = r * r;
  return s - s / (r * r);
}

float shadow_gaussian(float x, float sigma)
{
  return exp(-(x * x) / (2.0 * sigma * sigma)) / (2.5066283 * sigma);
}

// A row of the rounded rect at height `y`, convolved along x with the
// Gaussian: the difference of two `erf`s at the row's curved ends.
float shadow_blur_along_x(float x, float y, float sigma, float corner, float2 half_size)
{
  float delta = min(half_size.y - corner - abs(y), 0.0);
  float curved = half_size.x - corner + sqrt(max(corner * corner - delta * delta, 0.0));
  float2 integral = 0.5 + 0.5 * shadow_erf((float2(x, x) + float2(-curved, curved)) * (0.70710678 / sigma));
  return integral.y - integral.x;
}

// Coverage of a rounded rect (half extent `half_size`, circular corner
// `radii`) blurred by `sigma`, at centre-relative `p`; the y convolution is a
// midpoint sum over +/- 3 sigma.
float blurred_rounded_box(float2 p, float2 half_size, float4 radii, float sigma)
{
  if (half_size.x <= 0.0 || half_size.y <= 0.0)
  {
    return 0.0;
  }
  float corner = max(min(pick_radius(p, radii, radii).x, min(half_size.x, half_size.y)), 0.0);
  float extent = SHADOW_EXTENT_SIGMAS * sigma;
  float start = clamp(-extent, p.y - half_size.y, p.y + half_size.y);
  float end = clamp(extent, p.y - half_size.y, p.y + half_size.y);
  float step = (end - start) / (float)SHADOW_Y_SAMPLES;
  float y = start + step * 0.5;
  float coverage = 0.0;
  [unroll]
  for (int i = 0; i < SHADOW_Y_SAMPLES; i = i + 1)
  {
    coverage += shadow_blur_along_x(p.x, p.y - y, sigma, corner, half_size) * shadow_gaussian(y, sigma) * step;
    y += step;
  }
  return saturate(coverage);
}

float shadow_shape_alpha(float2 p, float2 half_size, float4 radii, float sigma)
{
  if (sigma < SHADOW_MIN_SIGMA)
  {
    if (half_size.x <= 0.0 || half_size.y <= 0.0)
    {
      return 0.0;
    }
    return supersampled_fill_alpha(p, half_size, radii, radii);
  }
  return blurred_rounded_box(p, half_size, radii, sigma);
}

// An outer shadow covers its shape outside the element box; an inset one
// covers the box outside its shape.
float box_shadow_alpha(float2 local, float2 half_size, float4 box_radii, float4 shape_radii, float4 params, float sigma)
{
  float2 offset = params.yz;
  float spread = params.w;
  float box_alpha = supersampled_fill_alpha(local, half_size, box_radii, box_radii);
  if (params.x < 1.5)
  {
    float2 shape_half = max(half_size + float2(spread, spread), float2(0.0, 0.0));
    return shadow_shape_alpha(local - offset, shape_half, shape_radii, sigma) * (1.0 - box_alpha);
  }
  float2 shape_half = max(half_size - float2(spread, spread), float2(0.0, 0.0));
  return box_alpha * (1.0 - shadow_shape_alpha(local - offset, shape_half, shape_radii, sigma));
}

// The render target is not sRGB, so the fixed-function blend mixes
// sRGB-encoded values, as CSS and design tools do. `ps_main` works in linear
// light like the rest of the pipeline and encodes each result it returns.
float4 encode_srgb(float4 color)
{
  float3 c = saturate(color.rgb);
  float3 low = c * 12.92;
  float3 high = 1.055 * pow(c, 1.0 / 2.4) - 0.055;
  return float4(c <= 0.0031308 ? low : high, color.a);
}

float4 ps_main(VsOut input) : SV_TARGET
{
  float clip_alpha_value = rounded_clip_alpha(input.position.xy);
  if (clip_alpha_value <= 0.0)
  {
    discard;
  }

  float4 base_color = input.color;
  if (input.gradient_offset >= 0.0)
  {
    int off = (int)(input.gradient_offset + 0.5);
    if ((int)gradients[off].x >= 1)
    {
      base_color = sample_gradient(off, input.local, input.half_size);
    }
  }

  float max_stroke = max(max(input.stroke.x, input.stroke.y), max(input.stroke.z, input.stroke.w));
  if (max_stroke <= 0.0)
  {
    if (input.pattern.x > 0.5)
    {
      float shadow_alpha = box_shadow_alpha(input.local, input.half_size, input.radii_h, input.radii_v, input.pattern, input.shadow_sigma);
      if (shadow_alpha <= 0.0)
      {
        discard;
      }
      return encode_srgb(float4(base_color.rgb, base_color.a * shadow_alpha * clip_alpha_value));
    }
    float fill_alpha = supersampled_fill_alpha(input.local, input.half_size, input.radii_h, input.radii_v);
    return encode_srgb(float4(base_color.rgb, base_color.a * fill_alpha * clip_alpha_value));
  }

  // A stroke's pattern holds its dash (y), its gap (z) and the edge its radii are at (w).
  float alpha = supersampled_stroke_alpha(input.local, input.half_size, input.radii_h, input.radii_v, input.stroke, input.pattern.w);
  if (alpha > 0.0 && input.pattern.y > 0.0 && input.pattern.z > 0.0)
  {
    float2 place = outline_position(input.local, input.half_size, input.radii_h, input.stroke);
    alpha *= dash_coverage(place.x, place.y, input.pattern.y, input.pattern.z);
  }
  return encode_srgb(float4(input.color.rgb, input.color.a * alpha * clip_alpha_value));
}
