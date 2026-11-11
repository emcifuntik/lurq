---
title: Profiling sessions
description: Shared bounded CPU profiling sessions, overlapping captures and stalled-UI observations.
---

# Profiling sessions

Profiling is a toolkit service, shared by the embedded MCP server and in-process consumers. It does not require DevTools and does not collect component properties, text, input values, document paths, window titles or tokens. The existing DevTools inspector UI remains separate; a future profiling UI can consume this same session API and model.

Enable `perf_profile` to instrument timings; `devtools` enables it too (see [Overhead](#overhead) for what it costs a pass). MCP profiling tools are available with `mcp`; without `perf_profile`, availability reports `feature_disabled` and starting a session errors. Starting, reading and ending sessions never request redraw, layout, synthetic input or an event-loop roundtrip.

```rust
use lurq::app::profiler::SessionOptions;

let profiling = tree.profiling_handle();
let id1 = profiling.start(SessionOptions::default())?.id;
let id2 = profiling.start(SessionOptions::default())?.id;
let result2 = profiling.end(id2)?;
// Normal application work continues. Session 1 keeps collecting.
let result1 = profiling.end(id1)?;
```

The handle is cloneable and can be moved to a diagnostic/server thread. `read(id)` returns an owned immutable snapshot while leaving that session running. `end(id)` finalizes only the named session. Ending session 2 cannot reset, clear or stop session 1. A finalized report cannot receive a later frame completion.

With `mcp`, use `lurq_profile_start {"max_samples":120}`, `lurq_profile_read {"id":"profile_1"}` and `lurq_profile_end {"id":"profile_1"}`. The returned ID must be used; IDs are not globally fixed. `lurq_profile_read {}` reports feature/build availability and limits. MCP requires its normal bearer token, enabled server, Observe scope and tool permission. These tools execute on the server thread even if the UI is blocked.

## Bounds and lifecycle

There are at most eight concurrent sessions across all consumers, 1–240 retained samples per session (default 120), and 64 tracked windows per application collector. A sample is a completed pass, root rebuild/subtree refresh or input dispatch. Input activity is explicitly a different sample kind and is never counted as a rendered frame. Oldest samples are dropped on overflow; reports include completed, returned, dropped and boundary-excluded counts. Samples shared by overlapping sessions use immutable reference-counted data, rather than cloning their payload for each session. There is no unbounded per-node tracing or prior-history replay on start.

Each session has independent monotonic start and end bounds. Only complete operations that started at or after its start and finished while it remained active are retained. A pass that crosses the start boundary is excluded, even if it later finishes. Nested rebuilds that fit entirely inside the session can still appear as separate samples. Reports count exclusions once those operations complete while a session remains active.

An end during a stall reports current unfinished work separately: toolkit window and prospective frame ID, current phase, elapsed-so-far, phase elapsed-so-far and whether the operation began before the session. These observations are excluded from completed timing samples. They are not fabricated partial stage measurements and cannot be added to completed durations. A session begun in the middle of a slow frame can therefore report that slow unfinished frame, even though no completed pass qualifies. An empty session or a session with unfinished work has an explicit status; it does not imply a fast frame.

Window IDs are toolkit identities (`main`, `w1`, and hierarchical IDs for nested secondary windows), never application-assigned titles or names. IDs are limited to 256 UTF-8 bytes; windows that exceed this or the metadata bound are counted as untracked. Closed-window metadata remains until its bounded slot is needed; retained samples keep their identities. DevTools windows are excluded by default and opt in through `SessionOptions::include_devtools` or MCP's existing `include_devtools` configuration. Separate root trees own separate collectors.

After the root tree closes, surviving handles may read/end existing sessions, but cannot start a new one. Unknown IDs and already-ended IDs error. The last 64 ended IDs are remembered; older IDs may be reported as unknown. MCP can only read/end sessions it started. Revoking Observe, disabling MCP or denying a profiling tool cancels MCP-owned sessions and discards their reports; in-process sessions continue. Regranting permission does not revive cancelled IDs. MCP shutdown cancels its own sessions.

## Two session modes

`start(SessionOptions)` starts a **bounded** session, the one described above and the only one MCP starts: at most `max_samples` samples, oldest dropped first.

`start_recording(RecordingOptions)` starts a **recording** session, meant for a DevTools recording that runs from the user's start to the user's stop:

```rust
use lurq::app::profiler::RecordingOptions;

let profiling = tree.profiling_handle();
let id = profiling.start_recording(RecordingOptions::default())?.id;
// While it runs, a UI can show how much it holds:
let size = profiling.read(id)?.recording.unwrap().size_bytes;
let report = profiling.end(id)?; // Every sample since the start.
```

A recording keeps every sample from its start to its `end`, in chunks of 4096 samples, so a long recording never moves what it holds and never drops a sample. `ProfileReport::recording` (None for a bounded session) reports `size_bytes`, an estimate of the memory its samples hold (the samples with their spans and the session's pointers to them; a sample shared with another session is counted in each), and its ceiling `max_bytes`, 1 GiB by default (`DEFAULT_RECORDING_MAX_BYTES`). When the next sample would pass the ceiling, the recording stops by itself: it keeps what it has, records nothing more, and reports `stopped_at_limit = true` with `stopped_ms`, instead of silently dropping samples. It stays readable until `end`. A recording counts against the eight concurrent sessions like any other, follows the same start/end boundary policy and window/DevTools rules, and its application scopes keep the per-session bound of 128. `max_samples` is 0 in its report (null in JSON), `max_bytes` of 0 is refused with `ProfileError::InvalidByteLimit`, and its JSON export adds a `recording` object (`max_bytes`, `size_bytes`, `stopped_at_limit`, `stopped_ms`); `SessionStarted` carries `recording_max_bytes`.

## Phase spans

A pass that begins while a session takes samples records the span of every phase guard of the pass (the phases listed in [Coverage and units](#coverage-and-units), down to the detailed Canvas phases) into `PassSample::spans`, so a flame chart can be drawn. Each `PhaseSpan` holds the `phase`, its `depth` (0 for the pass itself, then one more for each guard it is nested in) and `start_ns`/`end_ns`, nanoseconds since the collector epoch, the origin of every `*_ms` value in a report. Spans are ordered by start, so a span's parent is the nearest earlier span one level up, and each span lies inside its parent. The first span is the pass's own, ending when the pass completes. A phase around the pass (an input dispatch that runs a nested pass) is not one of its spans.

Spans are written without the collector lock and without allocating: each window has a buffer of `MAX_SPANS_PER_PASS` (1024) span slots of atomics, allocated by the window's first captured pass, which a phase guard fills on exit with one atomic add; spans past it are counted in `PassSample::dropped_spans`. The pass's spans are copied into one `Vec` when it completes. A pass that begins with no session taking samples records no span and allocates nothing for them; a session started mid-pass excludes that pass anyway. JSON adds `spans` (`phase`, `depth`, `start_ms`, `end_ms`) and `dropped_spans` to each pass sample; the version-1 fields are unchanged. Spans hold only phase names and times, never text, values or paths.

## Coverage and units

Toolkit stage timings are **CPU wall-clock milliseconds**; application scopes are explicitly **synchronous wall milliseconds**, including lock/I/O waits. Monotonic timestamps are relative to the collector's creation and are not calendar timestamps. Build metadata reports the toolkit version, target OS/architecture, relevant compiled features and `debug_assertions`. Cargo profile and optimization level are explicitly not embedded; debug assertions are not a reliable substitute for the build command.

| Scope | Actual boundary and interpretation |
| --- | --- |
| Pass total | Entire `Tree::pass`, including setup, update/layout, resolve/raster, render, runtime cleanup, existing observers and MCP notification/reconciliation/reply work. Completion is published after that tail; a blocked MCP notification appears as unfinished `pass_notifications`. Input dispatch or arbitrary host work before the pass is outside it. Idle passes are not retained. |
| Input dispatch | Actual Tree pointer move/down/up/leave, scroll and keyboard down/up entry points, including synchronous application callbacks and subsequent reactive updates. Coarse input phases are visible before any frame pass begins. No pointer coordinates, key/code strings, text or input values are retained. A nested pass/update restores its outer activity phase on completion. Menu/window callbacks outside these Tree entry points and unrelated host task processing are not attributed. |
| Root rebuild / subtree refresh | Retained-tree rebuild/update, including a nested commit that installs nodes, preserves runtime state/IDs and refreshes interaction/focus. An update may happen before or inside a pass. |
| Layout update | Existing `update_layout` plus caret processing. Includes rebuilds, resources, layout compute, Canvas binding/replay and component after-layout hooks. This is an inclusive update scope. |
| Layout compute | Aggregate runtime-owned `LayoutEngine` compute calls, including base/final overlay layout, nested overlay indexing and overlay/menu measurements. Includes UI text work and immediate call argument/lock preparation; nested in layout update. |
| Component after layout | One timer around the entire root plus recursive component hook sweep. Application Canvas preparation, shaping and painting inside these hooks belongs here, not to the Canvas binding timer. Nested in layout update; no per-node trace or component identity retained. |
| Runtime Canvas recording | Canvas binding traversal and its synchronous observers/replay. Component `after_layout` hooks run later and are separately timed. Calls to `CanvasHandle` from unrelated host work outside these boundaries are not independently timed. |
| Canvas preparation | Selecting attached hardware handles before `RenderEngine::render`; it does not represent GPU execution. |
| Quad / glyph work | Existing quad resolution and render-list/glyph construction timers, with UI GlyphEngine cache/shaping counters. The glyph scope includes non-text render-list construction. Document CanvasTextEngine shaping is excluded; zero UI shape counters do not imply absent Canvas text work. |
| Canvas text | Aggregate synchronous `Context2D::measure_text`/`fill_text` attempts and `CanvasTextEngine::shape` calls on the pass thread, including component hooks. Shape hits/misses/evictions, results not kept (`shape_cache_uncached`), the charge above the cache's budget (`shape_cache_stretch_bytes`) and newly produced final RGBA byte counts accompany inclusive shape CPU time. Nested stages cover buffer construction/font resolution/advanced shaping; the entire glyph loop including font metrics, cache-budget scans, Swash lookup/cloning; and final bitmap allocation, conversion/composition and RGBA copy. Cache hits only incur the total/cache path. Engine-lock wait, unrelated/background calls outside the pass and GPU work are excluded. Calls can precede or follow the after-layout sweep, so Canvas text total is a subset of the pass rather than necessarily a subset of that one hook timer. |
| Canvas backend total | Processing Canvas batches, resource/cache maintenance, command preparation, tessellation, uploads and command recording. Lazy Canvas renderer creation is included. WGPU processes Canvas before surface acquire; DX12 processes Canvas inside encode. |
| Canvas tessellation | `Prepared::new`, including command preparation and mesh-cache lookup/misses. Cache hits still perform preparation work. |
| Canvas asset upload | Asset-cache lookup, conversion/resource creation and texture staging for images/text; CPU time, with uploaded asset-byte counts. |
| DX12 asset-upload details | Optional `render.canvas.asset_upload_details` on a captured DX12 Canvas encode. Four disjoint CPU subscopes nest in asset upload: texture creation; row-padded pixel packing; staging plus copy/barrier command recording; and asset SRV-pair writes. Cache eviction is separately timed after drawing, outside asset upload and inside Canvas backend total. WGPU, custom backends and uncaptured/disabled work report null details. |
| Canvas buffer upload | WGPU vertex/global buffer staging; DX12 vertex staging. DX12 per-tile/layer constants remain inside recording rather than adding a clock operation for every constant. |
| Canvas recording | Draw/tile planning and command recording, including nested upload/submission work. Counters report batches, command groups, vertices and tiles without retaining commands. |
| Render init/acquire/encode/submit/present | Existing backend CPU timers. Current-phase observations also distinguish these stages while a pass is unfinished. WGPU encode includes buffer/image upload subtimers; DX12 encode includes Canvas and atlas upload. DX12 UI buffer/image uploads have no independent subtimers and are reported as included in encode. |
| GPU timestamps | **Unavailable**: neither backend adds GPU timestamp queries here. CPU queue submission and presentation calls are never labelled GPU execution time. |

Do not sum inclusive or nested scopes. A UI update may already be in pass/layout time; commit is in UI update; Canvas recording includes upload/submission; DX12 Canvas is in render encode. Existing `FrameProfile::gpu_submit` is retained for compatibility but means CPU time for Canvas preparation plus the render call. JSON states this explicitly.

Memory counters retain the existing cached estimate, sampled at most once per second. They are not an allocation trace or an exact live heap measurement. Zero counters can mean no work for that completed operation; disabled/missing frame data and unsupported stage coverage are reported separately. A custom renderer that provides no `last_profile` reports `render.profile_available=false` and null render CPU timings; Canvas coverage requires an instrumented built-in backend.

`canvas_text` is present on captured passes with the Canvas feature, independently of successful rendering; otherwise it is null. Byte counts are the newly produced final RGBA data length on misses, not total allocation, cache residency, transient glyph pixmaps or upload traffic. Measure calls can shape and produce bitmaps, and scale/color are part of the existing cache key; instrumentation does not change that behavior. A thread-local stack scope attributes synchronous work to its current window/pass, restores an outer pass after nested windows, and never resets the shared text engine. Calls made outside a pass are deliberately unattributed. A pass begun without an active session skips detailed Canvas text capture; a later mid-pass session excludes that crossing pass by the existing whole-operation boundary policy.

DX12 asset detail capture checks for an active session once at Canvas encode entry and accumulates only numeric values in that renderer's current frame profile. Texture creation surrounds the existing cache-miss DEFAULT texture creation. Pixel packing includes allocation/zeroing, row copying and conditional alpha premultiplication. Upload staging/commands includes the mapped arena copy or dedicated UPLOAD resource creation/map/copy/unmap, then CPU `CopyTextureRegion` and resource-barrier recording. These timers do not measure when the GPU executes those commands. Descriptor writes surround the existing two-SRV pair operation for each distinct asset in a command group; backing/layer/composite descriptors and descriptor-heap creation are excluded. Cache lookup and map/COM bookkeeping remain in the inclusive asset-upload residual. The detail stages are not individually published as in-flight phases; a stall there remains visible through `canvas_asset_upload`.

Cache hits/misses count every prepared draw carrying an asset, including repeated references within a group. Successful texture creation, descriptor-pair and arena/dedicated upload counts distinguish resource work from those repeated lookups. `padded_upload_bytes` is the row-padded CPU payload passed to staging, excluding 512-byte placement alignment gaps; the existing `uploaded_asset_bytes` remains unpadded asset payload bytes. Cache charge before/peak/after and entry before/after counters use existing asset-cache accounting (`max(payload length, 64 KiB)` per entry). They are policy charges, not actual D3D12 committed-resource sizes, RSS or live GPU allocation. Eviction count covers every texture evicted during the encode, including those evicted while drawing to make room; eviction time covers the end-of-encode return toward the budget and the retirement of textures that were not kept, which may keep a texture alive until its fence completes.

On both WGPU and DX12, `render.canvas.counts` also reports the renderer's `asset_cache_budget_bytes`; `asset_cache_stretch_bytes`, the most the asset cache was charged above its budget during the encode (above zero, the textures of the current and previous frame needed more than the budget); and `asset_cache_uncached` with `asset_cache_uncached_bytes`, the textures drawn but not kept past the encode because those frames filled the cache's ceiling of twice the budget. `canvas_text` counts report the same for shaped text as `shape_cache_uncached` and `shape_cache_stretch_bytes`.

## Synchronous application scopes

Application work before/around toolkit passes can use the same collector. This is opt-in instrumentation: adding the API does not automatically instrument asynchronous task polls, document projection, persistence, timers or menu callbacks. It changes no rendering, threading or cache policy.

```rust
use lurq::app::profiler::ApplicationLane;

let profiling = tree.profiling_handle();
let write = profiling.application_scope("main", "local_save_write", ApplicationLane::Worker);
{
    let _encode = write.child("container_encode", ApplicationLane::Worker);
    // The application's real synchronous encode call goes here.
}
write.finish(); // Or drop(write), always before an await.
```

The exact public API is:

```rust
ProfilingHandle::application_scope(&self, window: &str, label: &'static str,
    lane: ApplicationLane) -> ApplicationScope;
ApplicationScope::child(&self, label: &'static str,
    lane: ApplicationLane) -> ApplicationScope;
ApplicationScope::parent(&self) -> Option<ApplicationScopeParent>;
ProfilingHandle::application_scope_child(&self, parent: &ApplicationScopeParent,
    label: &'static str, lane: ApplicationLane) -> ApplicationScope;
ApplicationScope::status(&self) -> ApplicationScopeStatus;
ApplicationScope::finish(self);
```

`ApplicationLane::{Ui, Worker}` is declared at the actual callsite, not inferred from an OS thread. The cloneable handle and opaque parent token are Send + Sync. Guards are neither Send nor Sync; this prevents moving a running guard between threads, but does **not** prevent holding one across an await in a local/non-Send future. Callers must end each guard before awaiting. Use a cloned handle for a new worker operation. A parent token permits an explicit child only in the same collector while its parent remains live at child entry; a token to an ended, closed or replaced parent is refused. A child inherits the registered window and gets an opaque monotonically increasing ID, parent ID and depth. A parent may finish before its worker child: the relation records live-at-entry nesting, not guaranteed full time containment. There is no global/TLS application parent and concurrent scopes never overwrite the existing UI phase.

Use only content-free static source labels of 1–64 ASCII bytes, limited to letters, digits and underscore. Never intern text, document/node IDs, filenames, font names, paths, tokens or input values as labels. The window must already be registered by the toolkit (`main` for the root tree); applications cannot create profiling window identities. Invalid label, unknown/closed window, closed collector, exhausted live slots or invalid parent produces an inert guard with an explicit `status()`. Instrumentation refusal must not stop the application operation. The public `ProfilingHandle::new()` has no registered window; use the handle from the actual `Tree` (including a headless Tree in contract tests).

`ProfileReport::application_scopes` is an optional separate typed report (None without `perf_profile`); JSON adds `application_scopes` without changing the existing `samples` array or its pass/input/update counters. Reports have at most **64 live scopes per collector** and **min(session.max_samples, 128) completed application scopes per session**, separately from the existing sample bound. History drops the earliest-published application records. Worker completion timestamps are captured before acquiring the collector lock, so publication order can differ from timestamp order; application sample age uses the maximum completion timestamp among the returned bounded records, not the last published entry. Each report includes its own completed/returned/drop/truncation/age metadata, started count, boundary exclusions, refusal counts and abandonment counts. Application-only captures have a completed/unfinished status even when the old `samples` array is empty. The old top-level sample age and truncation fields still describe only that existing array; application age/truncation describe the new history. `lurq_profile_read {}` exposes application feature availability and bounds through build metadata. MCP authentication, Observe scope and session ownership are unchanged.

Application scopes are **inclusive synchronous wall milliseconds**, including application lock waits and synchronous I/O, not measured thread CPU or GPU execution. Samples use `wall_timings_ms.total`. They can overlap a pass/input/update, other lanes, or explicit child scopes; do not add them together or subtract them from process CPU as exact attribution. Their timestamps share the collector epoch. Read/end take only the bounded collector lock, never an application/core lock or UI completion wait. Completed histories and live observations are copied into immutable report data; JSON export occurs after the collector lock is released.

Every session independently admits whole scopes started after its own start and completed before it ends. A session started during an idle-tracked slow scope sees `started_before_session=true` and unfinished elapsed-so-far, but excludes its entire later completion. A child begun after session start may qualify even when its parent does not; its parent ID then has no completed parent record in that report. Ending session 2 does not stop a live scope or session 1, and a late completion cannot change the finalized session-2 report. Ending/revoking an MCP session removes only that session's membership; in-process sessions and live work continue. Closed/replaced windows remove their live slots and increment `abandoned_window_closed`; root shutdown removes remaining live slots and increments `abandoned_producer_closed`. Late drops cannot resurrect them. An abandoned scope that began before session start increments both its boundary-exclusion and abandonment counter; these are overlapping accounting categories, not completed durations. No fabricated completion/partial duration is retained on abandonment.

With the feature disabled, scope entry/child/drop performs no clock, collector lock or history allocation. Enabled idle entry/drop uses a timestamp and short lock around the fixed live-slot store so a new session can see already-running work. Window identities are shared from registered `Arc<str>` storage, with no per-scope String/Arc allocation. No completed payload/history is allocated until at least one active eligible session accepts a completion. Read/export allocate only bounded owned snapshots. Enabled idle/active collection overhead remains **unmeasured** until an actual controlled check; this API does not imply a performance improvement.

## Cost and interpretation

Collection has fixed bounds; locks are never held around UI/backend work or JSON serialization. Each window publishes its current coarse phase (phase, frame id, pass start, phase start) in its own fixed slot of atomics, written by phase entry and exit without the collector mutex and read by `read`/`end` under it, so a report never sees a half-written phase and a pass never waits for a reader. The collector mutex is taken only to record a completed operation while a session is active. Immutable phase context/window IDs use `Arc<str>`, so marker/context copies do not allocate per frame. A completed payload is allocated once, then referenced by up to eight sessions. With no active session, no completed sample payload is retained or allocated and detailed per-Canvas-group phase markers are skipped. Compiled `perf_profile` still runs the existing timers and coarse input/pass/layout/Canvas-backend/render phase markers so a session started mid-stall can observe current work. A session begun during an already-running Canvas group may initially observe the coarser Canvas-backend phase. Without that feature, runtime timing hooks are compiled out.

### Overhead

`cargo bench -p lurq --bench profiler_idle` measures a headless `Tree::pass` of a fixed scene in the three builds: run it without features for `off` and with `--features perf_profile` for `idle` (compiled in, no session) and `session` (one active session recording every pass). `devtools` enables `perf_profile`, so every DevTools build runs as `idle` until a session starts. Measured on a Windows development machine, release build, whose timings vary between runs by up to 3x (ranges are several runs of the same session):

| Scene | `off` | `idle` | `session` |
| --- | --- | --- | --- |
| `static`: 40 unchanged rows, layout served from cache | 0.31–0.34 µs | 0.52–0.61 µs | 0.66–0.84 µs |
| `signal_change`: one of 40 row components refreshed, then laid out | 20–28 µs | 20–23 µs | 21–26 µs |

So compiled-in profiling costs a pass about 0.2 µs with no session (the pass timers, a few clock reads and one slot write per phase entry and exit) and about 0.15–0.2 µs more with an active session (recording the sample). On a pass with real work the difference is below the run-to-run noise. Before phases were published without the mutex, the same `static` pass measured 0.68 µs `idle` and 0.90 µs `session` in runs interleaved with 0.59–0.61 µs and 0.82–0.84 µs for the slot.

Canvas text uses one active-session check at the pass boundary and a fixed numeric thread-local accumulator: no per-glyph clocks, collector locks, text/key payloads or allocations for profiling. Shape stages add coarse clocks only for captured passes. An unfinished text call remains visible through its enclosing coarse phase (such as `component_after_layout`); incomplete text-stage totals are never published as completed pass metrics.

DX12 upload details add clocks at whole resource/packing/staging/descriptor operation boundaries only during a captured encode, without extra collector locks or asset identifiers. With no active session at encode entry, no new detailed clocks run and the optional payload stays absent. If sessions end during an encode, its local detail accumulation can finish; the existing whole-operation/session bounds still prevent a late completion from entering a finalized report. A session started mid-pass retains the same boundary-exclusion policy even if Canvas encode begins later. Enabled collection overhead remains unmeasured until an actual controlled runtime check.

No frame is forced to obtain a sample. This is an on-demand renderer: an idle interval without samples is expected and a frame count divided by capture duration is not GPU throughput or FPS. Use pass cost, pending phase/age, request latency and named workloads for diagnosis. Collection overhead and release-versus-development performance require measurements on the actual binary/fixture; source instrumentation alone is not such evidence.

With the `serde` or `mcp` feature, `SessionStarted::to_json` and `ProfileReport::to_json` provide the same version-1 content-free export used by MCP. Finalized `sample_age_ms` is the age at end; a live read computes age at that read. Applications should retain the source SHA, binary/build command and fixture name beside exported reports.

## Layout trace

To find what a layout pass lays out again, and why, set `LURQ_LAYOUT_TRACE` to a file path in a build with `perf_profile`. Each `LayoutEngine` compute (a pass; the overlay host's second compute in a frame is a pass of its own) appends one tab-separated line per node it lays out without its cache, then a summary line, and flushes, so a windowed build without a console works:

```text
layout  pass=12  reason=local-dirty  kind=column  tag=Column  id=turn-4  component=TurnView  children=3  constraints=0.0..720.0x0.0..inf
pass    pass=12  root=OverlayHost  laid_out=41  hits=18  repairs=6  ms=1.284
```

`reason` is `local-dirty` (the node changed, or was never laid out), `descendant-dirty` (a descendant changed and the cached result could not be repaired in place), `no-cached-result` (its cache was not carried over from the node it replaced), `other-constraints` (cached under other constraints only), `cached-tree-mismatch` (the cached result no longer fits its children), `collapsed`, a `repair-*` reason (repairing a dirty child had to lay the node out again) or `direct`. `component` is the tag of the nearest component root at or above the node. `hits` counts nodes served whole from their cache, `repairs` nodes kept while dirty children were laid out again. Compare traces of two builds on the same scenario rather than timings, which vary between runs.

The trace writes tags, element ids, sizes and constraints, never text or input values. It is a development tool, not part of profiling sessions: without `perf_profile` it is compiled out, and without the variable it costs one branch per node laid out.
