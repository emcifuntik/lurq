---
title: Text Pipeline Optimization
description: Benchmark notes and improvement log for text layout and rasterization.
---

This page tracks text pipeline benchmark results and optimization work.

## Current implementation in 0.20.0

- Plain-text buffers retain shaping and full layouts with a 48 MiB accounted budget and a 64-entry limit per glyph engine. Cache pressure can compact layouts while preserving shaping.
- Unchanged paragraphs and immutable caret geometry are reused across edits. Conservative wrap intervals preserve eligible layouts across width changes; visual-row indexes accelerate caret and selection lookup.
- DX12 stages full glyph-atlas uploads in frame upload arenas. The full-upload correctness workaround remains, but the later staging optimization replaces the older allocation/copy path.
- Detailed counters require `perf_profile`. The workspace's eleven development dependency overrides do not propagate to consuming applications; configure their own workspace profiles when reproducing those dev-build measurements.

Changed in 0.61.0 by the [Chat Screen Text Optimization](/lurq/text-chat-optimization/) round:

- The 64-entry buffer cache above now holds documents only: plain texts of 1,024 bytes and more. Shorter texts (labels, chat messages) are kept shaped by their text and face in a 4 MiB least-recently-used cache of their own, and laid out again, not shaped again, for a new width, size, wrapping or scale factor.
- Each laid-out plain text has one cache entry holding its size, vertical extents, carets and glyphs, found with one fast hash of the text. A full cache drops the entries no pass used for 16 passes; it never empties at once.
- cosmic-text's `shape-run-cache` keeps shaped words across texts; the glyph engine trims it.
- A new glyph is packed into the atlas in place: the frame's snapshot shares the atlas's pixels and is released once drawn.
- The wgpu and DX12 renderers draw texts that follow one another in one instanced draw where their clips cut nothing off them.

The measurements below and in linked reports are historical observations from their stated fixtures, revisions, and machines. They are not new measurements of 0.20.0. Absolute `F:/codex-tmp/...` paths identify original local artifacts, which are not shipped with the repository.

## Measurement history

For development builds and package `opt-level` comparisons, see [Dev Text Pipeline Profiling](/lurq/text-pipeline-dev-profiling/). The historical Criterion results below use optimized bench builds.

The September 12 [CPU Text Pipeline Optimization](/lurq/text-pipeline-cpu-optimization/) shares full plain-text layouts, reduces optical centering work, and adds three dev-only font dependency overrides. In a repeated dev-profile comparison, cold README improved from 18.13 to 8.64 ms and column-flow long text from 143.18 to 17.90 ms. That report includes a unique-paragraph control, memory costs, and the remaining full-layout bottleneck.

The follow-up [Text Interaction Optimization](/lurq/text-interaction-optimization/) reuses unchanged paragraphs across edits and width changes. At 150% desktop scaling, native DX12 edit latency improved from 95.42 to 20.56 ms and window resize latency from 136.57 to 30.92 ms. The report includes CPU and native results, cache compaction, and the measured cost of DX12's full-atlas upload workaround.

The subsequent [Text Cache Budget](/lurq/text-cache-budget/) comparison raises the retained-storage limit to 48 MiB. Keeping logical and physical wrapped layouts together reduces native edit latency at 150% scaling from 19.81 to 10.29 ms in that experiment, with 5.73 MiB more charged storage during edits. A 64 MiB trial retained the same data.

[Text Cache Bookkeeping](/lurq/text-cache-bookkeeping/) then removes full-document paragraph indexing on small edits and repeated glyph-allocation scans on buffer returns. At the same 48 MiB budget, native edit latency improves from 9.73 to 3.82 ms and resize latency from 24.49 to 18.55 ms in its alternating comparison.

[Caret Layout Reuse](/lurq/text-caret-layout-reuse/) covers selectable text, whose caret calculation still shaped a separate buffer. Sharing its layout with measurement reduces native selectable-document edit latency from 52.92 to 6.79 ms and resize latency from 69.14 to 21.37 ms at 150% scaling. The report includes caret-coordinate validation and input tests.

