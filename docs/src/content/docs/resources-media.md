---
title: Resources And Media
description: Fonts, images, SVGs, background images, animated images, and async resources.
---

# Resources And Media

Media APIs are feature-gated. Enable only what the app needs.

| Feature | Public APIs |
| --- | --- |
| `raster` | `Image`, `ImageData`, `StreamingImage`, `Video`, and raw RGBA/native image transport. |
| `image` | Enables `raster` plus image decoding and image-backed style helpers. |
| `svg` | `Svg`, `SvgData`. |
| `resources` | `ResourceLoader`, resource-backed image/SVG constructors, resource roots. |
| `markdown` | Markdown parsing and `Markdown::mount`. |

## Fonts

`App` owns the glyph engine. Load fonts before running the window.

```rust
let mut app = lurq::app::App::new();

app.load_font_file(std::path::Path::new("assets/Inter.ttf"));
app.load_fonts_dir(std::path::Path::new("assets/fonts"));
app.register_font("ui", "Inter");
```

A family may also be a CSS generic family (`"serif"`, `"sans-serif"`, `"monospace"`, `"cursive"`, `"fantasy"`), drawn with the platform's font for it; see [Generic Families](../theme/#generic-families) and [`GenericFamily`](https://docs.rs/lurq/latest/lurq/layout/text_style/enum.GenericFamily.html) for the fonts of each platform. A bundled font replaces one by an alias of the keyword: `app.register_font("monospace", "JetBrains Mono")`.

### Hinted Fonts

Text is drawn unhinted: a letter falls at any fraction of a pixel, and looks the same wherever it falls. An icon font is the opposite case. Its glyphs are drawn on a pixel grid, and a stroke a pixel and a half wide spread over three pixels is a blur. Load such a font with `load_hinted_font`:

```rust
app.load_hinted_font(include_bytes!("../assets/icons.ttf").to_vec());
```

Its glyphs are then drawn grid-fitted: at the nearest whole pixel size and position, with the font's TrueType hints applied along both axes. The font has to carry hints for this to do anything, and to ask for native ClearType in its `prep` program for the hints along x to count. A glyph under a transform, or in a text that is not snapped to pixels, is drawn unhinted like any other.

Text uses `TextStyle`.

```rust
use lurq::{
  components::Text,
  layout::text_style::{FontStyle, FontWeight, TextStyle},
  node::color::Color,
};

Text::styled(
  "Hello",
  TextStyle {
    font_family: "ui".into(),
    font_size: 18.0,
    line_height: 1.25,
    weight: FontWeight::Bold,
    style: FontStyle::Normal,
    color: Color::from_hex("#e5e7eb"),
    ..TextStyle::default()
  },
)
```

## Images

With `image`, load from bytes, files, or raw RGBA.

```rust
use lurq::{components::Image, images::ImageData};

let image = ImageData::from_file("assets/photo.jpg").unwrap();
Image::new(image).size(240.0, 160.0)
```

Supported formats come from the `image` dependency configuration: PNG, JPEG, WebP, GIF, BMP, and TIFF. GIF and animated WebP preserve animation frames.

Raw RGBA only requires `raster` (also enabled by `canvas` and `image`):

```rust
let pixels = vec![255; 64 * 64 * 4];
let image = ImageData::from_rgba(pixels, 64, 64);
```

Streaming RGBA uses a stable image identity and uploads new pixels when the buffer version changes:

```rust
use lurq::{components::Image, images::StreamingImage};

let stream = StreamingImage::new_rgba(vec![0; 64 * 64 * 4], 64, 64);

stream.update_rgba(|pixels| {
  pixels[0] = 255;
  pixels[1] = 128;
  pixels[2] = 0;
  pixels[3] = 255;
});

Image::new(stream.image_data())
```

## Runtime Images

An image decoded at runtime from encoded bytes or a file, for attachments, thumbnails and screenshots that are not resources of the app:

