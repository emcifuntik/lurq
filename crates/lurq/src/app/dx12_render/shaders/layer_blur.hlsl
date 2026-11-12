// One pass of a layer blur or a backdrop blur (see
// `layout::opacity_layer::LayerBlurPass`).
//
// A blurred layer is painted like any other, then blurred on its way into
// its parent: a horizontal Gaussian into an intermediate texture, and a
// vertical one drawn straight into the parent at the layer's opacity. A wide
// blur first reduces the layer with a box filter, and both Gaussians run on
// the reduced copy; the last pass then also scales it back up.
//
// Every texture holds premultiplied colour and was cleared to transparent,
// so the premultiplied source-over blend of the pipeline writes an
// intermediate target as it is and composites the last pass.
//
// A backdrop blur (see `layout::backdrop`) runs the same passes on a copy of
// what its target held, and its last pass puts the result back inside the
// node's rounded box in place of what was there. That takes two draws of
// the same rect, since the blend has to remove the old pixels by the box's
// coverage and add the new ones by it: `ps_backdrop_erase`, blended
// `dst * (1 - src.a)`, then `ps_backdrop_add`, blended `dst + src`.

cbuffer BlurPass : register(b0)
{
  float4 rect;   // rect drawn, in target pixels: x, y, w, h
  float4 target; // target size, then the offset added to a target pixel
  float4 source; // offset target pixel -> source texel scale, then 1 / source texture size
  float4 kernel; // Gaussian axis in source texels, sigma in source texels, opacity
  float4 mode;   // x = 0: Gaussian; otherwise box taps per axis. y, z: mask and clip scale
  float4 limit;  // lowest, then highest source texel centre a tap may read
  float4 mask;         // backdrop: the box to fill: centre in target pixels, half size
  float4 mask_inverse; // 2x2 from a target-pixel offset to the box's own axes
  float4 radii;        // the box's radii: tl, tr, br, bl
  float4 cut;          // last pass: the rounded clip to cut to, in the same form
  float4 cut_inverse;
  float4 cut_radii;
};

Texture2D<float4> layer_texture : register(t0);
SamplerState layer_sampler : register(s0);

// Pairs of texels a Gaussian pass reads on each side of its centre. A pair
// is one bilinear tap, so this covers 32 texels: three deviations of the
// widest Gaussian a pass is given.
static const int MAX_PAIRS = 16;

struct VsOut
{
  float4 position : SV_POSITION;
};

VsOut vs_main(float2 corner : TEXCOORD0)
{
  float2 px = rect.xy + corner * rect.zw;
  VsOut output;
  output.position = float4((px.x / target.x) * 2.0 - 1.0, 1.0 - (px.y / target.y) * 2.0, 0.0, 1.0);
  return output;
}

// Past the texels the pass may read, the edge texel repeats: the texture
// around them holds whatever it was last used for.
float4 read(float2 texel)
{
  float2 inside = clamp(texel, limit.xy, limit.zw);
  return layer_texture.SampleLevel(layer_sampler, inside * source.zw, 0.0);
}

// Averages the `2 * taps` square of source texels the pixel covers: each tap
// sits on the corner shared by four texels and reads their mean.
float4 reduce(float2 center, int taps)
{
  float4 sum = float4(0.0, 0.0, 0.0, 0.0);
  float first = -float(taps - 1);
  [loop]
  for (int y = 0; y < taps; y++)
  {
    [loop]
    for (int x = 0; x < taps; x++)
    {
      sum += read(center + float2(first + 2.0 * float(x), first + 2.0 * float(y)));
    }
  }
  return sum / float(taps * taps);
}

// A Gaussian along `axis`, centred on the texel centre `center`. Texels `i`
// and `i + 1` are read with one bilinear tap between them, placed so the
// filter weighs them as the Gaussian does; that only holds from a texel
// centre.
float4 gaussian_at(float2 center, float2 axis, float sigma)
{
  float4 sum = read(center);
  float total = 1.0;
  float falloff = -0.5 / (sigma * sigma);
  float reach = ceil(sigma * 3.0);
  [loop]
  for (int pair = 0; pair < MAX_PAIRS; pair++)
  {
    float near_texel = float(2 * pair + 1);
    if (near_texel > reach)
    {
      break;
    }
    float far_texel = near_texel + 1.0;
    float near_weight = exp(near_texel * near_texel * falloff);
    float far_weight = exp(far_texel * far_texel * falloff);
    float weight = near_weight + far_weight;
    float2 offset = axis * ((near_texel * near_weight + far_texel * far_weight) / weight);
    sum += (read(center + offset) + read(center - offset)) * weight;
    total += 2.0 * weight;
  }
  return sum / total;
}

// A Gaussian along `axis` at `center`. The pass that scales a reduced layer
// back up reads between texel centres along the axis: there the Gaussian is
// taken at the texel centre on either side and the two are interpolated.
// (Across the axis the sampler interpolates.)
float4 gaussian(float2 center, float2 axis, float sigma)
{
  float along = dot(center, axis) - 0.5;
  float between = along - floor(along);
  float2 before = center - axis * between;
  if (between < 0.001)
  {
    return gaussian_at(before, axis, sigma);
  }
  if (between > 0.999)
  {
    return gaussian_at(before + axis, axis, sigma);
  }
  return lerp(gaussian_at(before, axis, sigma), gaussian_at(before + axis, axis, sigma), between);
}

float4 blurred(float2 pixel)
{
  float2 center = (pixel + target.zw) * source.xy;
  int taps = int(mode.x);
  if (taps > 0)
  {
    return reduce(center, taps);
  }
  return gaussian(center, kernel.xy, max(kernel.z, 0.01));
}

// How much of `pixel` a rounded box covers (see `RoundedBox`): the pixel's
// offset from the box's centre is taken into the box's own axes, where its
// distance to the rounded edge is measured, and `scale` brings the distance
// back to pixels.
float box_coverage(float2 pixel, float4 rounded, float4 inverse, float4 corner_radii, float scale)
{
  float2 offset = pixel - rounded.xy;
  float2 p = float2(inverse.x * offset.x + inverse.z * offset.y, inverse.y * offset.x + inverse.w * offset.y);
  float radius = corner_radii.x;
  if (p.x >= 0.0 && p.y < 0.0)
  {
    radius = corner_radii.y;
  }
  else if (p.x >= 0.0)
  {
    radius = corner_radii.z;
  }
  else if (p.y >= 0.0)
  {
    radius = corner_radii.w;
  }
  float2 q = abs(p) - rounded.zw + float2(radius, radius);
  float edge = length(max(q, float2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
  return clamp(0.5 - edge * scale, 0.0, 1.0);
}

// How much of the last pass's result shows at `pixel`: its opacity, cut to
// the rounded clip.
float shown(float2 pixel)
{
  return kernel.w * box_coverage(pixel, cut, cut_inverse, cut_radii, mode.z);
}

// How much of `pixel` the blurred backdrop replaces: the node's rounded box.
float backdrop_coverage(float2 pixel)
{
  return shown(pixel) * box_coverage(pixel, mask, mask_inverse, radii, mode.y);
}

float4 ps_main(VsOut input) : SV_TARGET
{
  return blurred(input.position.xy) * shown(input.position.xy);
}

float4 ps_backdrop_erase(VsOut input) : SV_TARGET
{
  return float4(0.0, 0.0, 0.0, backdrop_coverage(input.position.xy));
}

float4 ps_backdrop_add(VsOut input) : SV_TARGET
{
  return blurred(input.position.xy) * backdrop_coverage(input.position.xy);
}
