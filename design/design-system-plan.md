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
- **They are drawn in the Pencil file**: 144 glyphs on a 24-unit grid with a 2-unit stroke, one `Icon/<name>` component each, on the Icons pages next to a table from Lucide names to lurq names. Every icon in the file is an instance of one of them; no Lucide glyph is left. A few need a second pass: settings, airplane, quote, strikethrough, bug, the three hands, megaphone, replace and replace-all.
- **How they reach the screen is open** (section 12): an icon font generated from the sources, or vector paths. A font needs no optional feature because text rendering is always present.
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

Deferred until a component needs them: dashed borders, a single-side border that follows the corner radius, grid layout, sticky positioning. No z-index — overlay layers already cover it.

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
| Phase 0, E5–E8 | — | small to medium; E7 is the largest |
| Phase 1, tokens and schemes | E1 | medium; the palette migration touches every themed component and the docs |
| Phase 1, theme machinery | E1–E4 | medium to large; it is new architecture |
| Step 2, proof on four components | Phase 1 | medium; expect to revise the machinery here |
| Glyph set (D5) | — | drawn in Pencil (144 glyphs); left: a second pass on a few, and the export to code |
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
- **The icon set lives only in Pencil.** The 144 glyphs are drawn and the component sheets use them, but nothing in the repository holds them yet. They need a source format in the repository and a way to the screen (question 7), and a few need redrawing.
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
7. How do the glyphs reach the screen: an icon font generated from sources kept in the repository, or vector paths? A font works with no optional feature; paths need the `svg` or `canvas` machinery, or a new path primitive.
8. Does DevTools move to the new glyphs at once, so `lucide.ttf` can be removed, or keep Lucide with a notice file for now?
9. Is a `gallery` binary acceptable in `crates/demo`, or should it be its own crate?
10. Which platforms must screenshot tests run on in CI?
