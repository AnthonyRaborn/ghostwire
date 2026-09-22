# GHOSTWIRE — ambient netrunner rig

A Rust + Ratatui terminal dashboard you leave running on a spare monitor. In the
fiction you're jacked into the net, and each live public data feed is a node your rig
has breached. The fiction is driven by real behavior: a refresh decrypts the node, stale
data decays, a failed request is ICE, and a rate limit is a trace.

## Decisions

| Question | Choice |
|---|---|
| Function | Monitoring dashboard (not a game) |
| Data | Public live feeds |
| Platform | Terminal UI |
| Theme depth | Diegetic — the fiction shapes wording, flows, and effects |
| Role | Netrunner rig |
| Usage | Ambient display on a spare monitor |
| Stack | Rust + Ratatui |
| Layout | Hybrid: full grid, with a periodic full-screen dive into one node |
| API keys | Free keys OK (Finnhub for stocks only; everything else keyless), kept in `keys.toml` |
| Location | Set in the config file; nothing auto-detected |
| FX level | Active by default (`calm` / `active` / `chaotic` selectable) |

## Nodes

| Node | Shows | Source | Key | Poll |
|---|---|---|---|---|
| ZAIBATSU INDEX | Stock quotes | Finnhub | Free key | 60s in market hours, 15m otherwise |
| | Crypto prices + 7d sparkline | CoinGecko `/coins/markets` | None (free demo key optional) | 2m |
| ATMOS // SECTOR | Temp, rain, wind, AQI, UV | Open-Meteo forecast + air-quality | None | 10m |
| INTERCEPTS | HN top stories, new CISA KEV entries, optional RSS | HN Firebase API, CISA KEV JSON | None | 5m / 1h |
| SEISMIC // HELIOS | Quakes near you + big ones worldwide; Kp (1-min estimate + 72h of 3-hourly), X-ray flux, NOAA G/S/R scales | USGS GeoJSON feeds; NOAA SWPC JSON | None | 2m; 10m |
| SKYTRAFFIC | Airborne aircraft within `flight_radius_km` (150 km) | OpenSky Network | None (400 credits/day anonymous; ≤25 sq° costs 1) | 5m |

A node can be fed by more than one source (ZAIBATSU = stocks + crypto, INTERCEPTS =
HN + KEV + RSS, SEISMIC // HELIOS = quakes + space weather — both low-density feeds
sharing a slot). Link status is tracked per source; the node shows the best of its
sources, and the footer ticker reports any source that's in trouble. The grid has one
open cell as a result — nothing lives there yet.

## Screen (hybrid layout)

```
 GHOSTWIRE // RIG-07 ░ uplink 5/5 ░ neural load 3% ░ 21:14:07 NET
┌ ZAIBATSU INDEX ────────┐┌ ATMOS//SECTOR-4 ───────┐┌ INTERCEPTS ────────────┐
│ NVDA   182.40 ▲2.1% ▅▆▇││ 17°C  RAIN 20%  WIND 9 ││ ▓ HN  Show HN: a tiny… │
│ BTC    61,204 ▼0.8% ▇▆▅││ SMOG AQI 42 ░░▒  UV 3  ││ ▓ KEV CVE-2026-41822   │
└────────────────────────┘└────────────────────────┘└────────────────────────┘
┌ SEISMIC // HELIOS ─────┐┌ SKYTRAFFIC ────────────┐
│ M2.1  38km NE   4m ago ││ 7 contacts overhead    │
│ Kp 3 ▂▃▃▅  G0 S0 R0    ││ UAL1234  FL340  ↗ 452kt│
└────────────────────────┘└────────────────────────┘
 » diving SKYTRAFFIC in 12s ░ NOAA-SWPC: ICE, retry 30s ░ 2 ghosts cached
```

Every 45s the rig **dives** into one node: it takes over the grid for 15s with detail
(bar charts, full lists, a radar scope with a rotating sweep for quakes and flights),
then surfaces. Nodes take turns in grid order, skipping any with nothing to show.
("Breach" means fetching — `» breaching`, `[r] re-breach` — so the full-screen view got
its own word.)

## Real state → in the rig

| Real state | In the rig |
|---|---|
| App launch | Jack-in: a boot log types itself out — real facts about this run (config, sector, keys, ghost cache, node count) — then every node decrypts in |
| Refresh | Scramble-then-resolve animation on changed values only |
| Data getting old | Signal decay: colors dim, noise creeps in, age shown |
| Fetch error | ICE detected, retry countdown; 3+ consecutive failures → FLATLINED |
| HTTP 429 | TRACE ACTIVE — the source goes dark for the backoff window |
| Missing key / location | OFFLINE with the reason (no retry until config changes) |
| Cached data on restart | Ghost data, dimmed until live data lands |
| Notable event | PRIORITY INTERCEPT: banner in the ticker for 30s, glitch burst, and the node is dived into next, within 2s |

Notable events: a price crossing ±3% (re-arms below 2.5%), a new quake ≥M4 within your
radius from the last 6 hours, Kp crossing 5 (re-arms below 4.5), or a new KEV entry
compared with the previous catalog.

