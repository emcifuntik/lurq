# Design system implementation plan

Status: proposal for review. Nothing in this document is implemented.

Date: 2026-11-12. Repository baseline: lurq 0.60.0 plus Unreleased (`master` at `8770b598`).

Design source: [`pencil/lurq-design-system.pen`](../pencil/lurq-design-system.pen) — Foundations, 319 components for desktop, mobile and tablet, and Examples, in light and dark.

## 1. Goal

Ship the design system with lurq, driven entirely by the theme:

- **The Pencil design is the default.** Every lurq app has components that match the Pencil file in light and dark, with no feature to enable and no palette or state styling of its own.
- **The theme controls every component.** An app adjusts the global theme — tokens, per-component styling, and where needed a component's whole look — and every instance follows. Per-instance overrides exist but are the exception.
- **A theme can go as far as a native look.** A developer targeting iOS can make the same components look like the platform's own: different sizes and shapes, and different anatomy where the platform's control is built differently.

Non-goals for this plan:

- Anything the operating system draws: file, print and permission dialogs, native menus, the macOS menu bar, on-screen keyboards, home-screen widgets.
- A screen-reader bridge. It is a separate track; section 9 only keeps the door open.
- Android and web targets, and iPad support. Tablet components wait for iPad.
- Shipping finished native themes for every platform. The plan makes them possible and proves it with one iOS skin (section 7, step 2); complete platform themes are follow-up work.

## 2. Where the code stands

Findings from a read of `crates/lurq` (three inventories: components, theme, rendering and platform). File references are to `crates/lurq/src`.

**Solid base.** Gradients, multiple and inset box shadows, element and backdrop blur, rounded clips, popup anchoring with flip and clamp, a Tab-trapping modal layer, virtualized lists, flex wrap, transitions and keyframes, 35 cursor icons, breakpoints with `Responsive<T>`, and a reactive theme with role enums and `Extra` names.

**Three gaps sit between that base and the design.**

