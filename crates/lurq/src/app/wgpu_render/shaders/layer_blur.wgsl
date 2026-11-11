// One pass of a layer blur (see `layout::opacity_layer::LayerBlurPass`).
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
// Vertex buffer 0: unit quad corner in [0,1]^2.

struct Pass {
    rect:   vec4<f32>,  // rect drawn, in target pixels: x, y, w, h
    dest:   vec4<f32>,  // target size, then the offset added to a target pixel
    source: vec4<f32>,  // offset target pixel -> source texel scale, then 1 / source texture size
    kernel: vec4<f32>,  // Gaussian axis in source texels, sigma in source texels, opacity
    mode:   vec4<f32>,  // x = 0: Gaussian; otherwise box taps per axis
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

fn read(texel: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(layer, layer_sampler, texel * blur.source.zw, 0.0);
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
// centre.
fn gaussian_at(center: vec2<f32>, axis: vec2<f32>, sigma: f32) -> vec4<f32> {
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

// A Gaussian along `axis` at `center`. The pass that scales a reduced layer
// back up reads between texel centres along the axis: there the Gaussian is
// taken at the texel centre on either side and the two are interpolated.
// (Across the axis the sampler interpolates.)
fn gaussian(center: vec2<f32>, axis: vec2<f32>, sigma: f32) -> vec4<f32> {
    let along = dot(center, axis) - 0.5;
    let between = along - floor(along);
    let before = center - axis * between;
    if (between < 0.001) {
        return gaussian_at(before, axis, sigma);
    }
    if (between > 0.999) {
        return gaussian_at(before + axis, axis, sigma);
    }
    return mix(gaussian_at(before, axis, sigma), gaussian_at(before + axis, axis, sigma), between);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let center = (in.clip.xy + blur.dest.zw) * blur.source.xy;
    let taps = i32(blur.mode.x);
    if (taps > 0) {
        return reduce(center, taps) * blur.kernel.w;
    }
    return gaussian(center, blur.kernel.xy, max(blur.kernel.z, 0.01)) * blur.kernel.w;
}