```rust
use lurq::{components::{Image, Rect, Text}, images::ImageKey, node::BackgroundSize};

Image::from_bytes(attachment.bytes.clone())
  .cache_key(ImageKey::new(attachment.hash))
  .downscale_to(256, 256)
  .fit(BackgroundSize::Cover)
  .placeholder(Rect::new(96.0, 96.0).background("#374151"))
  .error_view(|error| Text::new(error.to_string()))
  .size(96.0, 96.0)
  .rounded(8.0)
```

`Image::from_bytes(bytes)` decodes PNG, JPEG, the first frame of a GIF or WebP, BMP and TIFF; `Image::from_file(path)` reads the file first; `Image::from_source(source, key)` takes either with a key. The decode runs on worker threads, off the UI thread: the node shows its `placeholder` (and its own background) until the pixels are there, then the image, and the `error_view` built from the `ImageDecodeError` for a broken file, instead of a panic. `downscale_to(width, height)` caps the decoded size, keeping the aspect ratio and never upscaling, so a thumbnail of a 4K screenshot does not keep its 30 MB of RGBA. `fit` places the pixels in the node's box: `Stretch` (the default, as `Image::new` always drew), `Contain` fits the whole image centered, `Cover` fills the box and crops the overflow; it works on every image node.

Decoded pixels are kept in the process-wide cache, `lurq::images::decoded_images()`, keyed by content and size cap: `from_bytes` hashes the bytes (`ImageKey::of_bytes`), `from_file` keys the path with the file's size and modification time (`ImageKey::of_path`), and an app that stores a hash of its own builds the node with `Image::from_source(ImageSource::Bytes(arc), ImageKey::new(hash))`, which neither hashes nor copies the bytes (`cache_key` replaces the key after `from_bytes` has hashed). A chat laid out again, or a `VirtualizedList` row built again as it scrolls back in, finds its image ready in the first pass and decodes nothing. The cache keeps at most `DecodedImages::budget()` bytes of RGBA (128 MiB by default, `set_budget`), dropping the least recently used ready images; a node that still shows one keeps it. `decoded_images().state(key, cap)` reads an entry without decoding, `insert` puts pixels the app decoded itself, and `decode_image(&source, cap)` decodes on the calling thread. A test waits with `lurq::images::wait_for_decoded_images(timeout)`; see [Testing](../testing/#pastes-file-drags-and-runtime-images).

## Clipboard

With `clipboard`, `lurq::clipboard` reads and writes the OS clipboard. Besides the text functions (`copy_to_clipboard`, `read_from_clipboard`, `clear_clipboard`), `read_clipboard()` reads everything it holds at once as a `ClipboardContent`: the text, the image a screenshot tool or a browser put there (Windows `CF_DIBV5`/`CF_DIB`, macOS `public.png`/`public.tiff`, as RGBA), and the files copied in Explorer or Finder (`CF_HDROP`, file URLs). A clipboard may hold text and an image at once; `kinds()` tells which.

```rust
use lurq::clipboard::{copy_image_to_clipboard, read_clipboard};

if let Some(content) = read_clipboard() {
  if let Some(image) = content.image_content() {
    let png: Vec<u8> = image.to_png()?; // to store it
  }
  for path in content.file_paths() {
    // a file copied in Explorer or Finder
  }
}
copy_image_to_clipboard(width, height, &rgba);
```

