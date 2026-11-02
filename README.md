# lurq

`lurq` is a Rust UI toolkit with typed component builders, retained runtime state, reactive signals, GPU-backed rendering, and an in-app DevTools window.

## Install

```toml
[dependencies]
lurq = "0.53.0"
```

There are no default features. Enable the window shell and renderer to run the example below:

```toml
[dependencies]
lurq = { version = "0.53.0", features = ["winit", "wgpu"] }
```

Useful optional features:

| Feature | Purpose |
|---------|---------|
| `winit` | Window shell integration |
| `wgpu` | WGPU renderer |
| `dx12` | DirectX 12 renderer on Windows |
| `raster` | Raw image/video transport and composition without image codecs |
| `image` | Image components and image-backed styles |
| `svg` | SVG components |
| `canvas` | Persistent Canvas 2D drawing through existing element refs; paths, gradients, shadows, blur, blend modes, isolated layers, text, clipping, and raw images |
| `resources` | Resource loader and resource-backed images/SVGs |
| `query` | Shared typed async queries, caching, and invalidation |
| `form` | Form state, validation, and compound form controls |
| `router` | In-process routing, nested layouts, and navigation |
| `markdown` | Markdown parsing and components |
| `i18n` | Reactive translations and locale switching |
| `serde` | JSON translation-resource loading with `i18n` |
| `persistent_storage` | Typed in-memory or file-backed app storage |
| `tokio` | Run component futures, streams, and queries on a configured Tokio runtime |
| `clipboard` | Clipboard shortcuts for text inputs; reading text, images and files from the clipboard |
| `devtools` | In-app DevTools window |
| `mcp` | Embeddable MCP server for AI agents to drive and inspect a running app |
| `screenshot` | Capture the next fully composed window frame to PNG; enabled automatically by `devtools` |
| `perf_profile` | Frame timing and memory counters, including `Tree::last_profile` |

## Example

```rust
use lurq::{
  app::{App, Tree, wgpu_render::WgpuRenderEngine, winit_shell::WinitWindow},
  components::{Column, Text},
};

fn main() {
  let app = App::new();
  let mut tree = Tree::new();
  tree.set_render_engine_factory(|| Box::new(WgpuRenderEngine::new()));

  tree.set_root(
    Column::new()
      .spacing(8.0)
      .child(Text::new("Hello from lurq")),
  );

  WinitWindow::new(app, tree)
    .with_title("lurq")
    .with_size(800, 600)
    .run();
}
```

## Documentation

- Guide: <https://ivanzaida.github.io/lurq/>
- API docs: <https://docs.rs/lurq>
- Local documentation build and maintenance: [docs/README.md](docs/README.md)

## Publishing

Crates are published by GitHub Actions when a pushed version is not already present on crates.io. Add a `CARGO_REGISTRY_TOKEN` repository secret with a crates.io API token, then bump the crate version and push to `master`.

The workflow publishes `lurq_macros` before `lurq` so the versioned macro dependency is available before the main crate is uploaded.

## License

MIT
