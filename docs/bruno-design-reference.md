# Bruno Design Reference

Extracted from the upstream Bruno React app kept under `reference/packages/bruno-app/`,
for porting the visual language into the bruno-rs gpui UI (`crates/bru-app`).

## Source of truth

The design language lives in **styled-components theme objects**, not CSS variables:

- Token shape (the contract): [`src/themes/schema/oss.js`](../reference/packages/bruno-app/src/themes/schema/oss.js)
- Theme registry (13 themes): [`src/themes/index.js`](../reference/packages/bruno-app/src/themes/index.js)
- Canonical themes: [`src/themes/dark/dark.js`](../reference/packages/bruno-app/src/themes/dark/dark.js),
  [`src/themes/light/light.js`](../reference/packages/bruno-app/src/themes/light/light.js)
- Global typography/scrollbars: [`src/styles/globals.css`](../reference/packages/bruno-app/src/styles/globals.css)

> ⚠️ **Ignore the `:root` vars in `globals.css`** (e.g. `--color-brand: #546de5`, an indigo).
> Those are legacy GraphiQL-era leftovers. The real brand color is **Bruno gold**
> (`#D9A342`, `hsl(39, 74%, 59%)`). Components read the styled-components `theme.*` object,
> not those CSS vars.

Themes are built from a per-mode **`palette`** (raw hues + neutrals) that is then mapped
onto **semantic tokens** (`background.base`, `request.methods.get`, etc.). Two derivations
use the `polished` lib: `rgba(color, a)` (status tints at 0.15 alpha) and
`lighten`/`darken` (e.g. dark `delete = lighten(0.08, RED)`, button hover states).

---

## Brand identity

| Token | Dark | Light |
|---|---|---|
| `brand` / `primary.solid` | `hsl(39, 74%, 59%)` (gold) | `hsl(33, 80%, 46%)` (amber) |
| `primary.text` (links, colored text) | `hsl(39, 74%, 64%)` | `hsl(33, 67%, 45%)` |
| `primary.strong` (tab underline, thick border) | `hsl(39, 74%, 64%)` | `hsl(33, 67%, 50%)` |
| `primary.subtle` (focus ring) | `hsl(39, 74%, 54%)` | `hsl(33, 69%, 56%)` |
| `colors.accent` / system control accent | `#D9A342` | `#b96f1d` |

The brand is a warm gold/amber in both modes — light mode just darkens it for contrast.

---

## Core neutrals (resolved values)

### Backgrounds — layered "geological strata" (crust → mantle → base, then surfaces stack up)

| Token | Role | Dark | Light |
|---|---|---|---|
| `background.base` | app canvas / main content | `#1a1a1a` (`hsl(0 0% 10%)`) | `#ffffff` |
| `background.mantle` | sidebars, panels | `#222224` | `#f8f8f8` |
| `background.crust` | status bar, app shell | `#333333` | `#f6f6f6` |
| `background.surface0` | cards, inputs | `#26292b` | `#f1f1f1` |
| `background.surface1` | hover states | `#444444` | `#eaeaea` |
| `background.surface2` | active/pressed, dividers | `#666666` | `#e5e5e5` |

### Text hierarchy

| Token | Role | Dark | Light |
|---|---|---|---|
| `text` / `colors.text.white` | primary text | `#cccccc` (`hsl(0 0% 80%)`) | `#343434` |
| `colors.text.subtext2` | strong secondary | `#bbbbbb` | `#666666` |
| `colors.text.subtext1` / `muted` | supporting | `#aaaaaa` | `#838383` |
| `colors.text.subtext0` | hints, timestamps, placeholders | `#999999` | `#9B9B9B` |

### Borders & overlays

