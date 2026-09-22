# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