`ClipboardImage::from_rgba(width, height, rgba)` builds the image a test or an app pastes; `copy_files_to_clipboard(&paths)` puts file paths there. A paste into a text input delivers the same `ClipboardContent` through a `PasteEvent`; see [Paste](../styling-events/#paste).

## Resource Images

With `image` and `resources`, let the runtime load files relative to the resource root.

```rust
let mut app = lurq::app::App::new();
app.set_resource_root(std::path::PathBuf::from("assets"));

let avatar = lurq::components::Image::from_resource("avatar.png");
```

The first frame may render while the resource is pending. The loader caches successful results according to `ResourceConfig`.

Background images use the same feature pair:

```rust
use lurq::{components::Rect, node::BackgroundSize};

Rect::new(320.0, 180.0)
  .background_image("hero.jpg")
  .background_size(BackgroundSize::Cover)
```

Or use helper shortcuts:

```rust
Rect::new(320.0, 180.0).background_cover()
Rect::new(320.0, 180.0).background_contain()
```

Slider track and thumb styles can also use resource-backed background images:

```rust
use lurq::components::Slider;

Slider::new(value)
  .track(|style| style.background_image("ui/track.png").background_cover())
  .thumb(|style| style.background_image("ui/thumb.png").background_cover())
```

## SVG

With `svg`, construct SVGs from bytes or strings.

```rust
use lurq::{components::Svg, node::color::Color, svg::SvgData};

let icon = SvgData::from_str(r#"<svg viewBox="0 0 24 24"></svg>"#)
  .with_fill(Color::from_hex("#a855f7"));

Svg::new(icon).size(24.0, 24.0)
```

With `svg` and `resources`:

```rust
Svg::from_resource("icons/search.svg").size(18.0, 18.0)
```

`SvgData` supports fill, stroke, and opacity overrides.

## Resource Loader

`ResourceLoader::load_resource(path, config)` returns:

- `Pending` while async load is in progress,
- `Loaded(Arc<Vec<u8>>)` when bytes are ready,
- `Error(ResourceError)` for not found, network, OS, or unknown errors.

Local paths are resolved under `App::set_resource_root(...)` when set. Remote `http://` and `https://` URLs are loaded through `ureq`.

```rust
use std::sync::Arc;
use lurq::resources::{LoadResourceResult, ResourceConfig};

let loader = lurq::resources::ResourceLoader::new();
let path: Arc<str> = Arc::from("data.json");
match loader.load_resource(&path, Some(ResourceConfig { ttl: 60, retries: 1 })) {
  LoadResourceResult::Pending => {}
  LoadResourceResult::Loaded(bytes) => println!("{} bytes", bytes.len()),
  LoadResourceResult::Error(error) => println!("{error:?}"),
}
```

Most app code should prefer the higher-level resource-backed `Image` and `Svg` constructors.

## Markdown

Enable `markdown` to parse and render Markdown through a retained component:

```rust
use lurq::components::{Markdown, MarkdownProps};

Markdown::mount(
  ctx,
  MarkdownProps::new("# Notes\n\nRead the **release notes**.")
    .selectable(true)
    .on_link_click(|link| println!("{}", link.destination())),
)
```

Use `MarkdownProps::style(...)` for the base text style, `.theme(...)` for block styling, and `.width(...)` for a width constraint. Link callbacks decide how the application opens or routes destinations. The demo's `/markdown` route exercises the component.

By default the document fills its container's width, and so does every block in it. `.fit_content(true)` sizes it to its content instead, like CSS `width: fit-content; max-width: 100%`, so a chat bubble hugs its message:

```rust
Column::new()
  .align_items(Alignment::End) // own messages on the right
  .child(
    Column::new()
      .padding(8.0)
      .background(bubble_color)
      .child(Markdown::mount(ctx, MarkdownProps::new(message).fit_content(true))),
  )
```

- The document is as wide as its widest block (a short message as its longest line) and never wider than its container's width, where its text wraps. Paragraphs, headings, lists, quotes and footnotes all size to their content; a long list item wraps beside its marker.
- Every block is then stretched to the document's width, as CSS blocks are: a code block's box and a rule span the widest block.
- A code block does not wrap: it is as wide as its longest line up to the container's width, and a longer line scrolls inside its box, as when filling.
- A table still takes the container's full width: its columns share that width so that they line up across its rows.
- An explicit `.width(...)` still sets the document's width.
- In a `Column`, the container's width is the column's. A `Row` gives its children unbounded width, so a bubble in a row needs `.max_width(Dimension::Pct(100.0))` (or a `flex_shrink` factor) for its text to wrap at the row's width.

### Code blocks

A code block's lines are never wrapped. A line wider than the block's box scrolls sideways inside it: the box, its language label and the document keep their width, and a horizontal scrollbar shows over the box's bottom padding while the block overflows, in `ThemeMarkdown::code_block_scrollbar` (a thin light bar on the default dark box; `None` draws the app theme's scrollbar, always as an overlay). Shift+wheel and a trackpad's sideways swipe scroll the block; a plain or slightly slanted vertical wheel over it scrolls the page around it, as it always did, so a chat's follow-to-end is not caught by a block that only overflows sideways. The block's own scroll is kept across renders, keyed by the block's ordinal among the document's code blocks and its language: a streaming reply and paragraphs added around it keep its place (a code block inserted above it shifts the ordinals). A math block (`$$`) scrolls the same way. A block whose lines fit has no scrollbar and lays out as before. Selection and copy cross a scrolled block like any other block, the lines out of view included.

