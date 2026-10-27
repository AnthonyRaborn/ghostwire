# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `ghostwire --check-config` checks the config and keys and prints what each source
  would do (run, run limited, off, or offline and why), plus anything the rig would
  quietly drop, like a malformed stock symbol, without starting the display or
  touching the network.
- A `?` manual over the screen: the controls, what each link state and signal means,
  and where the config, keys, and log live. The ticker always keeps room for a short
  `[?] help` hint.
- RSS/Atom feeds take per-feed settings: an entry in `[intercepts] rss` can be a table
  with `url`, `name` (shown instead of the feed's own title), and `max` (the most
  headlines that feed may show, so a busy feed can't crowd out the rest). Bare URLs
  still work as before.
- A sun readout under the NETSTATUS terminator map: the sector's light (day, civil,
  nautical, or astronomical twilight, or night), the sun's elevation, and a countdown
  to the next sunrise or sunset.

### Changed

- The terminator map draws in half-block cells for twice the vertical resolution, with
  real coastlines (from Natural Earth) instead of rough boxes, graded twilight bands
  instead of a hard day/night edge, and the point under the sun marked. The sun's
  position now allows for the equation of time (up to ~4° off before) and updates every
  five minutes instead of every hour, and the sector marker covers one cell instead of
  a 2×2 block.
- Sources in ICE or FLATLINED show why their last fetch failed, in the footer ticker
  and in the node, instead of only in the log.
- Sources in the same trouble for the same reason share one ticker entry (e.g.
  `9 LINKS: ICE — Connection refused, retry 5s`), and entries that don't fit drop off
  whole behind a `+N more` count instead of running off the edge mid-word.

### Security

- Updated rustls to 0.23.46, rustls-webpki to 0.103.16, and webpki-root-certs to
  1.0.11 for RUSTSEC-2026-0346, -0347, -0348, -0349, -0382, and -0383 (TLS secrets in
  `Debug` output, a key-exchange downgrade, distrusted intermediates, and more), and
  tokio to 1.54.1 for RUSTSEC-2026-0386. Every feed's HTTPS goes through rustls.

### Fixed

- Re-breach (`r`) no longer cuts a rate limit short, presses that land while a fetch is
  running are answered by that fetch, and presses within 10 seconds of the last one are
  ignored, so mashing `r` can't burn through OpenSky's or Finnhub's quota.
- Ghosts are skipped when the config they were fetched under has changed (another
  sector, radius, ticker list, or feed list) or when they're over 24 hours old, so a
  restart no longer shows another sector's weather or aircraft as this one's.
- Neural load reads the rig's real CPU use on Windows instead of a constant 0%.

## [1.0.0] — ZERO-DAY

First release.

### Nodes

- **ZAIBATSU INDEX** — stock quotes (Finnhub, free key) and crypto prices with a 7-day
  sparkline (CoinGecko).
- **ATMOS // SECTOR** — current conditions, air quality, UV, a 24-hour forecast chart,
  and a precipitation nowcast scope (Open-Meteo).
- **INTERCEPTS** — Hacker News top stories, new CISA KEV entries, and any RSS/Atom
  feeds you configure.
- **SEISMIC // HELIOS** — nearby and major global earthquakes (USGS) alongside space
  weather: Kp, X-ray flux, and NOAA G/S/R scales (SWPC).
- **SKYTRAFFIC** — aircraft overhead (OpenSky Network) and the ISS's position.
- **NETSTATUS** — country-level internet outage alerts (IODA), the rig's own uplink
  latency, and a day/night terminator globe.

### Features

- Hybrid layout: a full grid with a periodic full-screen dive into one node.
- Diegetic effects driven by real feed state — boot log, decrypt on refresh, glitches
  on errors, decay static on stale data — at `calm`, `active`, or `chaotic` levels.
- `--demo` mode on synthetic data, with no network, config, or keys.
- Offline cache ("ghosts") so a restart shows the last good data immediately.
- Config and keys in separate files; keys are created owner-only and can be
  overridden by environment variables.
- 24-bit color with an automatic 256-color fallback.
- Configurable rig name (`[rig] shortname` / `number`).

[Unreleased]: https://github.com/AnthonyRaborn/ghostwire/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/AnthonyRaborn/ghostwire/releases/tag/v1.0.0