1. **State styles are narrow.** `Style` (`node/style.rs:20`) holds background, radius, borders, shadow, cursor, frame, padding and flex. A hover, pressed or focus style cannot change text color, opacity, gradient or transform, and there are only five states: hovered, active, focused, focus-visible, drag-over. There is no node-level disabled state; `.focusable(false)` blocks focus but clicks still run.
2. **The theme is one light palette.** 22 roles on a blue accent (`app/theme/palette.rs`), no dark palette, no scheme switch, no system appearance (winit's `ThemeChanged` is only named in a debug match, `app/winit_shell.rs:1953`). No tokens for motion, layers, opacity, control size, density, icon size, focus ring or materials. Text defaults to pure black rather than a palette role, and text color is not inherited. There is one app-wide theme and no public way to override it for a subtree.
3. **Few styled components, and no common way to theme them.** `Select`, the `form` compound controls and `Markdown` read the theme, each through its own hand-written struct (`SelectStyle`, `FormTheme`, `ThemeMarkdown`). `Button`, `TextInput`, `Link`, `Popup` and `Modal` are unstyled primitives. `Sheet`, `TabBar`, `NavigationBar` and the window chrome carry hard-coded colors with a `dark()` constructor each. Radio, switch, tooltip, menu, toast, progress, tabs for desktop, table, tree, calendar and about thirty more do not exist.

## 3. Decisions

Agreed on 2026-11-12. The phases below build on them.

| # | Decision | Agreed | Why |
| --- | --- | --- | --- |
| D1 | Where components live | A `lurq::ui` module in the `lurq` crate, always compiled — no feature flag | They are what lurq is by default, and they need crate-private pieces (rich text, overlay layers, interaction state). Components that build on an optional feature (`markdown`, `canvas`, `router`) stay behind that feature. |
| D2 | Palette role names | The design's names become the roles. Old roles that are no longer needed are deprecated, and a migration guide maps each one to its replacement | Code and the Pencil file use one vocabulary. lurq is pre-1.0 and has shipped migrations before (`docs/…/migration-v0-13.md`). |
| D3 | The `accent` collision | The design's `accent` and `accent-foreground` are renamed `highlight` and `highlight-foreground`, in the Pencil file and in code | In lurq `Accent` is the brand color; in the design `accent` is a subtle hover fill. With the rename, deprecated `Accent` can point at `Primary` without silently changing meaning. |
| D4 | Fonts | System fonts by default, nothing bundled. Typography — family included — is settable at every level of the theme: globally, per component, per variant, per size, per state, per part, and per component nested inside another. Two buttons in one message box can use two different fonts | Font choice is part of a component's look, not a global constant. See 4.2 and 4.3. |
| D5 | Icons | lurq draws its own glyphs for the icons its components need, and does not depend on Lucide. An `Icon` component takes a name; the theme maps names to glyphs, so an app or a theme can plug in any set | No third-party asset to license or track. A native iOS theme maps the same names to SF Symbols. See the icon notes below. |
| D6 | How components are themed | Every component is described by a **recipe in the theme**: styles for each of its parts, per variant, size and state. A theme may also replace a component's **renderer**. See section 4 | This is the requirement in section 1. It replaces one-off structs like `FormTheme` with one model every component shares. |
| D7 | The `form` controls | `FormTextInput`, `FormCheckboxInput`, `FormSliderInput` and the two buttons are rebuilt on the new components; `FormTheme` is deprecated in favor of recipes | One implementation of a button, and one way to theme it. |

**Icon notes (D5).**

- **The set is small.** Only what components draw themselves: chevrons and arrows, check, minus, plus, close, search, the status marks (info, success, warning, error), spinner, ellipsis, grip, calendar, clock, eye, copy, upload, external link, window controls and a few more — on the order of fifty. Icons an app shows in its own content are the app's to bring.
- **They are drawn in the Pencil file**: 152 glyphs on a 24-unit grid with a 2-unit stroke, one `Icon/<name>` component each, on the Icons pages next to a table from Lucide names to lurq names. Every icon in the file is an instance of one of them; no Lucide glyph is left. A few need a second pass: settings, airplane, quote, strikethrough, bug, the three hands, megaphone, replace and replace-all.
- **They reach the screen as a hinted icon font.** The sources are in the repository: one SVG of filled outlines per glyph in `design/icons/svg`, exported from the Pencil file. `scripts/build-icon-font.py` builds `crates/lurq/assets/lurq-icons.ttf` from them and writes the TrueType hints itself; with `--from-board` it first exports the glyphs from the Pencil file, through the running Pencil app, so one command goes from the board to the font. `design/icons/codepoints.json` gives each name a codepoint from U+E000 up; a name keeps its codepoint for good, and a new glyph takes the next free one. A font needs no optional feature because text rendering is always present.
- **What the hints do.** A straight stroke is one pixel wide up to 17 px. Above that it keeps the width it was drawn with, with the edge that faces out of the glyph on the pixel grid, so it weighs what the diagonal next to it does. Strokes are placed from the middle of the glyph outward and keep a whole pixel of white between them; where the white is too narrow for that, a stroke is drawn in whole pixels. Diagonals and free stroke ends are left alone. None of it shows until the glyph engine renders this font hinted (E9 in section 5).
- **Lucide today.** lurq already ships `assets/lucide.ttf` in the published crate, for DevTools, with no license notice in the repository. Lucide is ISC and its Feather-derived icons are MIT; both allow this but require their notices to accompany copies. Either DevTools moves to the new glyphs and the file is removed, or a notice file is added until it does. A wider license audit is under way separately.

## 4. Theming model

Three layers. Each is set on the theme; each has the Pencil design as its default.

```
Tokens      palette, radius, spacing, type, motion…   "what blue is, how round is md"
   ↓ referenced by
Recipes     per component: parts × variant × size × state   "a primary button is filled, 36 tall"
   ↓ consumed by
Renderers   per component: builds the node tree from parts   "a switch is a pill with a thumb"
```

Most apps touch only tokens. A brand refresh edits recipes. A native platform look replaces renderers for the controls whose anatomy differs.

### 4.1 Tokens

The global scales and roles of section 6. Changing a token changes every recipe that refers to it, so "make everything rounder" or "use our blue" is one line.

### 4.2 Recipes

A component is a set of named **parts** — a button has `root`, `label`, `icon` and `spinner`; a switch has `track` and `thumb`. A recipe gives each part a style for every combination the component supports:

- **Base**: applies always.
- **Variant**: `Primary`, `Secondary`, `Outline`, `Ghost`, `Destructive`… Variants are keyed by name, so an app can add its own.
- **Size**: `Sm`, `Md`, `Lg`, or a density.
- **State**: `hovered`, `pressed`, `focus_visible`, `disabled`, and component states such as `checked`, `selected`, `invalid`, `loading`, `open`.

All of them use one shared type, a **part style**: an all-optional patch of background, foreground, border, radius, shadow, outline, opacity, padding, gap, height, width, typography and transition. Values are token roles or concrete values. Because every component uses the same patch type, learning to theme one component is learning to theme all of them.

Typography in a part style is the whole text style, each field optional: font family, size, weight, style, line height, letter spacing and font features. A typography role sets all of them at once; any single field can then be overridden. So a font can differ per component, per variant, per size, per state and per part — a primary button in one family and an outline button in another is two lines in the theme.

Resolution order, later wins: base → variant → size → state → the enclosing component's patch (4.3) → the instance's own override.

```rust
// Illustrative — the exact API is settled in step 2 of section 7.
let ui = app.theme().ui();

// Every primary button becomes a pill.
ui.button().variant(ButtonVariant::Primary).part(ButtonPart::Root, |s| s.radius(RadiusSize::Full));

// Medium controls are 44 tall everywhere.
ui.button().size(ControlSize::Md).part(ButtonPart::Root, |s| s.height(44.0));

// An app-defined variant.
ui.button().variant("brand").part(ButtonPart::Root, |s| s.background("brand").foreground("on-brand"));
```

Pointer and focus states in a recipe compile to node state styles (section 5, E2–E4), so a hover never re-renders the component. Component states are resolved when the component renders, which already happens when they change.

### 4.3 Components inside components

A message box contains two buttons; a date picker contains a field, a calendar and buttons; a table toolbar contains a search field and a menu button. A composite names each inner component as a **slot**, and its recipe can patch the recipe of whatever sits in that slot.

```rust
// Illustrative. In every message box, the confirm button is set in a serif face
// and the cancel button in the theme's mono family.
let message_box = ui.message_box();
message_box.slot(MessageBoxSlot::Confirm, |button| {
  button.part(ButtonPart::Label, |s| s.font_family("Georgia").weight(FontWeight::Bold))
});
message_box.slot(MessageBoxSlot::Cancel, |button| {
  button.part(ButtonPart::Label, |s| s.font_family(FontFamily::Mono))
});
```

- A slot patch applies only to that component in that place. Buttons elsewhere keep the button recipe.
- Slots nest: a preferences window can reach the button inside the message box it opens.
- A slot also fixes defaults the composite chooses, such as "the confirm slot is a primary button", and the theme can change that choice.
- The same mechanism serves an app's own composites, so app-level components are themable the same way.

### 4.4 Renderers

A recipe can restyle a control but cannot change what it is made of. An iOS switch, a segmented control, a navigation bar or an alert with hairline-separated buttons differs in anatomy, not only in color.

So each component is split in two:

- **Behavior** — state, keyboard handling, focus, semantics, events. Owned by lurq, not replaceable.
- **Renderer** — a function from the component's resolved state and recipe to a node tree. The default renderer draws the Pencil design. A theme may register another.

```rust
// Illustrative.
ui.switch().renderer(|switch, recipe, ctx| {
  // Build an iOS-shaped track and thumb from `switch.checked()`, `switch.interaction()` and `recipe`.
});
```

A replaced renderer still gets the recipe, so tokens and recipes keep working inside a native skin.

### 4.5 Presentation

Some differences are behavioral conventions rather than looks: whether a select opens a menu or a bottom sheet, the order of confirm and cancel in a dialog, whether a tooltip exists on touch. The theme carries these as per-component **presentation** options with platform-appropriate defaults, for example `SelectPresentation::{Menu, Sheet}`.

### 4.6 Whole themes, platforms and scopes

- **Theme definitions.** A definition bundles tokens for light and dark, recipes, renderers and presentation. `Theme::apply(definition)` installs one atomically. `Theme::neutral()` — the Pencil design — is the default.
- **Derived themes.** A definition can extend another and override part of it, so an app's theme is "neutral, plus these changes" rather than a copy.
- **Per platform.** An app picks a definition per target, for example the neutral theme on desktop and a native-looking one on iOS. lurq does not switch looks by platform on its own.
- **Scoped themes.** A public way to override the theme for a subtree — a dark sidebar in a light window, or a compact-density inspector. Today `Ctx::with_theme` is crate-private.

### 4.7 What this replaces

`FormTheme`, `SelectStyle`'s themed defaults, and the hard-coded `new()`/`dark()` pairs on `Sheet`, `TabBar`, `NavigationBar` and the window controls all become recipes. `ThemeMarkdown` moves from concrete colors to roles.

## 5. Phase 0 — Style engine

Small, self-contained changes to nodes. Each lands with tests and a docs section, and none depends on the others. Recipes need E1–E4 to express their states without re-rendering.

All of them are built. Section 13 says where each stands, what was decided while building it, and what still has to be checked on platforms it was not built on.

| Item | Change | Notes |
| --- | --- | --- |
| E1 Foreground color | An inherited `foreground` on nodes: `.foreground(color)`. `Text` and `Icon` without an explicit color use the nearest ancestor's foreground, then the theme's `Foreground` role | Fixes the pure-black default and lets one part recolor its label and icon together. |
| E2 State styles | Add `foreground` and `opacity` to `Style` | Paint-only, so no relayout. A ghost button's label or a menu item's text can change on hover without a re-render. |
| E3 Disabled | `.disabled(bool)` on nodes, a `disabled` bit in `InteractionState`, and a `.disabled_style(...)` slot. A disabled node takes no pointer or keyboard input, leaves the tab order, and passes the state to its subtree | Today only `SelectOption` and form `Control` know the word. |
| E4 Outline | `.outline(width, color, offset)`, also settable in state styles. Painted outside the border, takes no layout space | The focus ring in the design sits two pixels off the control. A border cannot do that without moving things. Containers clip by default, so the ring needs the same care as shadows: document it, and consider painting outlines in the overlay pass. |
| E5 Text decoration | Draw `underline` on plain `Text`; add `strikethrough` to `TextStyle` and to `MarkdownTextStyle`; add `max_lines` with an ellipsis on the last line | Links, struck task items in Markdown, and two-line card descriptions all need these. |
| E6 Transitions | Make `foreground`, `box_shadow` and `outline` animatable | Opacity, background, border and transform already are. |
| E7 Presence | An enter and exit animation hook for layers: content stays mounted until its exit animation ends | Popovers, dialogs and toasts fade and slide in the design. Today only `Sheet` and `StackOutlet` animate, each with its own code. |
| E8 Reduced motion | Read the system setting where the platform has one; expose `ctx.reduced_motion()`; durations resolve to zero or a fade | Docs currently state lurq does not read it. |
| E9 Hinted icons | The glyph engine renders the icon font hinted and snapped to whole pixels: whole sizes, whole positions. Today `render_glyph_image` forces hinting off for all fonts, which is right for text placed at fractions of a pixel and stays so | The font opts into native ClearType in `prep`; without that a smooth rasterizer ignores hints along x. swash's own scaler ignores them either way, because it pins `preserve_linear_metrics`. So the icon path hints the outline through skrifa (smooth mode without linear metrics) and hands it to zeno, the rasterizer swash uses. |

Dashed borders, a single-side border that follows the corner radius, grid layout and sticky positioning were deferred until a component needed them, and were then built with the rest: section 13 has them. No z-index — overlay layers already cover it.

## 6. Phase 1 — Theme

**Palette roles.** Replace the palette with the design's set, with light and dark values:

- Surfaces and content, in pairs: `Background`/`Foreground`, `Card`, `Popover`, `Primary`, `Secondary`, `Muted`, `Highlight`, `Destructive`, each with its `…Foreground`.
- States: `PrimaryHover`, `SecondaryHover`, `DestructiveHover`, `StateHover`, `StatePressed`.
- Status: `Success`, `Warning`, `Info`, `Destructive`, each with a `…Muted`.
- Lines: `Border`, `Input`, `Ring`.
- Extras: `Canvas`, `Scrim`, `Selection`, `Chart1`…`Chart5`, `GlassThin`, `GlassRegular`, `GlassThick`.

Deprecated roles and their replacements, for the migration guide (D2): `Accent` → `Primary`, `SurfaceBase` → `Background`, `SurfacePanel` → `Canvas`, `SurfaceRaised` → `Card`, `SurfaceInput` → `Background`, `BorderStrong` → `Input`, `BorderFocus` → `Ring`, `TextPrimary` → `Foreground`, `TextSecondary` and `TextMuted` → `MutedForeground`, `TextInverse` → `PrimaryForeground`, `Danger` → `Destructive`, `AccentHover` → `PrimaryHover`. `AccentMuted` has no exact match; it maps to `Highlight`.

**Schemes.**

- `ColorScheme { Light, Dark }` and `Appearance { Light, Dark, System }`.
- A theme definition holds both palettes; `theme.set_appearance(...)` switches in one version bump.
- System appearance on Windows, macOS and Linux through winit, and on iOS through the trait collection; `System` follows changes live.
- Changing the default colors is a visible break for apps that never set a palette. `Theme::legacy()` restores the 0.60 look; the change goes in the migration guide.

**New token tables**, each a role enum with an `Extra` variant, following the existing pattern:

| Table | Roles | Values from the design |
| --- | --- | --- |
| Radius | add `Xl`, `Full` | 4, 6, 8, 12, pill (today 3, 5, 6) |
| Control size | `Sm`, `Md`, `Lg` | 32, 36, 40 |
| Icon size | `Xs` … `Xl` | 12, 14, 16, 20, 24 |
| Duration | `Fast`, `Base`, `Slow`, `Slower` | 100, 160, 240, 400 ms |
| Easing | `Standard`, `Enter`, `Exit` | the three curves in Foundations |
| Opacity | `Disabled`, `Muted` | 0.5, 0.7 |
| Blur | `Thin`, `Regular`, `Thick` | 8, 20, 40 |
| Focus ring | width, offset | 2, 2 |
| Density | `Compact`, `Default`, `Comfortable`, `Touch` | 28, 36, 44, 48 |

**Typography.** Give each role its own line height; point `Body` at the palette foreground through E1; add the heading scale used by Markdown. Font families become two theme values, `sans` and `mono`, defaulting to the system's generic families (D4); every typography role refers to one of them, so substituting a font is one setting.

**Pencil file.** Rename the `accent` and `accent-foreground` variables to `highlight` and `highlight-foreground` (D3), so the parity test in section 8 compares like with like.

**Theme machinery** from section 4:

- The part-style patch type and recipe storage, with resolved recipes cached per theme version so a render does not re-merge them.
- Renderer registration and presentation options.
- Theme definitions, `Theme::apply`, derived definitions.
- Public scoped themes.

## 7. Phases 2–7 — Components

Every component ships with the same six things: behavior separate from its default renderer; a recipe with the Pencil values; `variant` and `size` where the design has them and every state the design shows; keyboard behavior; tests under `crates/lurq/tests/ui/`; a docs page and a gallery entry (section 8).

**Step 2 — Prove the model before building on it.** Four components chosen to stress it differently: `Button` (variants, sizes and pointer states), `Switch` (anatomy that differs by platform), `Tabs` (several parts and a selected state) and `MessageBox` (a composite with two buttons in slots). For those four:

1. Build the default renderers and recipes from the Pencil design.
2. Write an iOS-looking skin for them in `crates/mobile_demo` using only public theme API: tokens, recipes, and a renderer for `Switch`.
3. Restyle them for a made-up brand using tokens and recipes alone.
4. From the theme alone, give the message box's two buttons different fonts, and give primary and outline buttons different fonts everywhere else.

If any of these needs something the theme cannot express, fix the model here. The recipe, slot and renderer API is frozen only after this step. Nothing else in phases 2–7 starts before it.

**Phase 2 — Core controls.** The pieces everything else is built from.

The glyph set from D5, then `Icon`, `Button` (five variants; sizes; icon, icon-only, loading, disabled), `Link`, `Input` and `Field` (sizes; error, valid, read-only, disabled; leading icon, clear), `Textarea`, `Checkbox` (with indeterminate), `Radio` and `RadioGroup`, `Switch`, `Slider` (range, vertical, marks), `Select` rebuilt on a recipe (sizes, invalid, disabled, groups), `Badge`, `Tag`, `Chip`, `Avatar`, `Separator`, `Kbd`, `Spinner`, `Progress`, `Skeleton`, `Card`, `Alert`.

**Phase 3 — Overlays.** Built on `Popup`, `Modal` and E7.

`Tooltip`, `Popover`, `Menu` (dropdown and context; checkbox and radio items; submenus; shortcuts), `Dialog` and `AlertDialog` (scrim, sizes, scrolling body), `Sheet` on a recipe, `Toast` with a stack and positions, `HoverCard`, `Combobox`, `CommandPalette`.

**Phase 4 — Navigation and structure.**

`Tabs` (segmented, line, pill, vertical, overflow; optional counts), `Accordion`, `Collapsible`, `Breadcrumb`, `Pagination`, `Steps`, `Sidebar`, `Menubar`, `Toolbar`, `Splitter`, `TreeView`, `Navbar`.

**Phase 5 — Data and pickers.**

`Table` and `DataGrid` (sort, selection, empty, loading, toolbar), `ListItem`, `EmptyState`, `DataList`, `Timeline`, `Calendar`, `DatePicker`, `DateRangePicker`, `TimeField`, `ColorPicker`, `FileUpload`, `NumberInput`, `InputOTP`, `TokenInput` on a recipe, `Markdown` on roles. Charts last, on `Canvas`: bar, line, area, pie, with a shared legend and tooltip.

**Phase 6 — Desktop shell.**

Window chrome on recipes, with Windows, macOS, GNOME and KDE as four renderers of one component, and tabs in the title bar; `DocumentTabs`, `StatusBar`, `PropertyGrid`, `FindBar`, `ShortcutRecorder`, `MessageBox`, `DockLayout`. `DockLayout` is the largest single item in the plan and may deserve its own proposal.

**Phase 7 — Mobile.**

Touch sizes through the `Touch` density; `TabBar`, `NavigationBar` and `Sheet` on recipes, their current iOS look kept as a renderer rather than hard-coded colors; list rows with edit and select modes; swipe actions; pull to refresh; bottom sheet pickers; toast, banner and in-app notification. Swipe actions and pull to refresh are listed as unsupported in `docs/ios-gaps.md` and need gesture work first.

Not planned as components: the Examples page (whole windows and screens) stays as demos; rarely used items from the design — signature pad, angle slider, QR code, watermark, transfer, cascader — wait for a request.

## 8. Verification

- **Gallery.** A `gallery` binary in `crates/demo` with one page per component group, mirroring the Pencil sheets, with a switch for light, dark and system, and a switch between theme definitions. It is the place to compare against the design and the first consumer of every API.
- **Screenshot tests.** The `screenshot` feature renders each gallery page in both schemes; images are compared against stored references so a token or recipe change that shifts a component shows up in review.
- **Behavior tests.** Per component under `crates/lurq/tests/ui/`: states, keyboard, disabled, and focus order. They run against the behavior half, so they hold under any renderer.
- **Theme tests.** A second, deliberately different theme definition runs through the gallery in CI. A component that ignores its recipe — a hard-coded color, a fixed size — shows up as unchanged.
- **Design parity.** Token values are exported from the Pencil file's variables and checked against the neutral theme's defaults in a test, so the two cannot drift.

## 9. Accessibility hook

No bridge in this plan, but every component declares a semantic role, name and state on its node from day one, in the behavior half so a replaced renderer cannot drop them. A later AccessKit integration then has the data it needs without touching each component again.

## 10. Order and size

| Step | Depends on | Rough size |
| --- | --- | --- |
| Phase 0, E1–E4 | — | small each; E1 touches text layout and is the largest |
| Phase 0, E5–E9 | — | small to medium; E7 is the largest |
| Phase 1, tokens and schemes | E1 | medium; the palette migration touches every themed component and the docs |
| Phase 1, theme machinery | E1–E4 | medium to large; it is new architecture |
| Step 2, proof on four components | Phase 1 | medium; expect to revise the machinery here |
| Glyph set (D5) | E9 to look right | drawn in Pencil (152 glyphs), with sources, generator and hinted font in the repository; left: a second pass on a few |
| Phase 2 | Step 2, glyph set | large; about 25 components |
| Phase 3 | Phase 2, E7 | large |
| Phase 4 | Phase 2 | medium |
| Phase 5 | Phases 2 and 3 | large; table and calendar dominate |
| Phase 6 | Phases 2–4 | large; dock layout dominates |
| Phase 7 | Phases 2 and 3 | medium, plus gesture work |

Phase 0 and the tokens half of Phase 1 can be reviewed and released on their own: they improve lurq for every app, including ones that never use `lurq::ui`. A first useful release of the components is Phase 2 plus `Tooltip`, `Menu`, `Dialog` and `Toast` from Phase 3.

## 11. Risks

- **The theming API is the hardest part to change later.** Every component and every app theme depends on it. Step 2 exists to find its flaws on four components instead of thirty.
- **Recipe surface area.** A patch type with fifteen optional fields, times parts, variants, sizes, states and nested slots, is a lot to document and keep stable. Keeping one shared patch type, and adding fields only when a component needs them, holds it down.
- **No feature flag.** Every app compiles every component. Unused ones do not reach the binary, but build time grows with the set. If it becomes a problem, the large groups (data grid, charts, dock layout) can move behind features later without changing the rest.
- **The icon font's hints are our own.** The 152 glyphs are in the repository as sources and as a font, and E9 draws them hinted. The hints are written by `scripts/build-icon-font.py`, not by a tool with a history, so every glyph added should be looked at on a comparison sheet at 12 to 24 px. A few glyphs still need redrawing. Nothing draws the font yet outside `examples/icon_hinting.rs`: DevTools is still on Lucide (question 8) and `Icon` is Phase 2.
- **Renderers and behavior drifting apart.** A custom renderer that forgets to wire a part breaks hit-testing or focus. The behavior half must own events and semantics, and hand the renderer parts that already carry them.
- **Resolution cost.** Merging base, variant, size and state on every render would be slow. Resolved recipes are cached per theme version and per combination.
- **Breaking changes.** New default colors, renamed palette roles, a deprecated `FormTheme`. Mitigation: deprecated roles keep compiling and resolve to their replacements, a migration guide, and `Theme::legacy()`.
- **Inherited foreground and caching.** Text layout caches key on style. Foreground is paint-only and must stay out of shaping keys, or a hover will reshape text.
- **Clipping.** Containers clip by default, so focus rings, shadows and tooltips near an edge are cut. E4 has to settle where outlines paint.
- **Scope.** The design has 319 components. Phases 2–4 cover what most apps use; phases 5–7 should each be re-planned when reached rather than committed to now.
- **Design drift.** The Pencil file will keep changing. The parity test in section 8 turns silent drift into a failing check.

## 12. Open questions

1. How long deprecated palette roles stay before removal: one release, or until 1.0?
2. Recipes as builder calls on the theme (as sketched in section 4) or as plain data structs an app constructs and installs? Data is easier to derive and diff; builders read better. Step 2 should try both.
3. How much may a renderer change: only the node tree, or also which parts exist? Fixed parts keep recipes portable across renderers; free parts allow more native fidelity.
4. Does lurq ship a native-looking iOS theme itself, after the proof in step 2, or leave it to apps and the community?
5. Should the new components be configured through builders like the existing `components`, or through props structs like the `form` compound controls?
6. Does the default theme change in the same release that adds the components, or one release later?
7. Settled: the glyphs reach the screen as a hinted icon font, built by `scripts/build-icon-font.py` from `design/icons/svg` (icon notes, D5). Vector paths would have needed the `svg` or `canvas` machinery, or a new path primitive.
8. Does DevTools move to the new glyphs at once, so `lucide.ttf` can be removed, or keep Lucide with a notice file for now?
9. Is a `gallery` binary acceptable in `crates/demo`, or should it be its own crate?
10. Which platforms must screenshot tests run on in CI?

## 13. Phase 0: status and handover

Written when all of Phase 0 was built, on branch `feat/design-system`. It is for whoever picks the work up next, and for checking it on the platforms it was not built on.

### 13.1 Where each item stands

| Item | State | Commits |
| --- | --- | --- |
| E1 Foreground | built | `e6c8a343` |
| E2 State styles | built | `5d33f980` |
| E3 Disabled | built | `697fcfea`, then `7e3789bf` (scrolling), `b3db6430` (state for every element) |
| E4 Outline | built | `5d33f980`, then `d4a7bd32` (clipping) |
| E5 Text decoration | built | `1298fcad`, then `058784a4` (node size), `9b2c264e` (line limit as painted) |
| E6 Transitions | built | `29f95d08` |
| E7 Presence | built | `bee2b8d8` |
| E8 Reduced motion | built; macOS and iOS not built or run (13.3) | `9986db86` |
| E9 Hinted icons | built in its own session and merged | `19c57f9b`, `3d54ae3a`, merge `902c03e0` |
| Theme `Foreground` role | built | `dd5235bc` |
| Borders: one side follows its corners; dashes | built; Metal not looked at (13.3) | `96c04710` |
| Sticky positioning | built | `244c6b20` |
| Grid | built | `1122c430` |
| Presence for a container's children | built | `7f96b5a0` |

Everything was built and run on Windows only. The check is `scripts/check.ps1`: the main test suite, the suite with `devtools,mcp,perf_profile,counting_allocator`, the release stack-depth guard, and a build of the tests with the features of both suites together, which is where the size budgets of `size_budget.rs` are checked for a build that has everything on. All four passed at `2c40c46c`, and again, with the readback tests, at `a633412c`, where the branch was merged with its remote, which by then had master in it; start there when something fails later. The tests that read frames back from a GPU are ignored by default and run by hand: `cargo test -p lurq --features wgpu,dx12,screenshot --lib readback -- --ignored --test-threads=1`, on Windows only, since they open a hidden Win32 window. Clippy, over every target with every feature, is clean in the lines this branch wrote (`2c40c46c`) and reports 215 findings in the rest of the crate, which were there before and were left. `cargo fmt --check` reports differences in five files this work did not write (`winit_shell/frame_source.rs`, `winit_shell/touch.rs`, `components/sheet.rs`, `components/stack_outlet.rs`, `tests/components/sheet.rs`); they were there before.

`586e84c2` is not part of the plan: one line in a test that kept the second suite from compiling on the branch before this work began. Two more commits fix what later work found in earlier work: `c920668b` (a component that rendered again wrote into a layer on its way out) and `8982356f` (a node was over its size budget with every feature on, which no suite turned on together).

### 13.2 What was decided while building

Where the code differs from the table in section 5, the code is what was decided. User-facing behavior is in `docs/src/content/docs/styling-events.md`, `layout.md`, `animation-transforms.md` and `modals.md`, and in the changelog.

- **E1.** With no foreground above it a text keeps its typography style's color, so a tree that sets none paints as before. The theme's `Foreground` role (`PaletteColor::Foreground`) is the color of a text whose style keeps the default text color; it is black by default, so nothing changes until a theme sets it, and Phase 1 gives it the design's value. A `TextInput` and Markdown draw in their own styles and do not read the foreground, as an `<input>` does not inherit `color`. An overlay's content is painted outside the element that declares it and starts from no foreground.
- **E2.** State styles carry `foreground`, `opacity` and `outline`. Where several set a value the last applied wins: disabled, then drag-over, active, hovered, focus-visible, focused.
- **E3.** A press or click on a disabled element reaches nothing, the elements it is in included, as a disabled control on the web: a clickable card is not clicked through its disabled button. A pointer moving over it still reaches those elements. Scrolling is not input to a control and works as if nothing were disabled, inside the element too. Hit testing folds a disabled element's subtree into one hit; that is the mechanism to know before touching input code. The disabled style is paint-only. The state is written in the walk that assigns ids to a built tree, so it costs no walk of its own.
- **E4.** `Outline` is a type (`Outline::new(width, color).offset(px)`), not three arguments. Containers clip their content by default, so an outline takes the clip of its element let out by its own reach; scroll viewports and the window, which the paint walk tracks apart, cut it where they cut the element. An outline is painted over the elements painted after its own, at the end of the scope its element is in: a scroll container (under its border and bars), an element with an opacity or a blur (inside its layer), the siblings that scroll under an element that sticks, the page and each layer over it, or the tree. The paint walk keeps the outlines it meets (`pending_outlines`) and each scope paints those kept since it began.
- **E5.** `TextStyle` and `MarkdownTextStyle` gained a public field, `strikethrough`: a source break for code that names every field, accepted and listed in the changelog. Markdown now draws `~~struck~~` text struck. `Text::max_lines` holds in the layout that paints the text as well as the one that sizes it.
- **E6.** The outline animates as one value, and no outline is a value too, so a ring grows in and fades out. Transitions now read colors, widths and shadows given as theme roles; before, a color given as a role changed at once whatever its transition said. A role whose value changes with the theme runs the transition too. A list of more than four shadows changes at once.
- **E7.** There is no `Presence` type. An element enters with the animation it plays from its mount, which a layer's content already did, and `.exit_animation(animation)` on any element names what it leaves with. A closed layer with an exit animation anywhere in it stays in the overlay host until the longest ends, so a dialog's backdrop and panel leave each in its own way. What stays is the layer's nodes as they were when it closed: the app does not render them again. The layer is only painted: no pointer, focus or Tab, no modal trap, no lookup by id or ref (`node/presence.rs`, `app/runtime/leaving_layers.rs`). A layer opened again while it leaves is a new layer over the old one, not the old one turned around. A popup hidden with its anchor does not leave.
- **E7, layer identity.** Building it showed that open layers were matched between passes by their place in the host: when the first of two popups closed, the second took over its state. A layer is now the layer the same declaring node opened last pass, then the layer of its key, and by place only when the declaring node is new. A layer with an animation in it is never matched to another declaration's, which is what lets one popup leave while the next enters.
- **Presence for children.** A container asks for it with `.presence()`, and only a child with a key or an id and an exit animation of its own is kept: children without a key are matched by their place, where a removed one and a changed one look the same, and a list that mounts only the rows in view removes rows as it scrolls. The kept child stays in flow; an exit animation of its height lets its siblings close the gap. It is carried from the old tree to the new one in the walk that carries node ids (`Node::preserve_ids_from`), and the walk that carries state matches the other children as if it were not there. A popup that keeps one is measured and placed again.
- **The theme's foreground.** A style that keeps the color of `TextStyle::default()` names no color, and is drawn in `PaletteColor::Foreground`: a `Text`, a `TextInput` with its caret, a Markdown document. A typography style with a color of its own keeps it. The role is black by default, so nothing changes until a theme sets it; Phase 1 gives it the design's value. Telling "no color" by the default color is a convention, not a type: a text that must be exactly that black under another foreground names it with `Text::color`.
- **Borders.** A stroke is the ring between its outer edge and its hole, and the hole's corners are the ones CSS gives a padding box. The formulas are `layout/stroke.rs`; `quad.wgsl`, `quad.hlsl` and the software screenshot renderer paint with them, and `stroke_readback_tests.rs` compares both backends with them over thirteen scenes. A renderer takes the width of its anti-aliasing ramp from derivatives over two by two pixels, so the comparison is strict where a pixel is whole or empty and loose on an edge. Dashes run along the outline with the period fitted to it; they are not fitted per side as a browser fits them, which is what a design tool draws. `RectCmd` gained `stroke_style`.
- **Sticky.** Where a sticky element is drawn is written into its parent's layout when the scroll container is synced with its scroll, as the content's offset is, so every walk of the layout sees it there and scrolling lays nothing out. The setting is in the node's box of rare styles, not in `Position`, which every node holds. It is painted after its siblings and hit before them.
- **Grid.** A subset of CSS grid: tracks of pixels, shares and content; gaps; auto-placement row by row; spans; a named column or row. No named lines or areas, no `minmax`, no dense packing, no per-child alignment. A child with a size of its own is not stretched. A grid lays out again whenever a child changed. Placement and track sizes are `layout/grid.rs`, with no layout engine in them.
- **E8.** The setting lives in the theme: `Theme::reduced_motion()`, `Theme::set_reduced_motion(Option<bool>)`, `Ctx::reduced_motion()`. The shell reads the system when a window opens and when one takes focus. While motion is reduced, transitions of transform, offset, width and height end at once; fades and color changes run; keyframe animations are left to the app. A tree with no shell never reads the system, so tests do not depend on the machine.
- **E9.** `App::load_hinted_font(data)`. The hinted outline comes from skrifa directly and is rasterized by zeno, because swash's scaler drops hints along x. Only pixel-snapped runs are hinted; a glyph under a transform is drawn unhinted.

### 13.3 To check on macOS and iOS

None of this has been compiled for an Apple target.

**The reduce-motion readers** were written blind. The macOS one has since been built on a Mac, where it needed one fix (`fc30d2f6`): without the winit shell nothing linked AppKit, and the lookup of `NSWorkspace` failed in the lib's tests. Whether it reads the setting right in a window, and the iOS one, are still to check. As first written: `crates/lurq/src/app/theme/motion.rs` has one `platform` module per target. The Windows one is built and tested. The other two were written from the API documentation.

- macOS sends `+[NSWorkspace sharedWorkspace]` and `-accessibilityDisplayShouldReduceMotion` with `objc2::msg_send!`, through `class!(NSWorkspace)`. To check: that it compiles with objc2 0.6 (`msg_send!` on a `*mut AnyObject` receiver, returning `bool`); that `NSWorkspace` resolves, which needs AppKit linked, as `objc2-app-kit` already links it; that the call is safe from the thread the shell calls it on, the main thread.
- iOS declares `UIAccessibilityIsReduceMotionEnabled()` as `unsafe extern "C"` in UIKit, returning `bool`. To check: that it links, and that it is right on the simulator target as well as the device.

Steps on a Mac:

1. `cargo test -p lurq --lib motion`. `the_platform_has_the_setting` fails if the reader returns `None`.
2. `cargo check -p lurq --features winit,wgpu`, then the whole check: the three commands of `scripts/check.ps1`, run by hand, since the script is PowerShell.
3. Run an example with a transition of a transform or a size, turn on System Settings › Accessibility › Display › Reduce motion, click back into the window: the transition should end at once, and run again after the setting is turned off. The setting is read on focus, not while the window stays focused.
4. iOS: build `crates/mobile_demo` for the device and the simulator (`crates/mobile_demo/ios/build-rust.sh` is what Xcode runs), turn on Settings › Accessibility › Motion › Reduce Motion, come back to the app: a sheet and a stack push should switch without the slide and still fade.

**Built on Windows, not looked at on a Mac or a phone.**

- E3 on touch. The touch paths (`app/runtime/touch.rs`, `touch_pointer.rs`) take a disabled element out of taps, long presses and the pressed look, and let a finger scroll over and inside it. The tests drive these paths, but not through a real finger.
- E9. The hinted path was looked at in a window on Windows only. On macOS and iOS: `cargo run -p lurq --example icon_hinting --features winit,wgpu,screenshot` on a 1x display if one is at hand; on a 2x display hinting matters little.
- E4 and E6 paint through quads every backend already draws, so nothing there is specific to a platform; a look at a focus ring growing in on Metal is still worth a minute.
- **The stroke shader on Metal.** Borders are painted by `quad.wgsl`, which was rewritten for strokes and is turned into Metal's shading language on a Mac and a phone. It was compiled and compared with its formulas on Direct3D only: the readback tests need a Win32 window. On a Mac run `cargo run -p lurq --example engine_gallery --features winit,wgpu,screenshot -- --capture target/gallery.png` and look at the frame, and at the window without `--capture`. If the shader does not compile for Metal the app stops with a wgpu validation error as its window opens. Otherwise look at the six cards under "Borders": a blue bar on the left of the first that follows both corners and thins out; a thin line on top and a thick one below on the second, meeting in smooth corners; a ring around the third that is outside the card and rounder than it; dashes that go around the corners on the fourth and fifth; a dashed line under the sixth. Nothing should show along an edge that has no border.
- The same page shows the rest: a form in a grid whose labels are as wide as the widest, tiles that span two columns and two rows, a list whose section header stays at the top while it scrolls and leaves with its section, a list of tasks where a click removes a task that fades and then lets the others close up, and all text in slate, the theme's foreground, not black. These have no code of their own per platform.
- E7 has no code of its own per platform. Its tests move the tree's clock, and two of them run a model of the winit loop (`app/winit_shell/frame_pacing_tests.rs`). It was looked at in a window on Windows with `examples/presence.rs`: `cargo run -p lurq --example presence --features winit,wgpu,screenshot -- --capture <dir>` runs a script that opens and closes two menus and a dialog on 0.4 s animations, saves a frame at each step and writes `passes.txt` with the layout passes of each stretch. On Windows the frames showed each layer half faded on its way in and out, one menu leaving under the next entering, the frame after a layer left equal pixel for pixel to the page with nothing open, 26 to 28 passes per animation and none in 1.5 s with every layer gone. The first animation after the window opened took 13 passes, which was not looked into. On macOS run the same command and compare; without `--capture` the window stays open to be clicked, which is how to check what the script cannot: a click where a layer is leaving reaches the page (the counter on the page goes up). The script's window has to be on screen and not covered: a window the system counts as hidden draws no frames and its captures come out one pixel large. On iOS there is no example: give a popup and a dialog of `crates/mobile_demo` the animations, and also close a dialog that holds a focused text input: the keyboard should go down as the dialog begins to leave, since the focus is let go then.

**By hand, in the demo.** `crates/demo` has five pages for what Phase 0 built, at the end of its side bar: Borders, Grid, Sticky, Presence and Style. Each has controls for what it shows and says what to try. `cargo run -p demo`, or `cargo run -p demo -- --route /presence` to open on a page; with `--features mcp` the app can be driven through lurq's MCP server, which is how the pages were gone through on Windows at a display scale of 1.5: clicks, the wheel, Tab, Escape and typing, with a frame captured after each. All of it behaved as its tests say. Two things showed that are not in the tests:

- **An outline was painted with its element**, so the element painted next covered it where they touch: of two buttons side by side with no gap, the ring of the first was cut on the side of the second (the Style page, "At the edge"). Fixed: see E4 in 13.2.
- **Text is clipped a pixel wider than its clip** (`expand_text_clip_for_rasterization`, older than this work). At a fractional scale a scroll container's border can cover its first row of pixels only in part, and a line of text scrolled under that edge shows faintly in that one row. Seen on the Sticky page, where it looked like a fault of sticky positioning and is not: it is there without it.

**Linux.** No reader: there is no reduce-motion setting every desktop shares. GNOME's is `org.gnome.desktop.interface enable-animations`, reachable through the settings portal over D-Bus, which lurq does not link. Until someone adds that, motion is reduced there only when the app calls `set_reduced_motion`.

### 13.4 Left to do

- **Phase 0 is built**, with what section 5 had deferred. What is left of it is the check on macOS and iOS in 13.3, which is the last step before Phase 1.
- **Presence, what is left.** `Sheet` and `StackOutlet` still animate out with their own code; they could move to exit animations, and nothing needs them to. A `Select` menu has no exit animation: its menu is built by lurq, so Phase 3 gives it one from the theme.
- **DevTools on the new font** (question 8). The font holds the eight glyphs DevTools needed beyond the first 144. Moving DevTools to it lets `crates/lurq/assets/lucide.ttf` go, which is the one third-party asset the published crate still carries without a notice.
- **`Icon`** is Phase 2: a name, the theme's table from names to a font and a character, the hinted font loaded by default.
- **The Pencil file** has the DevTools card on its Icons pages (152 glyphs, `10e83706`). `bolt` and `external-link` there, and from the first set `settings`, `airplane`, `quote`, `strikethrough`, `bug`, the three hands, `megaphone`, `replace` and `replace-all`, are the glyphs to redraw. `pencil/lurq-devtools.pen` came from another session and was committed as it stood.
- **Not built, and not asked for:** grid lines and areas by name, `minmax` tracks, dense packing and per-child alignment in a grid; dotted borders; a reduce-motion reader on Linux.