## Keys

| Key | Grid | Dive |
|---|---|---|
| `1`–`5` | dive into that node (the number is in its title) | switch to that node |
| `space` | dive into the next node now | surface |
| `p` | hold the dive cycle (freezes timers) | hold / release |
| `esc` | jack out | surface |
| `r` | re-breach: every source fetches now | same |
| `q`, `ctrl-c` | jack out | same |

Any key skips the boot log.

## FX levels

| | calm | active (default) | chaotic |
|---|---|---|---|
| Boot log | skipped | ~2.5s | ~3.5s |
| Decrypt on refresh (changed cells only) | 300ms | 700ms | 1s |
| Glitch on ICE / TRACE / FLATLINED | — | yes | stronger |
| Glitch on priority intercept | small | yes | stronger |
| Unprompted glitches | — | every 20–40s | every 2–6s |
| Decay static on stale data | — | yes | yes |
| Hex sparkle in empty space | — | — | yes (10 fps) |
| Scanlines | yes | yes | yes |

Measured in a release build, demo mode, 40s with a dive every 20s: calm 0.6% CPU,
active 0.9%, chaotic 1.05% of one core, about 13 MB resident.

## Architecture

- **Crates:** ratatui 0.30, crossterm (`event-stream`), tokio, reqwest, serde/serde_json,
  toml, chrono, chrono-tz, directories, clap, tracing (to a log file — the TUI owns
  stdout), fastrand, libc (process CPU for "neural load").
- **Effects are hand-rolled** rather than `tachyonfx`: the core effect decrypts only the
  cells whose content changed since the last frame (each node keeps a per-cell hash
  snapshot), which doesn't map onto tachyonfx's cell filters. Every effect is a pure
  function of cell position and elapsed time, so frames are reproducible in tests.
- **Concurrency:** one tokio task per source, each on its own interval, reporting over an
  `mpsc` channel to the app loop. The UI never waits on the network. `r` signals every
  source task to re-breach immediately.
- **`Feed` trait:** `source()`, `interval()`, `fetch() -> Result<Reading, FetchError>`.
  JSON parsers are pure functions tested against saved fixtures. `FetchError` maps
  straight onto the fiction: `Failed` → ICE, `RateLimited` → TRACE,
  `NotConfigured` → OFFLINE.
- **Frame budget:** 30 fps only while an effect or radar sweep is running; 10 fps for
  chaotic mode's sparkle; otherwise one frame a second, aligned to the clock. Target <2%
  CPU on the spare monitor (met — see FX levels).
- **`lexicon.rs`:** all in-fiction wording lives in one file so the voice stays
  consistent and tunable.
- **Disk cache:** last good payload per source, so a restart shows ghost data instantly
  and makes fewer API calls.
- **`--demo` ("construct") mode:** synthetic feeds (random-walk prices, drifting weather,
  scattered quakes, occasional injected ICE/TRACE) with no network, for development and
  for leaving it running without spending API quota. Parsers are covered separately by
  fixture tests.

```
src/
  main.rs  app.rs  event.rs  config.rs  keys.rs  paths.rs  cache.rs  lexicon.rs  theme.rs
  colordepth.rs  256-color fallback: RGB downsampled to xterm-256 when COLORTERM
                  isn't truecolor/24bit
  source.rs     SourceId / NodeId / link-status model
  dive.rs       the dive cycle (rotation, priority queue, hold)
  intercept.rs  priority-intercept detection, with hysteresis
  feeds/   mod.rs (Feed trait + runner) · http.rs · demo.rs · one file per real source
  ui/      mod.rs · statusbar.rs · nodes.rs · dive.rs · radar.rs · bigtext.rs · text.rs
  fx/      mod.rs (engine, tuning per level) · boot.rs · decrypt.rs · glitch.rs · noise.rs
```

## Config

`~/.config/ghostwire/config.toml` (or `$XDG_CONFIG_HOME/ghostwire/config.toml`).
`ghostwire --init-config` writes commented examples of it and `keys.toml`, skipping
whichever already exists.

```toml
[sector]
name = "SECTOR-4"
lat = 0.0            # your coordinates — required for ATMOS, SEISMIC radius, SKYTRAFFIC
lon = 0.0
radius_km = 300      # range for nearby quakes
flight_radius_km = 150  # SKYTRAFFIC range; keeps OpenSky at 1 credit per call
units = "metric"     # or "imperial"

[zaibatsu]
stocks = ["NVDA", "TSM", "MSFT"]   # needs a Finnhub key in keys.toml
coins  = ["bitcoin", "ethereum"]   # CoinGecko ids

[intercepts]
rss = []

[fx]
level = "active"     # calm | active | chaotic
dive_every = "45s"   # old name breach_every still accepted
dive_hold  = "15s"
```

API keys live in `keys.toml` beside the config, never in `config.toml`, so the config
can be shared or committed to dotfiles safely:

```toml
# ~/.config/ghostwire/keys.toml — created owner-only (chmod 600)
finnhub = "..."      # overridden by FINNHUB_API_KEY
coingecko = "..."    # optional; overridden by COINGECKO_API_KEY
```

