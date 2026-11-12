// One pass of a layer blur or a backdrop blur (see
// `layout::opacity_layer::LayerBlurPass`).
//
// A blurred layer is painted like any other, then blurred on its way into
// its parent: a horizontal Gaussian and a vertical one between intermediate
// textures, and a pass that draws the result into the parent at the layer's
// opacity, one read per pixel. A wide blur first reduces the layer with a
// box filter, and both Gaussians run on the reduced copy; the last pass then
// also scales it back up, with the sampler's interpolation.
//
// Every texture holds premultiplied colour and was cleared to transparent,
// so the premultiplied source-over blend of the pipeline writes an
// intermediate target as it is and composites the last pass.
//
// A backdrop blur (see `layout::backdrop`) runs the same passes on a copy of
// what its target held, and its last pass puts the result back inside the
// node's rounded box in place of what was there. That takes two draws of
// the same rect, since the blend has to remove the old pixels by the box's
// coverage and add the new ones by it: `fs_backdrop_erase`, blended
// `dst * (1 - src.a)`, then `fs_backdrop_add`, blended `dst + src`.
//
// Vertex buffer 0: unit quad corner in [0,1]^2.

struct Pass {
    rect:   vec4<f32>,  // rect drawn, in target pixels: x, y, w, h
    dest:   vec4<f32>,  // target size, then the offset added to a target pixel
    source: vec4<f32>,  // offset target pixel -> source texel scale, then 1 / source texture size
    kernel: vec4<f32>,  // Gaussian axis in source texels, sigma in source texels, opacity
    mode:   vec4<f32>,  // x = 0: Gaussian; otherwise box taps per axis. y, z: mask and clip scale. w = 1: one read
    limit:  vec4<f32>,  // lowest, then highest source texel centre a tap may read
    mask:         vec4<f32>,  // backdrop: the box to fill: centre in target pixels, half size
    mask_inverse: vec4<f32>,  // 2x2 from a target-pixel offset to the box's own axes
    radii:        vec4<f32>,  // the box's radii: tl, tr, br, bl
    clip:         vec4<f32>,  // last pass: the rounded clip to cut to, in the same form
    clip_inverse: vec4<f32>,
    clip_radii:   vec4<f32>,
};

@group(0) @binding(0) var<uniform> blur: Pass;
@group(0) @binding(1) var layer: texture_2d<f32>;
@group(0) @binding(2) var layer_sampler: sampler;

// Pairs of texels a Gaussian pass reads on each side of its centre. A pair
// is one bilinear tap, so this covers 32 texels: three deviations of the
// widest Gaussian a pass is given.
const MAX_PAIRS: i32 = 16;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
};

@vertex
fn vs_main(@location(0) corner: vec2<f32>) -> VsOut {
    let px = blur.rect.xy + corner * blur.rect.zw;
    let size = blur.dest.xy;
    var out: VsOut;
    out.clip = vec4<f32>((px.x / size.x) * 2.0 - 1.0, 1.0 - (px.y / size.y) * 2.0, 0.0, 1.0);
    return out;
}

// Past the texels the pass may read, the edge texel repeats: the texture
// around them holds whatever it was last used for.
fn read(texel: vec2<f32>) -> vec4<f32> {
    let inside = clamp(texel, blur.limit.xy, blur.limit.zw);
    return textureSampleLevel(layer, layer_sampler, inside * blur.source.zw, 0.0);
}

// Averages the `2 * taps` square of source texels the pixel covers: each tap
// sits on the corner shared by four texels and reads their mean.
fn reduce(center: vec2<f32>, taps: i32) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    let first = -f32(taps - 1);
    for (var y = 0; y < taps; y++) {
        for (var x = 0; x < taps; x++) {
            sum += read(center + vec2<f32>(first + 2.0 * f32(x), first + 2.0 * f32(y)));
        }
    }
    return sum / f32(taps * taps);
}

// A Gaussian along `axis`, centred on the texel centre `center`. Texels `i`
// and `i + 1` are read with one bilinear tap between them, placed so the
// filter weighs them as the Gaussian does; that only holds from a texel
// centre, which is where a Gaussian pass reads: its target has its source's
// texels.
fn gaussian(center: vec2<f32>, axis: vec2<f32>, sigma: f32) -> vec4<f32> {
    var sum = read(center);
    var total = 1.0;
    let falloff = -0.5 / (sigma * sigma);
    let reach = ceil(sigma * 3.0);
    for (var pair = 0; pair < MAX_PAIRS; pair++) {
        let near = f32(2 * pair + 1);
        if (near > reach) {
            break;
        }
        let far = near + 1.0;
        let near_weight = exp(near * near * falloff);
        let far_weight = exp(far * far * falloff);
        let weight = near_weight + far_weight;
        let offset = axis * ((near * near_weight + far * far_weight) / weight);
        sum += (read(center + offset) + read(center - offset)) * weight;
        total += 2.0 * weight;
    }
    return sum / total;
}

fn blurred(pixel: vec2<f32>) -> vec4<f32> {
    let center = (pixel + blur.dest.zw) * blur.source.xy;
    // The pass that composites a finished blur: the sampler interpolates
    // between the texels of a reduced layer as it scales it back up.
    if (blur.mode.w > 0.5) {
        return read(center);
    }
    let taps = i32(blur.mode.x);
    if (taps > 0) {
        return reduce(center, taps);
    }
    return gaussian(center, blur.kernel.xy, max(blur.kernel.z, 0.01));
}

// How much of `pixel` a rounded box covers (see `RoundedBox`): the pixel's
// offset from the box's centre is taken into the box's own axes, where its
// distance to the rounded edge is measured, and `scale` brings the distance
// back to pixels.
fn box_coverage(pixel: vec2<f32>, box: vec4<f32>, inverse: vec4<f32>, radii: vec4<f32>, scale: f32) -> f32 {
    let offset = pixel - box.xy;
    let p = vec2<f32>(inverse.x * offset.x + inverse.z * offset.y, inverse.y * offset.x + inverse.w * offset.y);
    var radius = radii.x;
    if (p.x >= 0.0 && p.y < 0.0) {
        radius = radii.y;
    } else if (p.x >= 0.0) {
        radius = radii.z;
    } else if (p.y >= 0.0) {
        radius = radii.w;
    }
    let q = abs(p) - box.zw + vec2<f32>(radius);
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    return clamp(0.5 - distance * scale, 0.0, 1.0);
}

// How much of the last pass's result shows at `pixel`: its opacity, cut to
// the rounded clip.
fn shown(pixel: vec2<f32>) -> f32 {
    return blur.kernel.w * box_coverage(pixel, blur.clip, blur.clip_inverse, blur.clip_radii, blur.mode.z);
}

// How much of `pixel` the blurred backdrop replaces: the node's rounded box.
fn backdrop_coverage(pixel: vec2<f32>) -> f32 {
    return shown(pixel) * box_coverage(pixel, blur.mask, blur.mask_inverse, blur.radii, blur.mode.y);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return blurred(in.clip.xy) * shown(in.clip.xy);
}

@fragment
fn fs_backdrop_erase(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, backdrop_coverage(in.clip.xy));
}

@fragment
fn fs_backdrop_add(in: VsOut) -> @location(0) vec4<f32> {
    return blurred(in.clip.xy) * backdrop_coverage(in.clip.xy);
}