| Token | Role | Dark | Light |
|---|---|---|---|
| `border.border0` | subtle card outlines | `#2a2a2a` | `#efefef` |
| `border.border1` | default dividers, inputs | `#333333` | `#e5e5e5` |
| `border.border2` | focus, selected | `#444444` | `#cccccc` |
| `overlay.overlay0` | subtle dim | `#444444` | `#C0C0C0` |
| `overlay.overlay1` | standard overlay | `#555555` | `#B0B0B0` |
| `overlay.overlay2` | modal backdrop | `#666666` | `#8b8b8b` |

---

## Status / intent colors

`intent` maps `INFO→BLUE`, `SUCCESS→GREEN`, `WARNING→ORANGE`, `DANGER→RED` (from the hue
palette below). Each `status.*` token is a triplet:

- `background` = `rgba(intent, 0.15)` — a 15%-alpha tint
- `text` = the solid intent hue
- `border` = the solid intent hue

| Intent | Dark hue | Light hue |
|---|---|---|
| info | `hsl(210, 90%, 76%)` | `hsl(214, 55%, 45%)` |
| success | `hsl(140, 72%, 68%)` | `hsl(145, 50%, 36%)` |
| warning | `hsl(24, 88%, 72%)` | `hsl(35, 85%, 42%)` |
| danger | `hsl(8, 70%, 52%)` | `hsl(8, 60%, 52%)` |

---

## Hue palette (raw material — 14 hues)

These are the source hues every semantic color derives from. Dark hues are light & saturated
(for dark bg); light hues are darker & muted (for light bg).

| Hue | Dark | Light |
|---|---|---|
| RED | `hsl(8, 70%, 52%)` | `hsl(8, 60%, 52%)` |
| ROSE | `hsl(367, 84%, 70%)` | `hsl(352, 45%, 50%)` |
| BROWN | `hsl(35, 65%, 72%)` | `hsl(28, 55%, 38%)` |
| ORANGE | `hsl(24, 88%, 72%)` | `hsl(35, 85%, 42%)` |
| YELLOW | `hsl(41, 93%, 72%)` | `hsl(45, 75%, 42%)` |
| LIME | — | `hsl(85, 45%, 40%)` |
| GREEN | `hsl(140, 72%, 68%)` | `hsl(145, 50%, 36%)` |
| GREEN_DARK | `hsl(160, 90%, 44%)` | — |
| TEAL | `hsl(170, 70%, 60%)` | `hsl(178, 50%, 36%)` |
| CYAN | `hsl(190, 82%, 72%)` | `hsl(195, 55%, 42%)` |
| BLUE | `hsl(210, 90%, 76%)` | `hsl(214, 55%, 45%)` |
| INDIGO | `hsl(202, 88%, 72%)` | `hsl(235, 45%, 45%)` |
| VIOLET | `hsl(260, 75%, 78%)` | `hsl(258, 42%, 50%)` |
| PURPLE | `hsl(285, 72%, 75%)` | `hsl(280, 45%, 48%)` |
| PINK | `hsl(305, 59%, 74%)` | `hsl(328, 50%, 48%)` |

---

## Request method colors

The sidebar/URL-bar method badges. **Note the dark and light maps differ** (POST and
PATCH change hue between modes):

| Method | Dark | Light |
|---|---|---|
| GET | GREEN | GREEN |
| POST | INDIGO | PURPLE |
| PUT | ORANGE | ORANGE |
| DELETE | RED (lightened 8% in dark) | RED |
| PATCH | ORANGE | PURPLE |
| OPTIONS | TEAL | TEAL |
| HEAD | CYAN | CYAN |
| grpc | TEAL | INDIGO |
| ws | ORANGE | ORANGE |
| gql | PINK | PINK |

---

## Typography

| | Value |
|---|---|
| UI font | `Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif` |
| Mono font | `'Fira Code', monospace` (fallback `Consolas, Inconsolata, Droid Sans Mono, Monaco`) |
| Weights | regular `400`, medium `500` |

Font size scale (`font.size.*`, mode-independent):