The ticker shows `KEYS EXPOSED` if the file is readable by other users. Keys are held
in a `Secret` type whose `Debug` output is redacted, are sent only as request headers,
and never reach the log or the reading cache.

## Milestones

1. **Skeleton** — terminal setup/restore, event loop, the six nodes in the grid, config
   loading, file logging, `--demo`.
2. **First feeds end-to-end** — Open-Meteo and USGS, with the full link-status model
   (live / decay / ICE / trace / flatline / offline) and the disk cache.
3. **Remaining feeds** — CoinGecko, Finnhub, HN + KEV, NOAA SWPC, OpenSky.
4. **Diegetic layer** — jack-in boot, decrypt/glitch effects, dive cycle, priority
   intercepts, wording pass.
5. **Polish** — CPU tuning, small-terminal fallback, 256-color fallback,
   `cargo install`.

## Running

```bash
cargo run -- --demo          # construct mode: all six nodes on simulated feeds
cargo run -- --init-config   # write ~/.config/ghostwire/config.toml, then set lat/lon
cargo run                    # live feeds
cargo test                   # unit + fixture + render tests
cargo test live -- --ignored --nocapture   # hits the real APIs and prints the screen
```

Keys: see [Keys](#keys). Logs go to
`~/Library/Caches/ghostwire/ghostwire.log` (level via `GHOSTWIRE_LOG`); cached readings
live beside it in `readings/`.

## Status

- [x] M1 Skeleton — grid (3×2 at ≥120 cols, 2×3 below), status bar with uplink /
  neural load (the rig's own CPU via `getrusage`) / clock, footer ticker, construct mode
  for all eight sources, render tests on ratatui's `TestBackend`. Idle redraw is once a
  second, aligned to the clock; ~2% CPU in a debug build.
- [x] M2 First feeds — Open-Meteo (forecast + air quality, which degrades gracefully if
  it fails alone) and USGS (nearby past 7 days + global M4.5+ past day, merged). Full
  link model including GHOST from the disk cache and signal-decay fading. Parsers tested
  against real fixtures in `tests/fixtures/`. Nodes whose sources aren't built yet show
  `NO UPLINK // node not wired yet` outside construct mode.
- [x] M3 Remaining feeds — CoinGecko (7-day sparkline, config order), Finnhub (key sent
  as a header; 401/403 → OFFLINE; 1-minute polls only during NYSE hours, DST-aware;
  sparkline built from successive quotes and seeded from the cache), Hacker News
  (Firebase ranking + 10 stories), CISA KEV (newest 8, hourly), NOAA SWPC (Kp series
  required; 1-min Kp, X-ray, scales optional), OpenSky (own radius to stay in the
  1-credit tier; honors its retry header). All keyless feeds verified live. Finnhub's
  rejected-key path is verified live (dummy key → 401 → OFFLINE); a successful quote is
  tested against its documented response shape only, pending a real key.
  API keys moved to `keys.toml` (see Config).
  **Open:** RSS/Atom for INTERCEPTS isn't wired (the config key is accepted and ignored).
- [x] M4 Diegetic layer — boot log, changed-cells decrypt, glitch bursts, decay static,
  scanlines, chaotic sparkle, the dive cycle with six detail views (radar scopes with a
  sweep for SEISMIC and SKYTRAFFIC, block-font readouts, full-width charts), priority
  intercepts, and key controls. Effects hand-rolled (see Architecture).
- [ ] M5 Polish — 256-color fallback done: colors downsample to the nearest xterm-256
  index whenever `COLORTERM` isn't `truecolor`/`24bit`, applied as a whole-screen pass
  after every other effect so no widget code needs to know about it; a no-op (single
  branch) on the truecolor path. Small-terminal fallback reviewed: the existing 60×14
  floor and 2/3-column grid switch already degrade cleanly (`row()` drops the right-hand
  text rather than overflow, ratatui clips rather than panicking); added boundary tests
  at the floor, one row/column under it, and a dive at the floor for all five nodes.
  CPU reviewed: the frame-budget/sleep and per-cell-hash snapshot design from M4 already
  keeps idle draws at 1 fps, and the new downsample pass costs nothing on the common
  truecolor path, so no changes were needed there. **Open:** `cargo install` packaging
  (Cargo.toml metadata, LICENSE, README).
- [x] Layout — merged SEISMIC and HELIOS into one node: both are low-density feeds (a
  short quake list, a single Kp reading), and the grid was giving HELIOS a full cell for
  four lines of content. `NodeId::Seismic` now carries both `SourceId::Quakes` and
  `SourceId::Swpc`, the same multi-source pattern ZAIBATSU and INTERCEPTS already used.
  Grid tile: up to 4 quake lines, then a one-line space-weather summary (Kp, a 72h trend
  spark, G/S/R). Dive: quakes get the radar scope and full list on top (they need the
  room more), space weather gets a fixed-height strip underneath with its full detail
  (big Kp digit, gauge, 72h chart, X-ray, scales) — sized so the big digit doesn't get
  clipped. The grid now has one open cell (5 nodes in a 3×2/2×3 layout) for whatever
  comes next.
