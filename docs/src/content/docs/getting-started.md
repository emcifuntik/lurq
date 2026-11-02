---
title: Getting Started
description: Feature flags, demo commands, and the smallest useful lurq app.
---

# Getting Started

This guide targets **lurq 0.53.0**. Add the crate with the shell and renderer used by the example below:

```toml
[dependencies]
lurq = { version = "0.53.0", features = ["winit", "wgpu"] }
```

Use a current stable Rust toolchain; the workspace uses edition 2024. In a source checkout, the library lives in `crates/lurq`, the demo in `crates/demo`, and macros in `crates/lurq_macros`.

## Run The Demo

The demo is the best executable reference because it exercises layout, sizing, positioning, scrolling, input, events, reactivity, components, contexts, animation, transforms, resources, and DevTools.

```powershell
cargo run -p demo
```

On Windows, the demo can also use the DirectX 12 renderer:

```powershell
cargo run -p demo -- --renderer dx12
```

The demo manifest already enables its UI and renderer features. Its default renderer is `wgpu`; pass `--renderer wgpu` or `--renderer dx12` to choose explicitly. Add `--features perf_profile` for frame timings or `--features mcp` for the demo's MCP integration.

## Feature Flags

`lurq` has no default features. Optional subsystems are enabled explicitly:

| Feature | Enables |
| --- | --- |
| `winit` | The `WinitWindow` shell and desktop event loop integration. |
| `render` | Shared render data types. Usually enabled through `wgpu` or `dx12`. |
| `wgpu` | WGPU render engine. |
| `dx12` | DirectX 12 render engine on Windows. |
| `raster` | Raw RGBA/native image transport, `Image`, `StreamingImage`, and `Video`; no image codecs. |
| `canvas` | Persistent Canvas 2D, including gradients and effects; enables `raster` and `render`. |
| `image` | `Image`, background images, and image decoding. |
| `svg` | `Svg` and SVG tessellation/rendering. |
| `resources` | Async local/remote resource loading. |
| `form` | Form handles, validation, submission, and compound form controls. |
| `router` | Routes, links, outlets, and navigation history. |
| `markdown` | Markdown parsing and the `Markdown` component. |
| `i18n` | Translation resources, locale switching, and reactive lookups. |
| `serde` | JSON translation resources when combined with `i18n`. |
| `persistent_storage` | Typed app storage, with in-memory defaults and optional `redb` files. |
| `query` | Shared async reads, cache retention, and typed invalidation. |
| `tokio` | Optional execution on a Tokio handle configured on `App`. |
| `perf_profile` | Frame timing/memory instrumentation and `Tree::last_profile`; independent of `devtools`. |
| `devtools` | Component metadata, signal values, profiler data, and the DevTools secondary window. |
| `screenshot` | GPU capture of the next fully composed window frame to PNG. `devtools` enables it automatically. |
| `mcp` | Embeddable [MCP server](../mcp/) so AI agents can drive and inspect a running app. Off by default; nothing listens unless the app calls `Tree::enable_mcp`. |
| `clipboard` | System clipboard integration for text input copy, cut, paste, and selectable text copy shortcuts; reading text, images and files from the clipboard. |

When `devtools` is enabled, component props and signal values must implement `DevtoolsInspectable`. Derive it on structs and enums you want to inspect:

```rust
#[derive(Clone, PartialEq, lurq::DevtoolsInspectable)]
struct CardProps {
  title: &'static str,
  count: i32,
}
```

## Dev Profile

Text shaping, font parsing, and glyph rasterization run in dependencies (cosmic-text, harfrust, swash, skrifa, fontdb, and others). At `opt-level = 0` they make text-heavy screens noticeably slow in debug builds. Cargo reads `[profile]` sections only from the workspace being built and ignores the ones in dependencies, so lurq cannot set this for you. Add it to your workspace's root `Cargo.toml`:

```toml
# Optimize every dependency in dev builds; your own crates stay at opt-level 0 and debug normally.
[profile.dev.package."*"]
opt-level = 2
```

The wildcard does not match workspace members, so stepping through your own code is unaffected. The first debug build of the dependencies takes longer; after that they come from the build cache. The wildcard also keeps working when lurq's text stack changes, which a list of individual crates does not.

## Minimal App

Most apps wire three objects:

- `App`: shared runtime services such as fonts, theme, resources, storage, and optional Tokio execution.
- `Tree`: retained UI tree, component state, layout, input, rendering, devtools, and profiling state.
- `WinitWindow`: desktop shell that owns the event loop and forwards window/input events to the tree.

```rust
use lurq::{
  app::{
    App, Tree, WindowIcon,
    component::Component,
    ctx::Ctx,
    wgpu_render::WgpuRenderEngine,
    winit_shell::WinitWindow,
  },
  components::{Column, Text},
  core::Signal,
  node::Element,
};

struct Counter {
  count: Signal<i32>,
}

impl Component for Counter {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self { count: ctx.signal(0) }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let count = self.count.clone();

    Column::new()
      .spacing(12.0)
      .child(Text::new(&format!("Count: {}", count.get())))
      .child(Text::new("Click to increment").on_click(move |_| {
        count.update(|value| *value += 1);
      }))
  }
}

fn main() {
  let mut app = App::new();
  let mut tree = Tree::new();

  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));
  tree.mount_root::<Counter>(&mut app, ());

  WinitWindow::new(app, tree)
    .with_title("lurq counter")
    .with_size(800, 600)
    .with_icon(WindowIcon::from_rgba(vec![255, 0, 0, 255], 1, 1))
    .run();
}
```

## Docs Commands

The documentation site uses Yarn 1.22.22 and Astro Starlight. Node.js 22.12+ is required by the locked Astro version. Run these commands from the repository root:

```powershell
cd docs
yarn install --frozen-lockfile
yarn dev
yarn build
```

The Rust examples are Markdown code blocks: building the site does not compile them. See [Testing](../testing/) for Rust checks.
