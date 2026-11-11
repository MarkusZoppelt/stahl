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
5. **Depth only for what floats.** The palette, a chart readout and menus get a shadow and a
   blur (`ui::floating`, `ui::glass`). Nothing that sits on the page does. With a compositor a
   floating panel is real glass: the page behind it is blurred and slightly bent, its rim is lit
   by the room's light, and a tint keeps the text legible over anything.
6. **One room, one light, quiet motion that means something.** The whole app is one steel room lit
   by one light: it follows the pointer, rests above the middle of the window, and takes the day's
   colour. Digits roll when a value changes, charts draw in once, a refresh sends one pulse of
   light across the page, a new record runs light along the total once, and a pressable surface
   lifts toward the pointer. The room is alive while the owner is touching it or their money is
   changing, and comes to rest when they stop. All of it respects reduced motion. Nothing loops
   for attention, except the single breathing dot showing that prices are live and the light on
   the live end of the chart.
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
| `ui::light(s, t, radius)` | The room's one light on the wall behind a page: a soft pool that follows the pointer, rests above the middle, and takes the day's colour. Without a compositor it is `ui::wash` |
| `ui::wash(t, radius)` | The still light from above, for a window without a compositor |
| `ui::glass(s, t)` | A floating surface made of glass: a lens that blurs and bends the page behind it, a lit rim, a tint for legibility, and a shadow. Without a compositor it is `ui::floating` |
| `ui::glow_orb(color, size, strength)` | A soft round light, for the head and live end of the value chart |
| `ui::card(t)` | A flat box with a hairline border |
| `ui::ledger(t)` | A section with a rule over it and no box |
| `ui::rule(t)` | A hairline |
| `ui::floating(t)` | A surface over the page, with a shadow and a blur |
| `ui::title`, `ui::caption`, `ui::muted` | Section title, small label, secondary text |
| `ui::stat`, `ui::badge`, `ui::finding` | A labelled figure, a coloured value, a finding with its dot |
| `ui::share_bar`, `ui::stacked_bar`, `ui::gauge` | Shares of a whole, and progress against a target |
| `door::vault(s, t, size, place)` | The living mark: still at rest, its tumblers turn once per refresh, one mark when something needs attention, and it can be turned by hand. `door::welcome` is the welcome screen's large door and `door::mark` the small one. Drawn by the door shader when the window has a compositor, otherwise by the picture |
| `ui::arrive`, `ui::entering`, `ui::appear`, `ui::float_in`, `ui::notice_in`, `ui::page_arrive` | How things arrive: see "Motion" |
| `ui::settle`, `ui::catch_light` | A new record being set onto the wall: the plate settles and the light passes over it once |
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
| Prices land | What moved washes once in its gain or loss colour at 0.18 s (after the pulse reaches it), in 0.12 s and out over 0.8 s; digits roll; the door's tumblers turn once, on a heavy spring that settles with a small give in about a second; one soft ring of light leaves the door mark and crosses the wall, once, over 0.95 s | `ui::flash`, `RollingText`, `door::vault`, `ui::light` |
| The pointer moves | The room's light follows it on a heavy, damped spring, settling in about two seconds; it rests above the middle of the window when the pointer leaves. Every door's steel is lit from it, and the total's sheen band moves with it. The shader does this, so no view is rebuilt | `src/fx.rs` |
| A new record | Light runs once along the total, over 1.3 s, the first time the record is seen that day. Not confetti: the metal catches one line of light | `ui::sheen`, `statement::is_record` |
| The pointer over a tile or a pressable surface | The edge nearest it catches a faint light, easing in and out; the surface's own hover fill springs as before | `fx::lift`, `.hover()` |
| The door is turned by hand | Dragging the sidebar mark or the welcome door turns the dial with the pointer's angle, on the same heavy spring; letting go carries the hand's speed, so the steel coasts past and settles. Left and right arrows turn one notch when the door has focus | `door::vault`, `src/fx.rs` |
| Something is being composed (an answer) | The door's tumblers turn slowly and the light on its steel breathes until the answer lands, then settle on a whole turn | `State::door_busy` |
| A portfolio opens | The landing page's opening, over about 1.8 s: the door starts where the welcome screen's was (or in the window's middle when the app launches straight into a portfolio), its tumblers turn, it pushes in until it fills the window, and its halves part to reveal the page, already laid out, between them | `door::opening`, `src/fx.rs` |
| A press | A card gives to 0.98, a row to 0.995. Buttons don't move | `ui::PRESS_CARD`, `ui::PRESS_ROW` |
| A new record arrives | The plate is set onto the wall: it comes the last six points out of a slight blur and settles, and a pale light falls over it once and fades | `ui::settle`, `ui::catch_light` |

Rules:

- Motion plays once, on a change, and then everything is still. A window at rest draws nothing.
  Only the live-prices dot, the chart's live end and loading placeholders breathe. The room's
  light is alive only while the pointer moves, and settles when it stops.
- Keyed by what changed: a flash is keyed by the refresh, the door's opening by the portfolio's
  shell first showing, the pulse and the record's run by the refresh and the day, so none replays
  on a page change, a redraw or a refresh.
- Reduced motion lands every one of these at its final pose at once: nothing travels, turns,
  flashes or breathes, and the page simply appears instead of the door opening. The light does
  not chase the pointer, the pulse and the record's run do not play, and the door's tumblers do
  not coast.
- A new kind of motion is added here and in `ui.rs` first, then used.

## The icon

The icon is the steel door, rendered by the landing page's shader in `site/door.js` from the
flat rings in `assets/icons/icon.svg`. `site/render-icons.sh` regenerates every size from it:
`assets/icons/*`, `assets/door.png` (the mark inside the app), and the page's favicon and logo.
Change the shader, not the PNGs.

The app draws the same door live: `src/fx.rs` is that shader in WGSL, run through Rune's
compositor hook, so the light, the tumblers and the opening are real. The door is lit from the
room's one light, not a light of its own, so the steel and the wall agree. Keep the two shaders in
step: change both together, and `assets/door.png` stays the fallback the picture is compared
against.

## The room

`src/fx.rs` is one compositor for the whole room, and everything in it hangs off one light:

- The light is a position in window points on a heavy, damped spring. It follows the pointer,
  rests above the middle of the window, and takes the day's colour: the faintest teal up, the
  faintest red down, steel blue when flat or unknown, growing with the size of the move and
  capped low. It must never cost text contrast.
- The room slot (`ui::light`) is the pool of light on the wall, behind the page, with a shade
  away from it and a static half-LSB dither so the gradient never bands. It also carries the
  refresh's pulse.
- The door's steel, the total's sheen (`ui::sheen`), the glass rims (`ui::glass`) and the edge
  of a pressable surface (`fx::lift`) all take their direction from that light.
- rune-fx (`rune-fx` at Rune's `rev`) draws the glass and the chart's glows through the same
  compositor; its layers come from `El::effect` slots like the door's.
- Everything is drawn only while the window is animating. `Gfx::animating` must return false at
  rest, and the demo prints "at-rest frames" so you can check it.

## Checking a screen

Tests can't judge looks. Render what you changed in both themes and look at it:

```sh
cargo run --example demo -- --snap /tmp/x.png --scene overview --at 2.5 --size 1320x860
cargo run --example demo -- --snap /tmp/x.png --scene light --at 2.5 --size 1320x860
```

Moving things have scenes of their own: `pulse` (a refresh's ring crossing the wall),
`record` (light running along the total), `performance-draw` (the chart drawing in),
`door-play` and `door-thinking` (the dial turned by hand, and the door thinking). Pointer
strips need a script: `--script move.json` with `{"t":0.2,"type":"move","x":300,"y":600}`.

Then regenerate `screenshots/` and the landing page's footage with `site/render-media.sh`, so the
page keeps showing the app as it is.
