---
title: Chat Screen Text Optimization
description: A benchmark of a chat screen of short texts, and what a frame of it costs before and after this round's changes to the text caches, shaping and glyph drawing.
---

> Benchmark report of November 12, 2026, for the changes released in 0.61.0. The numbers describe this fixture, machine and toolchain. See [Text Pipeline Optimization](../text-pipeline-optimization/) for the pipeline as a whole and the earlier rounds, which measured one long document.

## The screen

A chat client draws a few hundred short texts, not one long one: a channel list, a member list and a panel of messages. Its costs are per text (a cache lookup, a draw call, a small paragraph to shape), not per paragraph of a document, so the earlier fixtures did not show them.

`crates/lurq/benches/text_chat/screen.rs` is that screen, 1280 × 2160 logical pixels: about 37 visible messages of one to three wrapped lines with an author name and a timestamp, in a `VirtualizedList`; a channel list of 40 labels and a member list of 50, each in a scroll container; a header and a `TextInput`. Two sizes (14 and 11 px), two weights, an emoji in every sixth message or so, and a hover background on every row. A settled frame holds 206 texts and about 5,700 glyphs. Everything is generated from the message's index, so two builds draw the same screen.

Three tools share the screen and its scripted steps (`text_chat/driver.rs`):

- `cargo bench -p lurq --bench text_chat`: Criterion, one frame per iteration, a no-op renderer.
- The same binary built with `perf_profile,counting_allocator` and run with `LURQ_CHAT_METRICS=<csv>`: a row per frame with the wall time, the phases, the cache counters, the allocations and the sizes of the text caches and the atlas.
- `examples/text_chat_gpu_probe.rs`: the same steps through the wgpu and the native DX12 renderer on a hidden window, with the renderers' own timers, draw calls, GPU time on wgpu, and captures of eight frames whose pixel hashes two builds can compare.

| Scenario | A frame of it |
| --- | --- |
| `cold_first_frame` | The first frame of a new app: every text shaped, every glyph rasterised |
| `cold_first_frame_warmed` | The same after `App::warm_text_styles` prepared the regular and the bold face |
| `cold_second_frame` | The frame after it, when the list has measured its rows |
| `idle_frame` | A redraw that changes nothing |
| `hover` | The pointer moves to another row: a background changes, no text does |
| `scroll_10k` | A 600 px wheel step through 10,000 messages |
| `switch_channel_new` | Every message replaced by those of a channel not seen before |
| `switch_channel_back` | Back and forth between two channels seen before |
| `resize_width` | The window 2 px narrower or wider than the frame before |
| `typing` | A character typed into the input |
| `scale_factor_first_150` | The first frame at 150% after a session at 100% |
| `scale_factor_toggle` | 100% and 150% in turn, both seen before |

## Results