### Links

A paragraph is one text, links included: it wraps and selects as one. Each link in it is a Tab stop in document order; a click and Enter follow it (Space does not), the pointer shows a pointer cursor, and an MCP client sees it as a `link` named by its words. `on_link_click` runs inside the input's dispatch, so `lurq::app::events::is_synthetic_input()` tells a person's press from an MCP client's, and `MarkdownLink::index()` says which link it was.

`on_link_click` gets left clicks and Enter. To tell how a link was activated, use `.on_link_activate(...)`: it gets the link and the `MouseEvent` that activated it, for a left click, a middle click, or Enter on the focused link. The event's `button` is `Left` or `Middle` (`Left` for Enter), `from_keyboard` is set for Enter, and `shift`, `ctrl`, `alt` and `meta` are the modifiers held (`meta` is Cmd on macOS and the Windows key on Windows). Browser rules for a chat's links, for example:

```rust
use lurq::app::events::MouseButton;

MarkdownProps::new(reply).on_link_activate(move |link, event| {
  let shortcut = if cfg!(target_os = "macos") { event.meta } else { event.ctrl };
  if event.button == MouseButton::Middle || (shortcut && !event.shift) {
    tabs.open_in_background(link.destination());
  } else if shortcut && event.shift {
    tabs.open_and_switch(link.destination());
  } else {
    tabs.open_here(link.destination());
  }
})
```

