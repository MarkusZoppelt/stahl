# Design language

Stahl Capital looks like the landing page at [zoppelt.net/stahl](https://zoppelt.net/stahl/): a private
bank with one client. That means steel, paper, a serif for the things worth announcing, and very
little decoration. This file is the reference for new screens. `tests/design.rs` enforces the parts
a test can check.

## Principles

1. **Steel and paper.** Dark is a steel-blue wall (`#0a0f13`). Light is the landing page's letter
   paper (`#e9e6dd`) with dark ink. Both themes are first class, so check every screen in both.
2. **One serif voice.** Newsreader is for names and big numbers: page titles, section titles, the
   total, key figures, the welcome sentence. Everything you read or operate (labels, tables,
   buttons, captions) is Inter. Never set running text in the serif.
3. **Rules, not boxes.** Group related things with a hairline above them (`ui::ledger`). A box
   (`ui::card`) is for things that must hold together: a table, a form, a list of findings. A box
   inside a box is a smell.
4. **Colour carries meaning, never decoration.** Teal means a gain or all clear, red a loss or a
   failure, amber a warning, cyan information. Each asset class keeps one colour on every page.
   Everything else is steel greys. No coloured backgrounds behind pages or cards.
5. **Depth only for what floats.** The palette, a chart readout and menus get a shadow and a blur
   (`ui::floating`). Nothing that sits on the page does.
6. **Quiet motion that means something.** Digits roll when a value changes, charts draw in once,
   a refresh flashes the total. All of it respects reduced motion. Nothing loops for attention,
   except the single breathing dot showing that prices are live.
7. **Plain words.** Say what it is and what happens. The private bank joke belongs on the welcome
   screen and the landing page, not in labels or errors.

## Tokens

All of these live in `src/theme.rs`. A page never defines its own.

| Token | Use |
|---|---|
| `HERO` | The total value. Newsreader 60, light, tabular figures (`HERO_COMPACT`, 46, on a phone) |
| `STATEMENT` | The welcome screen's sentence. Newsreader 46 (`STATEMENT_COMPACT`, 32, on a phone) |
| `LEDE` | The Overview's sentences about what changed. Newsreader 26 (`LEDE_COMPACT`, 21, on a phone) |
| `PAGE_TITLE` | Every page's name, with its section in a caption above. Newsreader 32 |
| `HOLDING_TITLE` | A holding's name in the details pane. Newsreader 24 |
| `FIGURE` | Key figures beside a label (`ui::stat`). Newsreader 24, tabular |
| `SECTION` | A section's name (`ui::title`). Newsreader 18 |
| `WORDMARK` | "Stahl Capital" in the sidebar. Newsreader 16 |
| `FACT` | The figures under the Overview's sentences. Inter 15, medium, tabular |
| `NUMBER` | Numbers in tables and badges. Inter 13, medium, tabular |
| `kit.ty.*` | Every other piece of text, from rune-kit, in Inter |
| `Theme::up/down/warn/info` | Gain, loss, warning, information. Nothing else |
| `Theme::class_color` | An asset class, the same on every page |
| `Theme::card`, `kit.c.border_*` | Boxes and hairlines |

Corner radii come from one scale: 1 to 3 for bars and dots, 6 for controls and rows, 8 for inner
panels, 12 for cards and floating panels. Spacing steps are 2, 4, 6, 8, 12, 16, 24 and 32. A page
has 32 points of side margin and 24 between its sections, and its content stops at 1240 points.

## Primitives

From `src/view/ui.rs`. Reach for these before writing layout.

| Primitive | What it is |
|---|---|
| `ui::door(size)` | The mark: the icon as a steel door, drawn live by the shader (`src/fx.rs`) when a compositor draws the window; the picture (`assets/door.png`) stands in for tests, which never draw, and for iOS. The sidebar has 28, the welcome screen 168 |
| `ui::wash(t, radius)` | The one soft light from above, behind a page |
| `ui::card(t)` | A flat box with a hairline border |
| `ui::ledger(t)` | A section with a rule over it and no box |
| `ui::rule(t)` | A hairline |
| `ui::floating(t)` | A surface over the page, with a shadow and a blur |
| `ui::title`, `ui::caption`, `ui::muted` | Section title, small label, secondary text |
| `ui::stat`, `ui::badge`, `ui::finding` | A labelled figure, a coloured value, a finding with its dot |
| `ui::share_bar`, `ui::stacked_bar`, `ui::gauge` | Shares of a whole, and progress against a target |
| `door::vault(s, t, size, place)` | The living mark: still at rest, its dial turns once per refresh and once as a portfolio opens, one mark when something needs attention. `door::welcome` and `door::mark` are the flight's two ends. Drawn by the door shader when the window has a compositor, otherwise by the picture |
| `ui::arrive`, `ui::entering`, `ui::appear`, `ui::float_in`, `ui::notice_in`, `ui::page_arrive` | How things arrive: see "Motion" |
| `ui::flash`, `ui::price_flash`, `ui::class_flash` | What a refresh moved, shown once in its colour |

A new kind of thing on more than one page becomes a primitive here, with its look decided once.

## Motion

Motion says what changed and where it went. There are few kinds, all in `src/view/ui.rs`,
`src/view/door.rs` and `src/fx.rs`; a page uses them and never writes its own offsets, springs or
timings.

| What happens | Motion | Primitive |
|---|---|---|
| Content arrives (cards, sections, a list) | Rises 12 pt and sharpens from a 5 pt blur; cards follow one another 0.05 s apart | `ui::entering(card, index)`, `ui::arrive()` |
| Moving between pages | The new page comes 10 pt from the side moved towards: up from below going down the sidebar, from above going back; along the tabs on a phone. The old page only fades | `ui::page_arrive`, applied by the page container |
| A small thing appears in place (a badge, a button, a mark) | Scales from 0.9 and fades | `ui::appear()` |
| Something floats over the page (the palette, a menu) | Scales from 0.96 out of a 4 pt blur | `ui::float_in()` |
| A banner or an error | Drops 6 pt from above | `ui::notice_in()` |
| A value follows data (a bar, a highlight) | Springs on `LEAD` | `.motion(LEAD)` |
| Prices land | What moved washes once in its gain or loss colour, in 0.12 s and out over 0.8 s; digits roll; the door's tumblers turn once, on a heavy spring that settles with a small give in about a second | `ui::flash`, `RollingText`, `door::vault` |
| The pointer over the door | The light on its steel follows, within two radii, easing by 6% a frame; it rests up and to the left when nobody steers. The shader does this, so no view is rebuilt | `src/fx.rs` |
| A portfolio opens | The welcome door travels to the mark and shrinks to it, the dial turning once as it lands. Nothing covers the page. With no welcome door, only the dial turns | `door::flight` |
| A press | A card gives to 0.98, a row to 0.995. Buttons don't move | `ui::PRESS_CARD`, `ui::PRESS_ROW` |

Rules:

- Motion plays once, on a change, and then everything is still. A window at rest draws nothing.
  Only the live-prices dot and loading placeholders breathe.
- Keyed by what changed: a flash is keyed by the refresh, the door's flight by the portfolio's
  shell first showing after the welcome screen, so neither replays on a page change, a redraw or a refresh.
- Reduced motion lands every one of these at its final pose at once: nothing travels, turns or
  flashes, and the door appears in its place instead of flying. The door's light doesn't chase the
  pointer either.
- A new kind of motion is added here and in `ui.rs` first, then used.

## The icon

The icon is the steel door, rendered by the landing page's shader in `site/door.js` from the
flat rings in `assets/icons/icon.svg`. `site/render-icons.sh` regenerates every size from it:
`assets/icons/*`, `assets/door.png` (the mark inside the app), and the page's favicon and logo.
Change the shader, not the PNGs.

The app draws the same door live: `src/fx.rs` is that shader in WGSL, run through Rune's
compositor hook, so the light, the tumblers and the opening are real. Keep the two in step: change
both shaders together, and `assets/door.png` stays the fallback the picture is compared against.

## Checking a screen

Tests can't judge looks. Render what you changed in both themes and look at it:

```sh
cargo run --example demo -- --snap /tmp/x.png --scene overview --at 2.5 --size 1320x860
cargo run --example demo -- --snap /tmp/x.png --scene light --at 2.5 --size 1320x860
```

Then regenerate `screenshots/` and the landing page's footage with `site/render-media.sh`, so the
page keeps showing the app as it is.