"Baseline" is upstream master with only the benchmark added (the first commit of the round); "final" is the head of the branch. Both builds were measured at the end, in turn, on an idle machine; see [Method](#method-and-reproduction). A cell is the median over runs of the run's median frame, with the lowest and the highest run in brackets.

### CPU pass

`Tree::pass` with a no-op renderer: layout, quads and the glyph list, without a window. Nine runs of each build.

| Scenario | Baseline, µs | Final, µs | Final / baseline |
| --- | ---: | ---: | ---: |
| `cold_first_frame` | 19,692.6 [19,140.6..20,873.6] | 17,184.6 [16,453.4..17,838.4] | 0.87 |
| `cold_first_frame_warmed` | – | 2,509.9 [2,434.3..3,161.3] | 0.13 |
| `cold_second_frame` | 2,925.8 [2,874.8..3,268.3] | 1,604.2 [1,449.4..1,980.3] | 0.55 |
| `idle_frame` | 198.4 [197.3..201.9] | 158.6 [157.6..160.8] | 0.80 |
| `hover` | 200.4 [199.0..202.7] | 159.6 [158.9..160.6] | 0.80 |
| `scroll_10k` | 1,459.4 [1,436.3..1,475.5] | 1,179.8 [1,155.3..1,212.8] | 0.81 |
| `switch_channel_new` | 3,438.8 [3,355.2..3,515.8] | 2,049.6 [2,005.4..2,106.1] | 0.60 |
| `switch_channel_back` | 1,005.9 [998.7..1,017.5] | 961.2 [953.6..968.5] | 0.96 |
| `resize_width` | 1,671.3 [1,636.4..1,700.8] | 490.4 [470.2..500.3] | 0.29 |
| `typing` | 240.2 [238.6..242.1] | 194.3 [193.0..199.6] | 0.81 |
| `scale_factor_first_150` | 4,669.6 [4,622.3..4,723.2] | 1,709.5 [1,630.0..1,794.9] | 0.37 |
| `scale_factor_toggle` | 387.1 [383.9..389.0] | 335.8 [334.1..343.6] | 0.87 |

`cold_first_frame_warmed` is the first frame of an application that prepared its fonts with `App::warm_text_styles` (see [What changed](#what-changed)). The baseline has no such call, so its ratio is against the baseline's `cold_first_frame`. Without that call the first frame is still mostly cosmic-text's first font match (see [What remains](#what-remains)).

The slow frames changed more than the median ones. Over every frame of the nine runs:

| Scenario, frames a build | Baseline, µs: median / p95 / p99 / slowest | Final, µs: median / p95 / p99 / slowest |
| --- | ---: | ---: |
| `scroll_10k`, 1,080 | 1,455 / 2,673 / 3,411 / 4,853 | 1,182 / 1,467 / 1,603 / 1,695 |
| `resize_width`, 540 | 1,674 / 2,965 / 3,746 / 4,166 | 489 / 670 / 758 / 886 |
| `typing`, 540 | 240 / 262 / 1,223 / 1,298 | 194 / 214 / 234 / 267 |
| `idle_frame`, 540 | 199 / 209 / 225 / 246 | 159 / 170 / 184 / 202 |

The baseline's slow scroll frames were of two kinds: a frame that rasterised a new glyph copied the atlas twice, and every 43rd frame or so a cache reached its limit, dropped everything and laid out all visible text again (at most 136 paragraphs shaped in one frame, 24 now; at most 13.3 MB allocated in one frame, 2.3 MB now).

### Phases

Median over the same runs, in microseconds, baseline → final. "Layout" includes measuring text; "glyph list" is everything the glyph engine does to turn the frame's text quads into glyph commands. The four columns after it are parts of those two: shaping (cosmic-text and HarfRust), wrapping (line layout), glyph rasterisation (swash), and writing the glyph commands of texts painted before. Packing new glyphs into the atlas is the last column.

| Scenario | Layout | Quads | Glyph list |
| --- | ---: | ---: | ---: |
| `cold_first_frame` | 15,806.8 → 15,990.0 | 93.0 → 118.7 | 2,382.8 → 1,052.7 |
| `cold_first_frame_warmed` | – → 1,378.3 | – → 91.0 | – → 968.4 |
| `cold_second_frame` | 1,324.7 → 1,072.1 | 116.3 → 117.4 | 471.8 → 398.5 |
| `idle_frame` | 0.4 → 0.4 | 104.0 → 102.8 | 91.2 → 52.8 |
| `hover` | 0.3 → 0.3 | 103.8 → 102.3 | 91.4 → 52.5 |
| `scroll_10k` | 880.0 → 708.7 | 104.5 → 103.6 | 208.8 → 127.4 |
| `switch_channel_new` | 1,987.2 → 1,356.2 | 105.3 → 104.3 | 455.8 → 276.5 |
| `switch_channel_back` | 502.1 → 495.1 | 101.8 → 100.9 | 97.9 → 56.2 |
| `resize_width` | 1,208.2 → 145.9 | 102.0 → 100.2 | 338.9 → 230.4 |
| `typing` | 36.9 → 32.5 | 101.3 → 101.0 | 94.5 → 55.2 |
| `scale_factor_first_150` | 187.2 → 156.3 | 101.2 → 99.8 | 3,328.4 → 1,400.0 |
| `scale_factor_toggle` | 154.0 → 144.4 | 99.3 → 98.8 | 92.8 → 52.4 |

The text work inside those phases. "Measuring" is the part of layout spent measuring text; shaping, wrapping and the extents used for vertical alignment are counted wherever they happen, in layout or in the glyph list:

| Scenario | Measuring | Shaping | Wrapping | Extents | Rasterising | Atlas packing | Writing commands |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `cold_first_frame` | 15,585.9 → 15,679.4 | 11,890.2 → 11,076.0 | 265.3 → 248.5 | 1,379.3 → 127.7 | 562.3 → 550.6 | 72.3 → 27.6 | 62.0 → 138.6 |
| `cold_first_frame_warmed` | – → 1,165.6 | – → 843.9 | – → 140.4 | – → 122.8 | – → 550.4 | – → 27.7 | – → 58.9 |
| `cold_second_frame` | 921.2 → 684.8 | 687.9 → 503.4 | 87.7 → 104.2 | 20.2 → 20.5 | 66.2 → 63.3 | 1.9 → 1.6 | 152.9 → 148.7 |
| `idle_frame` | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 31.2 → 31.1 |
| `hover` | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 31.2 → 30.6 |
| `scroll_10k` | 412.1 → 261.0 | 317.1 → 176.9 | 36.3 → 35.5 | 8.4 → 7.4 | 0.0 → 0.0 | 0.0 → 0.0 | 34.7 → 34.0 |
| `switch_channel_new` | 1,375.7 → 772.6 | 1,167.2 → 548.9 | 124.3 → 112.9 | 62.7 → 33.0 | 0.0 → 0.0 | 0.0 → 0.0 | 37.8 → 33.9 |
| `switch_channel_back` | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 33.6 → 33.2 |
| `resize_width` | 1,115.0 → 64.0 | 894.5 → 0.0 | 90.0 → 52.2 | 14.8 → 12.7 | 0.0 → 0.0 | 0.0 → 0.0 | 31.9 → 31.0 |
| `typing` | 10.4 → 6.5 | 5.6 → 3.5 | 1.1 → 1.1 | 0.4 → 0.4 | 0.0 → 0.0 | 0.0 → 0.0 | 31.3 → 30.7 |
| `scale_factor_first_150` | 0.0 → 0.0 | 1,350.0 → 0.0 | 187.4 → 223.8 | 2,095.1 → 350.5 | 665.7 → 637.1 | 39.7 → 50.5 | 33.6 → 39.8 |
| `scale_factor_toggle` | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 0.0 → 0.0 | 31.2 → 30.6 |

- **Shaping** is gone from a resize and from the first frame at a new scale factor, and halved where text is new: the words of a new channel or of rows scrolling in were mostly shaped before.
- **The glyph list of a frame that changes no text** went from 91 to 53 µs: one cache lookup per text in place of two, no copy of the text, and each glyph command written once.
- **Extents** (where a text sits in its box) no longer shape the text a second time: 1.4 → 0.1 ms in the first frame, 2.1 → 0.35 ms in the first frame at 150%.
- **Quads** did not change, and are now two thirds of an idle frame.
- The cold first frame's 15 ms of measuring is cosmic-text's first font match for the regular and the bold face, not shaping proper; prepared fonts leave 1.2 ms of it (`cold_first_frame_warmed`: layout 1.4 ms, glyph list 1.0 ms, of which rasterising 0.55 ms).

### Allocations and memory

Per frame, from a profiling session with the counting allocator. "Text caches" is the glyph engine's own estimate of what its caches hold at the end of the frame (the atlas is apart).

| Scenario | Allocations | Bytes allocated | Paragraphs shaped | Text caches, bytes |
| --- | ---: | ---: | ---: | ---: |
| `cold_first_frame` | 13,579 → 9,879 | 14,626,608 → 4,698,386 | 274 → 138 | 890,334 → 1,769,947 |
| `cold_first_frame_warmed` | – → 9,014 | – → 4,537,061 | – → 138 | – → 1,775,463 |
| `cold_second_frame` | 6,392 → 7,615 | 12,713,777 → 4,500,080 | 53 → 53 | 1,742,609 → 3,176,873 |
| `idle_frame` | 620 → 414 | 20,335 → 13,568 | 0 → 0 | 1,783,947 → 3,236,509 |
| `hover` | 620 → 414 | 20,335 → 13,568 | 0 → 0 | 1,783,947 → 3,236,509 |
| `scroll_10k` | 3,442 → 3,877 | 1,806,150 → 1,852,624 | 20 → 20 | 3,954,344 → 6,176,774 |
| `switch_channel_new` | 9,876 → 11,928 | 6,192,136 → 5,291,004 | 56 → 54 | 4,503,946 → 8,608,240 |
| `switch_channel_back` | 5,268 → 5,052 | 1,704,674 → 1,697,199 | 0 → 0 | 2,822,062 → 5,002,237 |
| `resize_width` | 4,662 → 1,213 | 2,804,273 → 340,694 | 42 → 0 | 5,105,977 → 6,698,606 |
| `typing` | 768 → 566 | 43,175 → 34,514 | 1 → 1 | 1,451,618 → 3,514,054 |
| `scale_factor_first_150` | 10,371 → 3,192 | 12,616,266 → 694,720 | 181 → 0 | 1,440,737 → 3,522,025 |
| `scale_factor_toggle` | 1,581 → 1,375 | 85,396 → 78,629 | 0 → 0 | 1,436,919 → 3,522,025 |

- Allocations fall where text is reused (a third fewer in an idle frame, a quarter of the baseline's in a resize) and rise by 13% to 21% in the three scenarios that shape new text: cosmic-text's word cache allocates a key per word. The bytes allocated in those frames fall or stay.
- The megabytes a frame allocated before were the two copies of the atlas: 12.7 → 4.5 MB in the second cold frame, 12.6 → 0.7 MB in the first frame at 150%. The first two frames of a new window still allocate about 4.5 MB each, about the size of the 4 MiB atlas; later frames that rasterise a glyph do not.
- The text caches hold about twice what they held: 1.8 → 3.2 MB for the settled screen, 4.5 → 8.6 MB at the worst point of the run (after switching between channels). The short texts of the screen now stay shaped, up to 4 MiB; before, only the last 64 did. cosmic-text's word cache is not in these numbers.

### Renderers

`text_chat_gpu_probe`, RTX 5090, 1280 × 2160 window, release build.

| wgpu | Draw calls | Renderer on the CPU, µs | Encode | Submit | GPU time, µs |
| --- | ---: | ---: | ---: | ---: | ---: |
| `idle_frame` | 210 → 9 | 235.9 [235.9..237.9] → 211.4 [209.9..214.5] | 36.5 → 46.3 | 118.6 → 82.2 | 27.5 → 26.9 |
| `hover` | 211 → 11 | 262.8 [262.2..263.9] → 239.0 [235.9..240.5] | 57.7 → 68.3 | 119.1 → 82.8 | 28.1 → 28.5 |
| `scroll_10k` | 209 → 13 | 330.9 [328.9..359.1] → 291.6 [290.5..293.6] | 100.2 → 103.3 | 128.4 → 88.8 | 30.5 → 30.9 |
| `switch_channel_new` | 209 → 11 | 389.6 [386.0..392.1] → 317.5 [304.0..322.9] | 111.9 → 111.7 | 144.0 → 92.4 | 30.2 → 31.5 |
| `resize_width` | 200 → 11 | 1,662.0 [1,627.3..1,687.2] → 1,561.1 [1,539.5..1,634.5] | 130.6 → 128.8 | 132.4 → 91.9 | 29.4 → 30.2 |
| `typing` | 212 → 12 | 299.1 [293.8..300.4] → 268.5 [267.1..271.8] | 86.8 → 97.9 | 122.3 → 84.5 | 28.3 → 28.8 |
| `scale_factor_toggle` | 202 → 11 | 1,690.5 [1,655.5..1,698.5] → 1,657.2 [1,625.9..1,684.0] | 88.1 → 100.3 | 123.0 → 88.9 | 55.8 → 55.3 |

Three runs of each build in turn, both renderers in each run. wgpu presents with `Immediate`, so a frame's time is its work. One draw per text became one per run of texts; encoding costs about 10 µs more in a frame whose text did not change (the walk that finds what the clips spare) and submitting 36 µs less. The GPU's own time for the UI pass, 27 to 31 µs, did not move: it was never the cost.

For DX12 the table has "encode", the renderer's CPU time to build the frame's command list, which halves. Its total frame time is the wait for the vertical blank and says nothing.

| DX12 | Draw calls | Encode before, µs | Encode after, µs |
| --- | ---: | ---: | ---: |
| `idle_frame` | 210 → 9 | 244.6 [241.2..250.8] | 117.2 [116.7..118.4] |
| `hover` | 211 → 11 | 244.6 [243.4..247.6] | 118.8 [118.5..122.8] |
| `scroll_10k` | 209 → 13 | 272.8 [264.0..276.0] | 127.9 [126.2..132.8] |
| `switch_channel_new` | 209 → 11 | 299.0 [286.8..336.9] | 150.2 [141.1..214.3] |
| `resize_width` | 200 → 11 | 289.8 [258.4..302.0] | 194.8 [194.1..198.3] |
| `typing` | 212 → 12 | 255.7 [252.4..257.9] | 119.6 [116.0..120.2] |
| `scale_factor_toggle` | 202 → 11 | 314.3 [250.6..336.1] | 194.4 [194.2..198.2] |

## What changed

**Texts that follow one another share a draw.** Both renderers issued one instanced draw per text. Texts painted one after another lie one after another in the instance buffer, so one draw over several paints the same instances in the same order. Runs under the same clip join; so do, in the window, runs whose clips cut nothing off them, which are then drawn without a clip. A run is cut when a glyph's painted pixels reach outside its clip's scissor, when the clip is rounded, or when a glyph is transformed; the outermost pixel of a glyph drawn texel for pixel is the atlas's transparent padding and paints nothing, so a clip that cuts only that still spares the run. A rect, image or SVG between two texts ends the draw, as before. The DX12 renderer also reuses its glyph instance buffer between draws. (`render::glyph_batch`)

**A new glyph no longer copies the atlas.** A frame that rasterised a new glyph copied the whole atlas twice (4 MiB each at 1024 × 1024) to hand the renderer a snapshot. The atlas's pixels now live in the `Arc` the snapshots share and are written in place while no snapshot is held; a list kept for the next frame lets go of its snapshot once drawn. About 1.1 ms on every frame that brings a new letter, digit pair or emoji into view.

**One cache entry per laid-out text, found without copying it.** A plain text's size, vertical extents, carets and glyphs were five caches, each with its own copy of the text as a key; drawing a text looked two of them up every frame, one of them after copying the text into a `String`, both with SipHash. Each cache dropped everything on reaching 1,024 entries, so while scrolling every 43rd frame or so laid out all visible text again. They are one cache now: a lookup hashes the text once with a fast hash seeded per engine and compares the key in place; an entry remembers the pass that last used it, and a full cache drops only the entries no pass used for 16 passes. The hash of a text over 384 bytes reads its first, middle and last 128 bytes, so two such texts of one length that differ between the samples have one fingerprint. The round kept one entry per fingerprint, and two such texts shown together were laid out again on every frame; since the release after it the first holds the place of the fingerprint and each of the others is kept under the hash of its whole text, which only those texts pay for. (`glyph_engine::text_cache`)

**A short text is shaped once.** A plain text was kept shaped under its whole key: text, face, size, width, wrapping and alignment. Shaping depends on the text and its face only (family, weight, slant, features, tracking in ems); size, width, wrapping and alignment place the shaped glyphs. Texts under 1,024 bytes are now kept shaped by that, and a kept text asked for with another width or size is laid out again by cosmic-text, not shaped again: a resized window, a new scale factor, and at a fractional scale the layout in logical pixels and the paint in physical ones. The cache holds 4 MiB of buffers and drops the ones used longest ago. Longer texts keep the document cache of the earlier rounds, paragraph reuse included. A short text is also always laid out whole when painted, so its cached glyphs serve every position of the clip it scrolls under. (`glyph_engine::shaped_texts`)

**A word shaped in one text is not shaped again in the next.** cosmic-text shapes a paragraph word by word. Its `shape-run-cache` feature keeps the glyphs of each shaped run by its text and attributes; lurq now enables it, and trims it (cosmic-text never does): every 256 passes or 256 newly laid out texts the words not used since two trims ago go, and a change of fonts empties it.

**The atlas finds a glyph with the fast hash.** Laying a text out to be painted asks the atlas where each glyph is; those two maps now use the text cache's hasher in place of SipHash.

**A text painted before costs one lookup and one write per glyph.** Each frame looked every text up twice (where it sits in its box, then its glyphs) and wrote its glyph commands twice (appended, then walked again to set paint order and clip). One lookup and one write now; transformed text is unchanged.

**An application can prepare its fonts.** The first text of each family, weight and slant makes cosmic-text score every face of the font database (`FontSystem::get_font_matches`), mapping the file of every face whose weight differs to look for a weight axis: about 4 ms for the first regular text and 10 to 11 ms for the first bold one with this machine's fonts, on the frame that first shows it. cosmic-text keeps the answer, and nothing in lurq can make the first one cheaper, but it can be asked for early: `App::warm_text_styles(&[regular, bold])` measures a sample in each style. It blocks for as long as the frame would have, so an application calls it on a thread of its own (`App` is `Send`) after loading its fonts, while the window and the renderer are created. Nothing calls it by default.

## Tried and not kept

- **Joining only runs under the same clip** left 172 of 210 draws: every row of a list has its own clip. Sparing the runs a clip does not cut brought that to 65, and counting the transparent padding to 9.
- **A two-generation text cache** (drop the older half when full) was replaced before it landed by the pass stamps: a frame that shows more texts than a generation holds would have dropped texts it had just laid out.
- **Rounding glyph positions with an inline function** in place of `f32::round` made writing the commands slower: 30.5 → 34 µs an idle frame.
- **The word cache's worst case** was measured, and the cache kept. With every word of every message made unique (nothing to reuse but spaces and the ellipsis) the same scenarios were 1% to 7% slower than without the cache: scroll 1,337 → 1,350 µs, channel switch 3,052 → 3,161 µs, second cold frame 2,367 → 2,528 µs. With the fixture's vocabulary of 106 words they were 12% to 22% faster, and real messages repeat their words too.

## What remains

1. **The first use of a font face** an application did not prepare: 4 to 11 ms on the frame that first shows a new family, weight or slant, and most of the first frame of an application that does not call `App::warm_text_styles`. The cost is cosmic-text's: whether a face has a weight axis never changes, and remembering it per face there would make every match after the first cheap. lurq could also warm the theme's faces by itself when a window starts.
2. **Quads**, about 100 µs of every frame and unchanged: the walk that resolves the tree into quads copies each text and its style (`QuadContent::Text` owns a `String`, which is public API). It is now most of an idle frame.
3. **Shaping new text**: a channel never seen before still shapes about 55 paragraphs, 0.55 ms with the word cache, and a scroll step about 20, 0.18 ms. Shaping off the UI thread, or ahead of the scroll, would move it out of the frame.
4. **Laying out new rows** while scrolling: about 0.4 ms of a scroll frame is mounting rows and the layout engine, not text.
5. **DX12 uploads**: the DX12 renderer uploads the whole atlas whenever it changes (the workaround of the earlier rounds stands), and writes every glyph instance each frame; wgpu uploads the changed rects.
6. **Rich text** (Markdown, styled spans) keeps its own caches, which still hash with SipHash and empty at 1,024 entries. A chat that renders messages as rich text gets the draw batching and the atlas change, not the cache ones.

## Behaviour that differs

- A glyph whose ink reaches into the clip from a line that lies outside it is now painted whenever its text is (texts under 1,024 bytes). Before, it was painted only if the text had first been painted unclipped, and dropped if the text was first painted while the clip cut it.
- Memory: a screen's short texts stay shaped, up to 4 MiB, where the last 64 did; `GlyphEngineProfile::plain_cache_bytes` and `plain_cache_budget` include them. cosmic-text's word cache is not in lurq's estimate.
- `RenderProfile::draw_calls` is lower for the same frame. What is drawn is the same: every capture of the probe has the hash it had.

Nothing public was removed or changed in signature. One method was added, `App::warm_text_styles`.

## Method and reproduction

Windows 11 Pro (26200), AMD Ryzen 9 7950X, 192 GB, NVIDIA GeForce RTX 5090, rustc 1.97.1, `x86_64-pc-windows-msvc`, release profile (`cargo bench`, `cargo run --release`). The executables of both builds were saved, then run in turn with nothing else building: three rounds, the order of the builds alternating between rounds. The metrics binary runs every scenario three times a round, each in a new app, so a CPU cell has nine runs; a Criterion cell has three.

The GPU probe ran in the same rounds, once a build a round, so a renderer cell has three runs. It needs the system to be composing windows: with every display asleep the DX12 renderer, which waits for the compositor before each frame, draws none. `LURQ_PROBE_CAPTURES_ONLY=1` then still compares what is drawn (it hands the renderer only the captured frames), without times.

The Criterion numbers below are lower than the metrics binary's for the same frame: that binary is built with the profiler and the counting allocator. Compare a build with a build, not one table with the other.

```powershell
# Criterion
cargo bench -p lurq --bench text_chat -- --warm-up-time 1 --measurement-time 3
cargo bench -p lurq --bench text_pipeline --features markdown -- --warm-up-time 1 --measurement-time 3
python scripts/summarize-text-chat.py --criterion run1.txt run2.txt run3.txt

# Phases, allocations and cache sizes
cargo bench -p lurq --bench text_chat --features perf_profile,counting_allocator --no-run
$env:LURQ_CHAT_METRICS = "target/text-chat/metrics.csv"; <the bench executable cargo printed>
python scripts/summarize-text-chat.py target/text-chat/metrics.csv

# Renderers and captures
cargo run --release -p lurq --example text_chat_gpu_probe --features screenshot,wgpu,dx12,perf_profile -- target/text-chat/gpu
python scripts/summarize-text-chat.py target/text-chat/gpu/frames.csv
python scripts/compare-text-chat-captures.py before/captures.csv after/captures.csv
```

### Criterion

The same frames under Criterion, without the profiling features, three runs of each build, median of the point estimates in microseconds:

| Benchmark | Baseline, µs | Final, µs | Final / baseline |
| --- | ---: | ---: | ---: |
| `cold_first_frame` | 19,802.0 [19,616.0..20,198.0] | 16,638.0 [16,439.0..16,686.0] | 0.84 |
| `cold_first_frame_warmed` | – | 2,346.3 [2,304.7..2,361.7] | 0.12 |
| `cold_second_frame` | 2,897.7 [2,891.5..2,898.8] | 1,444.4 [1,417.9..1,505.1] | 0.50 |
| `idle_frame` | 190.5 [187.3..190.6] | 148.8 [148.7..150.9] | 0.78 |
| `hover` | 192.9 [189.4..193.4] | 151.4 [150.8..152.9] | 0.78 |
| `resize_width` | 1,621.1 [1,620.7..1,626.0] | 415.4 [411.5..416.6] | 0.26 |
| `typing` | 218.1 [215.4..221.2] | 176.2 [175.6..178.3] | 0.81 |
| `switch_channel_new` | 3,014.0 [2,990.4..3,055.4] | 1,864.8 [1,860.5..1,911.4] | 0.62 |
| `switch_channel_back` | 888.9 [877.4..895.3] | 829.6 [828.5..836.5] | 0.93 |
| `scale_factor_toggle` | 354.4 [353.4..357.6] | 309.3 [309.0..310.8] | 0.87 |
| `scale_factor_first_150` | 4,531.3 [4,477.3..4,623.2] | 1,658.0 [1,649.1..1,853.7] | 0.37 |
| `scroll_10k` | 1,002.3 [1,001.6..1,019.4] | 702.4 [693.0..707.0] | 0.70 |

`text_pipeline`, the Markdown document benchmark of the earlier rounds, the same way. It was not this round's subject, and nothing in it got slower: `parse_readme_markdown` does not reach the text engine, and its 0.2 to 0.8 µs are where the code landed in this binary (the build before the last commit had it within 1% of the baseline).

| Benchmark | Baseline, µs | Final, µs | Final / baseline |
| --- | ---: | ---: | ---: |
| `parse_readme_markdown/32` | 5.9 [5.9..5.9] | 6.1 [6.1..6.3] | 1.04 |
| `cold_readme_markdown_first_pass/32` | 28,543.0 [28,253.0..28,551.0] | 26,134.0 [26,048.0..26,298.0] | 0.92 |
| `warm_readme_markdown_cached_pass/32` | 36.2 [35.9..36.3] | 32.2 [32.2..32.2] | 0.89 |
| `parse_readme_markdown/128` | 14.9 [14.9..15.0] | 15.5 [15.4..15.6] | 1.04 |
| `cold_readme_markdown_first_pass/128` | 30,993.0 [30,921.0..31,114.0] | 28,088.0 [27,917.0..29,043.0] | 0.91 |
| `warm_readme_markdown_cached_pass/128` | 92.6 [92.3..93.4] | 84.6 [84.6..84.7] | 0.91 |
| `parse_readme_markdown/all` | 14.9 [14.8..15.0] | 15.7 [15.5..15.7] | 1.05 |
| `cold_readme_markdown_first_pass/all` | 30,924.0 [30,766.0..31,092.0] | 28,003.0 [27,977.0..28,265.0] | 0.91 |
| `warm_readme_markdown_cached_pass/all` | 92.6 [92.1..92.6] | 84.9 [84.6..85.0] | 0.92 |
| `cold_readme_markdown_realistic_viewport/all` | 29,274.0 [29,255.0..29,610.0] | 27,459.0 [27,398.0..27,510.0] | 0.94 |
| `warm_readme_markdown_realistic_viewport/all` | 32.2 [31.9..32.3] | 29.0 [28.9..29.0] | 0.90 |
| `cold_long_text_realistic_viewport/all` | 16,473.0 [16,251.0..16,707.0] | 15,170.0 [14,992.0..15,601.0] | 0.92 |
| `warm_long_text_realistic_viewport/all` | 21.3 [21.2..21.3] | 21.2 [21.1..21.6] | 1.00 |
| `cold_flow_long_text_realistic_viewport/all` | 16,698.0 [16,561.0..16,913.0] | 15,416.0 [15,141.0..15,561.0] | 0.92 |
| `warm_flow_long_text_realistic_viewport/all` | 24.8 [24.7..25.0] | 24.3 [24.1..24.5] | 0.98 |
| `remount_flow_long_text_same_app/all` | 51.2 [51.0..52.8] | 48.7 [48.6..50.4] | 0.95 |
| `cold_unique_long_text_realistic_viewport/all` | 36,221.0 [36,082.0..36,677.0] | 29,857.0 [29,725.0..30,273.0] | 0.82 |
| `edit_document_paragraph` | 3,007.7 [2,956.2..3,052.9] | 2,817.4 [2,732.4..2,930.9] | 0.94 |
| `resize_document` | 3,295.0 [3,291.4..3,370.2] | 3,203.0 [3,115.0..3,238.6] | 0.97 |
| `scroll_document` | 4,108.8 [3,962.0..4,203.6] | 2,949.4 [2,945.1..3,066.5] | 0.72 |
| `edit_selectable_document` | 3,562.4 [3,506.0..3,609.0] | 3,367.2 [3,304.1..3,610.3] | 0.95 |
| `resize_selectable_document` | 3,909.5 [3,899.7..4,111.7] | 3,917.2 [3,790.2..3,973.8] | 1.00 |
| `scroll_selectable_document` | 4,906.2 [4,804.5..4,911.6] | 3,702.8 [3,698.8..3,972.3] | 0.75 |

## Validation

- The three commands of `scripts/check.ps1` pass at the head of the branch. `cargo test -p lurq --features markdown,canvas,query,tokio,svg,image,screenshot,form,router,i18n --no-fail-fast`: 2,438 tests in 28 binaries, 8 ignored. `cargo test -p lurq --features devtools,mcp,perf_profile,counting_allocator --no-fail-fast`: 2,056 tests in 30 binaries, 12 ignored. `cargo test -p lurq --release --test layout_stack_depth`: 2 tests. The second command did not compile on master (a test's props lacked `DevtoolsInspectable`); the round fixes that test.
- The GPU readback tests, which are ignored by default: `cargo test -p lurq --features screenshot,wgpu,dx12,raster,perf_profile --lib -- --ignored --test-threads=1 readback glyph_batch`, 20 tests. Among them a scene of clipped, rounded, cut, overlapping and unclipped runs of text is drawn batched and run by run through wgpu and through DX12, and the frames are compared pixel for pixel.
- The probe's 16 captures (eight frames: settled, hovered, typed, channel switched, resized, at 150%, back at 100%, scrolled; both renderers) have the same pixel hashes at the baseline and at the head, in each of the three rounds. The two renderers' captures of a frame differ by at most one level a channel.
- New unit tests cover the rules for sharing a draw, the text cache (colliding fingerprints, what a full cache drops, growth), a kept shaped text laid out again against a freshly shaped one for other widths, sizes, wrapping and alignments, the shaped texts' budget, the atlas's copy on write and its release after a frame, painting with one lookup against the two calls it replaces for every vertical alignment, and fonts prepared on another thread.
- The branch was then merged with master (43 commits: the iOS navigation work, the system's text size and bold text in the theme, a variable font drawn at the weight asked for). Master's bold text changes the weight a face is matched at and empties the text caches when the setting changes; the caches of this round are among those it empties, a test sets and unsets it against them, and `App::warm_text_styles` prepares the weights it draws. Master's text size scales the typography's styles, which a kept shaped text is laid out again for. Nothing in those commits touches a renderer. At the merged head the three commands pass (2,499, 2,089 and 2 tests), the 20 GPU readback tests pass, the 16 captures still have the baseline's hashes, and the chat metrics (three runs) are inside the spread of the tables above for every scenario: an idle frame 158.6 µs, a wheel step 1,167 µs, a resize frame 490 µs.
- Not run: macOS, iOS, Linux and Android. The renderer changes are in the wgpu and DX12 renderers only; the cache, shaping and atlas changes are platform-independent code.