- A middle click is a press and a release of the middle button on the same link, like a left click: a press released off the link activates nothing, and neither does a press on one link released on another more than a few pixels away. Closer than that, the link under the release activates, as with a left click. The press starts no text selection, no drag of what the link is in and no [middle-button autoscroll](../layout/#middle-button-autoscroll) of the container around it, and lurq has no middle-click paste for it to start.
- `on_link_activate` runs before `on_link_click`, which still gets the left clicks and Enter when both are set. With `on_link_activate` set, an in-app link no longer navigates the router by itself. Without it, a middle click on a link does nothing.
- It runs inside the input's dispatch, like `on_link_click`: an MCP client's `lurq_interact` click with `"button": "middle"` or `"modifiers": ["ctrl", "meta"]` reaches it with those, and `is_synthetic_input()` is `true`.

`.link_presentation(...)` decides how each link is shown. It is called once per link and render with the same `MarkdownLink` that `on_link_click` gets, so what a link shows always comes from the destination it opens:

```rust
use lurq::{
  app::theme::{MarkdownFocusRing, MarkdownInlineStyle, MarkdownLinkState, MarkdownTextStyle},
  components::{MarkdownLinkPresentation, MarkdownProps, MarkdownRun},
  layout::text_style::TextUnderline,
  markdown::isolate_bidi,
};

let words = MarkdownInlineStyle {
  text: MarkdownTextStyle {
    color: Some(accent),
    underline: Some(TextUnderline::new().color(muted).thickness(1.0)),
    ..Default::default()
  },
};
MarkdownProps::new(reply)
  .link_presentation(move |link| match kind_of(link.destination()) {
    // In the app: the link look, a tooltip naming the place.
    Kind::App(place) => MarkdownLinkPresentation::link()
      .style(words.clone())
      .hovered(MarkdownLinkState::new().color(accent_hover).background(hover_fill).underline_color(accent_hover))
      .focus_ring(Some(MarkdownFocusRing::new(2.0, focus).radius(3.0)))
      .tooltip(format!("{place} · opens in this tab")),
    // On the web: the host and an icon glyph after the words.
    Kind::Web(host) => MarkdownLinkPresentation::link()
      .style(words.clone())
      .trailing([MarkdownRun::new(format!("\u{a0}{host}\u{a0}\u{e04d}"), host_style.clone())])
      .hovered(MarkdownLinkState::new().trailing_color(secondary))
      // A destination is text the reply's author wrote: isolate it so its
      // directional controls cannot reorder the sentence around it.
      .tooltip(format!("Opens {} in your browser", isolate_bidi(link.destination()))),
    // Anything else is not followed: the words as text, then the address.
    Kind::Other => MarkdownLinkPresentation::text()
      // A run is stripped of directional controls and isolated by lurq.
      .trailing([MarkdownRun::new(format!(" ({})", link.destination()), address_style.clone())]),
  })
  .on_link_click(move |link| open(link.destination(), lurq::app::events::is_synthetic_input()))
```

- `link()` draws the words in the theme's `link` style, or the presentation's `style(...)` over the text around them. `text()` draws them as the surrounding text: nothing to press or focus.
- `trailing(...)` runs follow the words in the same paragraph, so they wrap with it; each run's style applies over the text around the link. After a link, the pointer takes them as part of the link. Like CSS `::after` content they are presentation: copying a selection, and inspection tools (MCP, DevTools), leave them out, and a selection highlight skips them.
- The words and trailing runs are drawn between invisible bidi marks, and a paragraph with a link drops every directional control (U+202A–U+202E, U+2066–U+2069) and directional mark (U+200E, U+200F, U+061C) from its text, so lurq's marks are the only ones left: a right-to-left override in the words or anywhere in the paragraph cannot draw a host after them backwards, not even past Unicode bidi's nesting limit. In a right-to-left paragraph (Hebrew or Arabic text) the paragraph still reads right to left around each link, and a link's trailing runs still follow its words, left to right. At any width, a laid-out line puts a link's own runs, or nothing, right after its words. A link stays on one line where it fits; its words are one line of text (a hard break in them is a space). A link whose words draw nothing (none, or only spaces, invisible characters and combining marks with no letter) is not presented: it is its words as text, with no runs and nothing to press. The marks are invisible, are not copied and are not part of the paragraph's `text_content`. Text you build yourself from a destination outside a run (a tooltip, say) needs the same care: `lurq::markdown::isolate_bidi` strips the controls and marks and wraps the text in an isolate; `strip_bidi_controls` only strips.
- A link's name (`MarkdownLink::text()`, and its MCP name) is its words as drawn: inline HTML as its text, an image as its alt text, without directional controls. An email autolink (`<team@example.com>`) reaches the app as `mailto:team@example.com`.
- `hovered(...)` and `focused(...)` (`MarkdownLinkState`) recolor the words, their underline and the trailing runs, and can fill behind the words, one rectangle per line with the link's `radius(...)`. Keyboard focus (focus-visible) also draws the `focus_ring(...)` around each line of the words, outside them on all sides and over the text. The document's own containers do not clip it; a code block, a table or the app's containers do.
- `tooltip(...)` shows under the link after the pointer rests on it for `ThemeMarkdown::link_tooltip_delay`, and at once while it has keyboard focus. It lets the pointer through.
- `pointer_only()` leaves a link to the pointer, for an app whose reading text is not part of its Tab order: the link is no Tab stop, a press on it leaves the focus where it was, so Enter never reaches it and it never shows its focused look, its ring or a tooltip from the keyboard. A click and a middle click follow it, and the pointer over it shows its hovered look and its tooltip, as on any link.
- Unset looks are the theme's: `ThemeMarkdown::link`, `link_hovered`, `link_focused`, `link_focus_ring`, `link_radius`, `link_tooltip` and `link_tooltip_box`.
- Underlines (`TextStyle::underline`, `MarkdownTextStyle::underline`: color, thickness and offset, each defaulting to the text color and the font's metrics) are drawn under every line a span covers, and lines through text (`strikethrough`: color and thickness) the same way. `~~struck~~` text has one by default, from `ThemeMarkdown::strikethrough`. A plain `Text` draws both too; see [layout](../layout/#underline-and-strikethrough).

`.list_view(...)` lets the app draw a list block itself. It is called for every list with a `MarkdownList` and returns the element to draw, or `None` to draw the list as usual. `MarkdownList::inlines` and `blocks` draw content as the document does, links included; decide from `items()` before drawing with them, since drawing numbers the links.