| Token | rem | px |
|---|---|---|
| xs | 0.6875 | 11 |
| sm | 0.75 | 12 |
| base | 0.8125 | 13 |
| md | 0.875 | 14 |
| lg | 1.0 | 16 |
| xl | 1.125 | 18 |

The default UI text size is **13px** (`base`). This is a dense, IDE-style UI.

---

## Spacing, radii, shadows

**Border radius** (`border.radius.*`, mode-independent): `sm 4px`, `base 6px`, `md 8px`,
`lg 10px`, `xl 12px`. The `Button` also supports `full` = `9999px` (pill).

**Shadows** (`shadow.*`): dark uses heavier black; light is soft.

| | Dark | Light |
|---|---|---|
| sm | `0 1px 3px rgba(0,0,0,0.5), 0 0 0 1px rgba(0,0,0,0.3)` | `0 1px 3px rgba(0,0,0,0.12), 0 0 0 1px rgba(0,0,0,0.05)` |
| md | `0 2px 8px rgba(0,0,0,0.6), 0 0 0 1px rgba(0,0,0,0.4)` | `0 2px 8px rgba(0,0,0,0.14), 0 0 0 1px rgba(0,0,0,0.06)` |
| lg | `0 2px 12px rgba(0,0,0,0.7), 0 0 0 1px rgba(0,0,0,0.4)` | `0 2px 12px rgba(0,0,0,0.15), 0 0 0 1px rgba(0,0,0,0.05)` |

Spacing has no formal token scale — components use rem literals. Common rhythm seen in
`Button`: padding `0.25–0.75rem` vertical, `0.5–1.5rem` horizontal; gaps `0.25–0.75rem`.

---

## Component idiom

The canonical modern component is `ui/Button`
([index.js](../reference/packages/bruno-app/src/ui/Button/index.js),
[StyledWrapper.js](../reference/packages/bruno-app/src/ui/Button/StyledWrapper.js)).
Its prop matrix is the design system's interaction vocabulary:

- **`size`**: `xs | sm | base | md | lg` — controls padding, font-size, icon size.
- **`variant`**: `filled | outline | ghost`
  - `filled` = solid `bg` + `text`, border = `bg`; hover `darken(0.03)`, active `darken(0.07)`.
  - `outline` = transparent bg, colored text + border; hover `rgba(color, 0.05)`, active `0.1`.
  - `ghost` = transparent bg + border, colored text; hover `rgba(color, 0.1)`, active `0.15`.
- **`color`**: `primary | light | secondary | success | warning | danger` — each is a
  `{bg, text, border}` triplet under `theme.button2.color.*`.
- **`rounded`**: `sm | base | md | lg | full`.
- **`fontWeight`**: `regular (400) | medium (500)`.
- Plus `icon` + `iconPosition (left|right)`, `loading` (spinner), `fullWidth`, `disabled`.
- Transitions: `all 0.15s ease`. Focus ring: `box-shadow: 0 0 0 2px rgba(color, 0.4)`.
  Disabled: `opacity 0.7`, `cursor not-allowed`.

`button2.color` triplets (the primary CTA is **gold bg + black text** in dark, white text in light):

| color | Dark (bg / text / border) | Light (bg / text / border) |
|---|---|---|
| primary | gold / `#000` / gold | amber / `#fff` / amber |
| light | `rgba(gold,0.08)` / gold / `rgba(gold,0.06)` | same pattern |
| secondary | mantle / text / border1 | mantle / text / border2 |
| success | GREEN / `#fff` / GREEN | GREEN / `#fff` / GREEN |
| warning | ORANGE / `#1e1e1e` / ORANGE | ORANGE / `#fff` / ORANGE |
| danger | RED / `#fff` / RED | RED / `#fff` / RED |

`ui/MethodBadge` ([StyledWrapper.js](../reference/packages/bruno-app/src/ui/MethodBadge/StyledWrapper.js)):
colored by `request.methods[method]`, uppercase, weight 600. `md` size = fixed 52px width,
`xs` font; `sm` size = 9px monospace pill (`border-radius 3px`).