[Caret and Selection Indexing](/lurq/text-selection-optimization/) shares caret geometry and indexes visual rows. In the CPU event-to-render benchmark at 150% scale near the document bottom, input drag falls from 2.89 to 0.30 ms and Up/Down navigation from 3.05 to 0.30 ms. The report includes first-click costs, exact lookup validation, and native edit/resize regression checks.

[Paragraph Caret Reuse](/lurq/text-paragraph-carets/) then replaces full caret-vector reconstruction on multiline edits with shared paragraph geometry. In the 1,728-paragraph fixture, an edit rebuilds one paragraph and reuses the other 1,727. The report separates validated work counts from preliminary timings affected by other compiler activity.

[Stable Text Reflow](/lurq/text-stable-reflow/) preserves wrapped layouts and caret geometry while width changes stay within unchanged wrap decisions. At 150% scale, CPU resizing rebuilds a median 72 of 1,728 paragraphs for the caret layout. The report includes native DX12 checks, the 54 KiB metadata cost, and separately labeled timing observations under external compiler load.

[DX12 Atlas Upload Reuse](/lurq/text-atlas-upload/) stages full atlas updates in existing frame memory. It removes the temporary atlas-sized vector and dedicated upload allocation in the normal path. All 24 before/after pixel captures match exactly; the native comparison observes scroll atlas preparation/upload falling from 3.90 to 0.25 ms under external compiler load.

[Chat Screen Text Optimization](/lurq/text-chat-optimization/) (November 12, 2026) measures a different workload in release builds: a chat screen of 206 short texts instead of one document, with the `text_chat` benchmark, a per-frame metrics mode and a probe through both native renderers. It shares draws between texts, stops copying the atlas for a new glyph, replaces the five plain-text caches with one, keeps short texts shaped across widths and sizes, reuses shaped words, and lets an application prepare its fonts before the first frame. An idle frame of that screen goes from 198 to 159 µs, a resize frame from 1.67 to 0.49 ms, a wheel step through 10,000 messages from 1.46 to 1.18 ms (its 99th percentile from 3.4 to 1.6 ms), the first frame from 19.7 to 2.5 ms with the fonts prepared, and 210 draw calls become 9. The report has the baseline and final numbers of every scenario and phase, what was tried and dropped, and what remains.

## Benchmark

The primary benchmark is `text_pipeline`, which renders the workspace root `README.md` through the real `Markdown` component. This keeps the workload close to a document-heavy app path: Markdown parsing, Markdown rendering, rich text layout, glyph rasterization, atlas population, and render-list generation.

Changing the README also changes these fixtures. Preserve the same README contents in both compared revisions when measuring an implementation change, and record its size or hash with the results.

Run it with:

```powershell
cargo bench -p lurq --bench text_pipeline --features markdown
```

For quick local checks while iterating:

```powershell
cargo bench -p lurq --bench text_pipeline --features markdown -- --sample-size 10 --warm-up-time 1 --measurement-time 2
```

The short command is useful for direction, but final claims should use the normal Criterion run.

## Historical Criterion Results

Full Criterion run from June 16, 2026 after the kept text/layout changes:

| Case | Full run |
| --- | ---: |
| `parse_readme_markdown/all` | ~10.4 us |
| `cold_readme_markdown_first_pass/all` | ~6.32 ms |
| `warm_readme_markdown_cached_pass/all` | ~57.1 us |
| `cold_readme_markdown_realistic_viewport/all` | ~5.15 ms |
| `warm_readme_markdown_realistic_viewport/all` | ~29.0 us |
| `cold_long_text_realistic_viewport/all` | ~3.44 ms |
| `warm_long_text_realistic_viewport/all` | ~25.9 us |
| `cold_flow_long_text_realistic_viewport/all` | ~27.1 ms |
| `warm_flow_long_text_realistic_viewport/all` | ~25.3 us |
| `remount_flow_long_text_same_app/all` | ~45.7 us |

One-shot profile from the same run:

```text
realistic README: total=4.35ms, layout=1.68ms, glyphs=2.25ms, swash=1.76ms/238
long text realistic: total=2.77ms, layout=0.01ms, glyphs=2.34ms, swash=0.94ms/178
flow long text realistic: total=28.28ms, layout=22.27ms, glyphs=5.61ms, swash=0.97ms/178
```

Short-run results from June 16, 2026:

| Case | Baseline | After rich text raster cache | After atlas snapshot reuse | After shared shaped layout | After borrowed key lookup | After display-text skip |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `parse_readme_markdown/32` | - | - | - | - | ~6.6 us | ~6.4 us |
| `cold_readme_markdown_first_pass/32` | ~4.40 ms | ~4.58 ms | ~5.11 ms | ~4.47 ms | ~4.41 ms | ~4.94 ms |
| `warm_readme_markdown_cached_pass/32` | ~581 us | ~202 us | ~27.8 us | ~29.9 us | ~26.4 us | ~28.3 us |
| `parse_readme_markdown/128` | - | - | - | - | ~11.4 us | ~10.1 us |
| `cold_readme_markdown_first_pass/128` | ~6.25 ms | ~6.84 ms | ~6.52 ms | ~6.38 ms | ~5.97 ms | ~6.19 ms |
| `warm_readme_markdown_cached_pass/128` | ~1.35 ms | ~257 us | ~51.0 us | ~65.0 us | ~48.9 us | ~49.1 us |
| `parse_readme_markdown/all` | - | - | - | - | ~11.7 us | ~9.7 us |
| `cold_readme_markdown_first_pass/all` | ~6.76 ms | ~6.67 ms | ~6.59 ms | ~6.29 ms | ~5.93 ms | ~6.12 ms |
| `warm_readme_markdown_cached_pass/all` | ~1.28 ms | ~248 us | ~51.1 us | ~60.9 us | ~48.4 us | ~49.3 us |

## Improvement Log

### Rich Text Raster Cache

Added a `rich_glyph_layout_cache` in `GlyphEngine`.

The cache key includes rich text spans, style data, width, wrapping, and raster mode. The cached payload stores glyph atlas coordinates and per-glyph color, allowing warm Markdown render-list passes to append glyph commands without reshaping rich text.

Validation:

```powershell
cargo test -p lurq --lib app::glyph_engine::tests
cargo test -p lurq --features markdown --test markdown_tests
cargo bench -p lurq --bench text_pipeline --features markdown --no-run
```

### Removed Cache-Miss Clones

Plain and baked-transformed text layout cache misses now append commands from the newly built layout, then move that layout into the cache. This removes a `Vec` clone on cache misses.

### Atlas Snapshot Reuse

Changed `GlyphAtlas` to hold shared immutable byte data and changed `AtlasPacker` to reuse an unchanged atlas snapshot. Warm frames now clone an `Arc<[u8]>` handle instead of cloning the full atlas byte buffer.

This moved the short-run `warm_readme_markdown_cached_pass/all` case from about 248 us to about 51 us.

### Shared Rich Text Shaped Layout

Added a shaped rich text layout cache populated by `measure_rich_text`. The following snapped rich text raster pass can now pack atlas glyphs from cached shaped glyph positions instead of shaping the same rich text block again.

This improved the short-run `cold_readme_markdown_first_pass/all` case from about 6.59 ms to about 6.29 ms. The same short run showed `warm_readme_markdown_cached_pass/all` moving from about 51 us to about 61 us, so this change primarily helps first render and relayout rather than the warm render-list path.

### Borrowed Rich Text Cache Lookup

Changed the rich text shaped-layout and packed-glyph caches to use fingerprint buckets with exact borrowed span comparison. Hits no longer need to clone span text into an owned key. Owned keys are still stored on insert, so fingerprint collisions are resolved by exact comparison.

This moved the short-run `cold_readme_markdown_first_pass/all` case from about 6.29 ms to about 5.93 ms and moved `warm_readme_markdown_cached_pass/all` from about 61 us to about 48 us.

### Atlas Dirty Rect Tracking

Added dirty rect tracking to the glyph atlas snapshot. `AtlasPacker` records packed regions, `GlyphAtlas` carries those regions, and the WGPU and DX12 renderers upload dirty subrectangles when the atlas size is unchanged. Texture creation and resize still use full-atlas uploads.

