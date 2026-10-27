# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- RSS/Atom feeds take per-feed settings: an entry in `[intercepts] rss` can be a table
  with `url`, `name` (shown instead of the feed's own title), and `max` (the most
  headlines that feed may show, so a busy feed can't crowd out the rest). Bare URLs
  still work as before.

### Fixed

- Re-breach (`r`) no longer cuts a rate limit short, presses that land while a fetch is
  running are answered by that fetch, and presses within 10 seconds of the last one are
  ignored, so mashing `r` can't burn through OpenSky's or Finnhub's quota.
- Ghosts are skipped when the config they were fetched under has changed (another
  sector, radius, ticker list, or feed list) or when they're over 24 hours old, so a
  restart no longer shows another sector's weather or aircraft as this one's.
- Neural load reads the rig's real CPU use on Windows instead of a constant 0%.

### Changed

- Sources in ICE or FLATLINED show why their last fetch failed, in the footer ticker
  and in the node, instead of only in the log.

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