Other `ui/*` primitives present: `ActionIcon`, `ErrorBanner`, `HeightBoundContainer`,
`MenuDropdown`, `ResponsiveTabs`, `StatusBadge`. All use the same
`styled-components + theme.*` idiom (a `StyledWrapper.js` + `index.js` per component).

---

## Syntax highlighting (CodeMirror tokens)

Relevant to `crates/bru-app/src/highlight.rs`. From `codemirror.tokens.*`, derived from a
`syntax` map over the hue palette:

| Token | Hue |
|---|---|
| keyword, tag, atom | ROSE |
| variable, number | PINK |
| property, definition | BLUE |
| string | BROWN |
| operator, tagBracket | SUBTEXT1 (muted) |
| comment | SUBTEXT0 (most muted) |

Editor variable validity: `valid` = GREEN_DARK (dark) / GREEN (light), `invalid` = RED,
`prompt` = BLUE.

---

## Mapping to the current gpui port (`crates/bru-app/src/theme.rs`)

The port already hand-encodes a subset as `dark, light` pairs:

| theme.rs fn | Maps to | Status |
|---|---|---|
| `bg` `0x1a1a1a / 0xf6f6f7` | `background.base` | ✅ matches dark; light tweaked |
| `mantle` `0x222224 / 0xececee` | `background.mantle` | ✅ dark exact |
| `surface0` `0x26292b / 0xe1e3e6` | `background.surface0` | ✅ dark exact |
| `input_bg` `0x1b1b1b / 0xffffff` | input bg | ~ (theme uses `transparent` dark) |
| `border1` `0x333333` / `border2` `0x444444` | `border.border1/2` | ✅ dark exact |
| `text` `0xcccccc / 0x1c1e22` | `text` | ✅ dark exact |
| `subtext` `0xaaaaaa` / `muted` `0x808080` | `subtext1` / `subtext0`-ish | ✅ close |
| `accent` `0xd9a342 / 0xb07d1e` | gold brand | ✅ dark exact (`#D9A342`) |
| `green/blue/orange/red` | GREEN / BLUE / ORANGE / RED hues | ✅ approximate dark hues |

### Gaps to fill when extending the port

1. **Method colors diverge.** `theme.rs::method_color` uses `POST→orange`, `PUT/PATCH→blue`.
   Reference uses `POST→INDIGO`, `PUT/PATCH→ORANGE` (and per-mode differences). Also missing
   `OPTIONS/HEAD` and the `grpc/ws/gql` protocol colors. Decide: match reference or keep the
   current iced-derived mapping.
2. **Missing background layers**: no `crust`, `surface1`, `surface2` — the port collapses to
   `bg/mantle/surface0`. The strata model needs all six for hover/active depth.
3. **Missing scales**: no border-radius scale (4/6/8/10/12), no font-size scale
   (11–18px, base 13), no shadow tokens.
4. **Missing neutrals**: no `border0`, no `overlay0/1/2`, only one `subtext` (ref has
   subtext0/1/2).
5. **Missing status tints**: the `rgba(intent, 0.15)` info/success/warning/danger
   backgrounds aren't represented.
6. **Missing hues**: only green/blue/orange/red exist; teal/cyan/indigo/violet/purple/pink/
   yellow/rose/brown are needed for protocols, syntax, and badges.
7. **Fonts**: UI should be **Inter**, code **Fira Code** — confirm these are bundled/available
   to gpui.

### Suggested next step

Grow `theme.rs` from a flat color list into the strata + scales above (still as `dark/light`
pairs). The two canonical themes here give every resolved value needed; the other 11 themes
(monochrome, pastel, Catppuccin ×4, Nord, VS Code ×2) can come later from
`src/themes/{dark,light}/*.js` since they all conform to the same schema.