This section describes the historical implementation. DX12 currently forces full uploads when the atlas changes to avoid a known missing-glyph defect in its partial-copy path; see the [current native measurements](/lurq/text-interaction-optimization/#remaining-renderer-cost).

The README Criterion benchmark uses a no-op render engine, so this optimization is not reflected in the CPU-only benchmark table above.

### Atlas Upload Instrumentation

`FrameProfile` now reports glyph atlas upload bytes, upload rect count, and full-atlas upload count. WGPU records the actual source byte range sent through `queue.write_texture`; DX12 records padded upload-buffer bytes, including row-pitch padding required by the copy footprint.

This gives renderer-facing visibility for the dirty-rect path without changing the no-op README benchmark.

### Atlas Upload Probe

The demo app includes an `Atlas Upload Probe` route that warms the atlas with ASCII text, then introduces Latin extensions, symbols, Cyrillic, Greek, currency signs, and arrows over timed updates. This gives the renderer a live scenario where new glyphs are packed after the atlas already exists.

Run it with profiling enabled:

```powershell
cargo run -p demo --features perf_profile -- --atlas-upload-probe --renderer wgpu --profile-log
```

For DX12 on Windows:

```powershell
cargo run -p demo --features perf_profile -- --atlas-upload-probe --renderer dx12 --profile-log
```

The default log path is `target/perf_profile.log`. Look for `atlas=<bytes>B <rects> rects <full> full` in presented frames. Warm steady frames should report zero atlas upload bytes; timed glyph-introduction frames should report dirty rect uploads instead of repeated full-atlas uploads.

DX12 probe run from June 16, 2026:

| Frame | Atlas Upload | Upload Rects | Full Uploads | Note |
| --- | ---: | ---: | ---: | --- |
| initial | 1,048,576 B | 1 | 1 | texture creation/full upload |
| steady warm | 0 B | 0 | 0 | no new glyphs |
| Latin/math update | 158,208 B | 22 | 0 | dirty rects only |
| Cyrillic/arrows update | 150,016 B | 24 | 0 | dirty rects only |
| mixed update | 183,552 B | 25 | 0 | dirty rects only |

### Dirty Rect Coalescing

Added a same-atlas-row coalescing pass before `GlyphAtlas` snapshots expose dirty rects to renderers. Adjacent or near-adjacent glyph dirty rects on the same atlas row are merged when the merged area stays within a conservative waste threshold. Separate atlas rows are not merged.

DX12 probe after coalescing:

| Frame | Before | After | Upload Rects Before | Upload Rects After |
| --- | ---: | ---: | ---: | ---: |
| Latin/math update | 158,208 B | 27,904 B | 22 | 2 |
| Cyrillic/arrows update | 150,016 B | 23,808 B | 24 | 1 |
| mixed update | 183,552 B | 39,424 B | 25 | 2 |

All three glyph-introduction frames still reported `0` full-atlas uploads.

WGPU probe after coalescing:

| Frame | Atlas Upload | Upload Rects | Full Uploads |
| --- | ---: | ---: | ---: |
| initial | 1,048,576 B | 1 | 1 |
| Latin/math update | 70,164 B | 2 | 0 |
| Cyrillic/arrows update | 31,278 B | 1 | 0 |
| mixed update | 76,469 B | 2 | 0 |

WGPU uses the atlas slice directly with atlas-width row stride for dirty uploads, so its byte counter represents the source span passed to `queue.write_texture`. DX12 builds compact padded upload buffers per rect, so its byte counter is not directly comparable.

After inspecting WGPU 24's queue path, the WGPU counter was changed to report the internally staged texture bytes: `align(rect.width, COPY_BYTES_PER_ROW_ALIGNMENT) * rect.height`. The renderer still passes atlas-width source slices to `queue.write_texture`, because WGPU already copies rows into an aligned staging buffer internally. Building an additional compact CPU buffer in lurq would reduce the source slice size but add a redundant packing copy before WGPU's own staging copy.

### Markdown Parse Baseline

Added `parse_readme_markdown` cases to the `text_pipeline` Criterion benchmark. The full README parses in about 10 us in short local runs, while the cold full Markdown pass remains around 6 ms. This confirms the remaining cold-path cost is layout, shaping, and glyph rasterization rather than Markdown parsing.

### Non-Selectable Rich Text Display Text

Rich text layout no longer concatenates spans into `TextState::display_text` unless the rich text is selectable. Markdown still paints from the rich text spans directly, and selectable rich text still builds display text for caret and selection behavior.

In the short README run, this moved `cold_readme_markdown_first_pass/all` from the previous measured ~6.40 ms run to ~6.12 ms. The parser-only baseline remained around 10 us.

### Glyph Engine Profiling

Added fine-grained `perf_profile` counters inside `GlyphEngine` for plain text shaping, rich text shaping, packing shaped rich text into atlas glyphs, Swash glyph image lookup, atlas packing, and cached command append time.

Run the README workload with a one-shot text profile:

```powershell
$env:LURQ_TEXT_PROFILE = "1"
cargo bench -p lurq --bench text_pipeline --features "markdown perf_profile" -- --sample-size 10 --warm-up-time 1 --measurement-time 2
```

The June 16, 2026 profile for the full README cold pass before whitespace-cluster skipping showed:

| Counter | Value |
| --- | ---: |
| rich text shaping | ~2.0 ms |
| rich shaped packing | ~2.7 ms |
| Swash lookup | ~2.6 ms / 560 requests |
| atlas packing | ~0.08 ms / 386 packs |
| cached append | ~0.12 ms |

This points the remaining cold-path work at rich text shaping and Swash glyph image generation. Atlas packing and command append are not currently the dominant costs.

### Rich Shape Phase Profiling

Split the rich text shape profile into buffer acquire, rich text setup, Cosmic shaping, measurement, and glyph extraction. The README profile showed that the expensive part of `rich_shape` was loading rich text into the Cosmic buffer, not shaping:

```text
rich_shape=2.20ms(acq=0.11 set=1.91[prep=0.00 buffer=1.90 align=0.00] cosmic=0.06 measure=0.00 extract=0.11)
```

This means optimizing local span preparation is not enough; the remaining cold setup cost is mostly inside the buffer text loading path.

### Single-Span Rich Text Fast Path

Rich text nodes with exactly one span now delegate measurement and rasterization to the plain text paths. This preserves visual behavior for single-style text blocks while avoiding the rich shaped-layout and rich glyph-layout caches entirely.

The README profile before this delegation showed that most rich loads were single-span:

```text
rich_shape=2.19ms(... loads=59/55+4 spans=87 bytes=1792)
```

After delegating single-span rich text to the plain text path:

```text
text shape=0.88ms rich_shape=0.96ms(acq=0.00 set=0.88[prep=0.00 buffer=0.87 align=0.00] cosmic=0.02 measure=0.00 extract=0.05 loads=4/0+4 spans=32 bytes=853)
```

This removed 55 rich text loads from the README cold pass and reduced glyph-engine memory in the profile from about 170 KiB to about 112 KiB. The single-span work now appears in the plain `text shape` bucket, but total text setup still moves down modestly.

### Whitespace Cluster Raster Skip

Rasterization now skips shaped glyph clusters whose source text is entirely whitespace before asking Swash for a glyph image. Whitespace still participates in shaping, wrapping, measurement, caret positions, and selection geometry; it just does not enter the atlas/raster path because it does not paint.

The README profile moved Swash requests from `560` to `386`, matching the number of successful atlas packs:

```text
swash=2.57ms/386 atlas_pack=0.06ms/386
```

The short Criterion run did not show a clear cold-frame timing win, but this removes known non-painting Swash calls without retaining miss-cache state.

### Realistic Viewport Baseline

Added full README benchmark cases with an 800 px viewport in addition to the tall 20,000 px viewport. The tall viewport keeps the whole document visible; the realistic viewport exercises viewport culling in the render-list path.

Short-run results from June 16, 2026:

| Case | Tall viewport | Realistic viewport |
| --- | ---: | ---: |
| cold full README | ~6.35 ms | ~5.27 ms |
| warm full README | ~56 us | ~33 us |

Profile comparison:

```text
tall:      1253 glyphs, swash=2.61ms/386, glyphs=3.38ms
realistic:  568 glyphs, swash=1.84ms/238, glyphs=2.38ms
```

Viewport culling is already reducing raster work for document-sized content. The remaining realistic-viewport cold cost is still successful Swash glyph image generation for visible glyphs.

### Mask And Atlas Copy Cleanup

Changed glyph coverage extraction to borrow Swash mask bytes for `SwashContent::Mask` instead of cloning them, and changed atlas packing to copy each glyph row with `copy_from_slice` instead of a byte-by-byte inner loop.

The README profile did not move meaningfully from this change; `atlas_pack` remained around 0.06 ms in the tall viewport profile. This confirms the current bottleneck is not mask cloning or atlas memory copy.

### Clipped Identity Text Rasterization

Identity-transform text rasterization now receives the active text clip and uses it while building glyph commands. Plain text and single-span rich text can skip layout runs that are fully outside the clip, and cached glyph command append filters glyph rects against the clip. Multi-span rich text still uses the existing full rich-text raster path.

This did not materially change the README realistic-viewport benchmark because the README workload is already mostly helped by whole-quad viewport culling:

```text
realistic README: 568 glyphs, swash=1.79ms/238, glyphs=2.27ms
```

To cover the case this optimization targets, the benchmark now includes a single large wrapped `Text` node in an 800 px viewport. Before adding a clipped glyph-layout cache, this exposed a warm-redraw problem: clipped rasterization skipped offscreen lines, so it could not safely populate the normal full glyph-layout cache and had to reshape the large text node on every redraw.

| Case | Before clipped cache | After clipped cache | After finite-height raster shape | After tight text layout skip |
| --- | ---: | ---: | ---: | ---: |
| `cold_long_text_realistic_viewport/all` | ~49 ms | ~50 ms | ~28 ms | ~3.4 ms |
| `warm_long_text_realistic_viewport/all` | ~25 ms | ~22.6 us | ~23 us | ~27 us |

Cold profile after tight text layout skip:

```text
long text realistic: total=3.17ms, layout=0.01ms, glyphs=2.44ms, swash=1.01ms/178
```

The clipped raster path keeps Swash generation near the visible glyph set. The clipped glyph-layout cache then makes repeated stable redraws cheap by reusing the visible-line glyph layout for the same text/style/width and clip-relative rectangle.

The raster path also sets a finite Cosmic buffer height from the active clip bottom before shaping, so top-of-document cold rasterization stops after the visible range instead of shaping the whole node again. This moved the cold long-text benchmark from about 50 ms to about 28 ms. Cold first render is now dominated by full-node layout measurement rather than rasterization.

For non-selectable clipped text with fully tight width and height constraints, layout now skips intrinsic text measurement entirely and returns the fixed constraint size. Scroll content still receives unbounded height constraints, so this does not replace full measurement where the parent needs exact content height. In the single huge `Text` benchmark, this removes the remaining full-node measurement cost and moves the cold case to about 3.4 ms.

### Borrowed Plain Text Measurement Lookup

Added flow long-text benchmark cases that place the README text in normal column flow instead of tight viewport-sized constraints. This keeps exact height measurement on the path and gives a baseline for repeated mounts where measurement cache hits matter:

| Case | Before | After borrowed measurement lookup |
| --- | ---: | ---: |
| `cold_flow_long_text_realistic_viewport/all` | ~28 ms | ~28 ms |
| `warm_flow_long_text_realistic_viewport/all` | ~23 us | ~23 us |
| `remount_flow_long_text_same_app/all` | ~50 us | ~36 us |

The measurement cache now uses sampled-text fingerprint buckets with exact borrowed comparison on hits. Cache misses still store owned keys, so correctness does not depend on the fingerprint being collision-free. This does not change first-render cost because full text measurement still has to shape the node once, but it removes the large owned-key clone from repeated mount cache hits.

### Uncached Swash Images For Atlas Misses

Normal glyph rasterization now asks Swash for an uncached image when the glyph is missing from the atlas. The packed atlas entry remains the durable cache used by warm frames, so retaining a second copy of the same successful image in `SwashCache` does not help the normal text path.

Short-run results:

| Case | Before | After uncached Swash image |
| --- | ---: | ---: |
| `cold_long_text_realistic_viewport/all` | ~3.4 ms | ~3.5 ms |
| `warm_long_text_realistic_viewport/all` | ~27 us | ~24 us |
| `cold_flow_long_text_realistic_viewport/all` | ~28 ms | ~28 ms |
| `remount_flow_long_text_same_app/all` | ~36 us | ~38 us |

This keeps the hot-path cache model simpler without a clear regression in the focused long-text run: atlas entries are the cache for paintable glyphs, while Swash image caching is not used as a second layer for normal atlas-backed text.

### Batch Glyph Raster Experiment

Tried collecting unique missing glyph keys for each raster pass, resolving fonts on the main thread, rendering images in scoped worker threads with per-worker `ScaleContext`, and then packing the resulting masks into the atlas sequentially.

The experiment was not kept. In short local runs it regressed cold README and long-text passes:

| Case | Direct atlas miss path | Batch raster experiment |
| --- | ---: | ---: |
| `cold_readme_markdown_first_pass/all` | ~6 ms | ~10.4 ms |
| `cold_readme_markdown_realistic_viewport/all` | ~5 ms | ~8.4 ms |
| `cold_long_text_realistic_viewport/all` | ~3.5 ms | ~4.4 ms |
| `cold_flow_long_text_realistic_viewport/all` | ~28 ms | ~27.7 ms, no useful win |

Thread startup, extra collection passes, and reduced cache locality outweighed the parallel Swash work at the current glyph counts. A future parallel path likely needs persistent workers or a larger async raster queue instead of per-pass scoped thread spawning.

### Outline-First Swash Rasterization Experiment

Tried asking Swash for an outline image first and falling back to color outline/bitmap sources only when the outline path did not produce an image. The goal was to avoid probing color glyph sources on every successful normal-text glyph image.

The experiment was not kept. A short run suggested `cold_long_text_realistic_viewport/all` might improve, but the full Criterion run did not confirm it:

| Case | Full run with outline-first |
| --- | ---: |
| `cold_long_text_realistic_viewport/all` | ~3.67 ms, regressed in Criterion |
| `warm_long_text_realistic_viewport/all` | ~26 us, regressed in Criterion |
| README realistic cold profile | ~4.39 ms, within the existing direct-path range |

The direct Swash source order was restored. Successful Swash generation remains the main visible-glyph cold cost, but this source-order tweak is not a reliable win.

### Flow Text Exact Measurement

The `cold_flow_long_text_realistic_viewport/all` case still spends most of its time in layout measurement:

```text
flow long text realistic: total=28.28ms, layout=22.27ms, text shape=22.24ms
```

This benchmark places one large `Text` node inside normal column flow. The vertical column layout gives non-flex children unbounded height so they can report exact content height. That is the correct contract for normal flow, especially when following siblings, scroll extents, hit testing, or selectable text may depend on the full height.

The tight-constraint measurement skip handles fixed-size clipped text, but it should not be silently reused for normal flow. Reducing this case safely needs an explicit lazy/estimated layout model or an overflow rule that says exact child height is not needed.

### Color Glyph Atlas

Changed the glyph atlas from an alpha-only `R8` texture to RGBA so Swash color glyphs, including emoji, keep their source pixels instead of being converted to an alpha mask and tinted with the current text color. Monochrome glyphs store white RGB plus alpha coverage and still use the text style color in the shader.

This touches both render backends:

- WGPU uses `Rgba8UnormSrgb` for the glyph atlas and reports staged upload bytes using RGBA row sizes.
- DX12 uses `DXGI_FORMAT_R8G8B8A8_UNORM_SRGB` for the glyph atlas and copies RGBA dirty rect rows.

Short-run CPU benchmark tradeoff after avoiding intermediate RGBA allocation per glyph:

| Case | After RGBA atlas |
| --- | ---: |
| `cold_readme_markdown_realistic_viewport/all` | ~6.2 ms |
| `warm_readme_markdown_realistic_viewport/all` | ~32-37 us |
| `cold_long_text_realistic_viewport/all` | ~4.6 ms |
| `warm_long_text_realistic_viewport/all` | ~25.7 us |

The cold path is expectedly higher because atlas rows are now four bytes per pixel. The change fixes color glyph correctness; a future optimization could split monochrome and color glyphs into separate atlas formats if the extra cold cost matters.

### Empty Glyph Miss Cache Experiment

Tried caching glyph cache keys that produced no Swash image or zero-sized placements. In the README profile this reduced Swash requests from 560 to 394, but did not reduce measured Swash time.

The experiment was not kept. The stateless whitespace-cluster skip covers the repeated space-glyph misses in the README workload without adding persistent miss sets. The next optimization should target expensive successful glyph image generation or reduce repeated rich shaping.

## Current Storage Model

Fully shaped plain-text Cosmic buffers are retained across measurement, vertical alignment, and painting. For documents (plain texts of 1,024 bytes and more) this cache holds at most 64 entries under a 48 MiB charged-storage budget; see the [budget comparison and memory accounting limits](/lurq/text-cache-budget/). Identical complete paragraphs can also reuse shaped data within a newly built full buffer. Buffers containing only a clipped prefix are not retained as full layouts.

Unchanged paragraphs can transfer from a compatible prior document version after an edit or width change. Under memory pressure, older entries can discard wrapped layout while retaining paragraph shaping; full layout is reconstructed before use. This lets logical and physical text sizes share the budget at fractional DPI.

Plain texts under 1,024 bytes have a cache of their own (`glyph_engine::shaped_texts`), keyed by what shapes a text: the text, family, weight, slant, font features and tracking in ems. It holds 4 MiB of buffers, least recently used first out. A buffer taken from it for another size, width, wrapping or alignment is laid out again by Cosmic without being shaped again. `GlyphEngineProfile::plain_cache_bytes` and `plain_cache_budget` cover both caches. Cosmic's own `shape-run-cache` additionally keeps the glyphs of shaped words; it is trimmed every 256 passes or newly laid out texts and is not part of lurq's accounting.

The engine additionally stores derived layout data:

- plain text: one `TextEntry` per `CacheKey` (`glyph_engine::text_cache`) with the text's measured size, vertical and optical extents, carets and `Vec<CachedGlyph>`, each filled in by the first request for it; entries not used for 16 passes are dropped when the cache is full
- rich text: `RichTextCacheKey -> Vec<CachedRichGlyph>`
- baked transformed text: `CacheKey -> Vec<CachedTransformedGlyph>`

Markdown input stores rich text spans. It does not store shaped runs.

## Next Candidates

1. Reduce the remaining full-layout cost for unique long documents with incremental layout or an explicit viewport/overflow contract that preserves required flow height.
2. Reproduce and fix DX12's partial atlas upload defect before removing its full-upload workaround. The later [atlas upload report](/lurq/text-atlas-upload/) reduced staging cost to 0.25 ms in its measured scroll fixture; the earlier 3–4 ms figure describes the old staging path, not the current implementation.
3. Reassess persistent workers or GPU glyph generation if new workloads show rasterization dominating; after the dev font dependency changes it is under 1 ms in the measured regular viewport cases.
4. Refresh optimized Criterion results separately from the development-profile measurements above.
5. For screens of many short texts, see "What remains" in [Chat Screen Text Optimization](/lurq/text-chat-optimization/): the first use of a font face an application did not prepare with `App::warm_text_styles` (cosmic-text scores every face of the font database, 4 to 11 ms a combination of family, weight and style), the copy of every text into its quad, and rich text's caches, which the short-text work did not touch.

## Updating This Page

When changing the text pipeline:

1. Run the focused tests.
2. Run `text_pipeline` with the same command before and after the change.
3. Add the benchmark date, command, and main numbers to this page.
4. Note whether the result affects cold first render, warm cached render, or both.
