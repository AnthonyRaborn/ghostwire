# GHOSTWIRE

A terminal dashboard for live public data — stocks, crypto, weather, quakes, space
weather, air traffic, internet outages, Hacker News, and known-exploited CVEs — styled
as a netrunner rig you've jacked into. Leave it running on a spare monitor: a refresh
decrypts the node, stale data decays, a failed request is ICE, a rate limit is a trace.

```
 GHOSTWIRE // ZERO-DAY // RIG-07 ░ uplink 6/6 ░ neural load 3% ░ 21:14:07 NET
┌ ZAIBATSU INDEX ────────┐┌ ATMOS//SECTOR-4 ───────┐┌ INTERCEPTS ────────────┐
│ NVDA   182.40 ▲2.1% ▅▆▇││ 17°C  RAIN 20%  WIND 9 ││ ▓ HN  Show HN: a tiny… │
│ BTC    61,204 ▼0.8% ▇▆▅││ SMOG AQI 42 ░░▒  UV 3  ││ ▓ KEV CVE-2026-41822   │
└────────────────────────┘└────────────────────────┘└────────────────────────┘
┌ SEISMIC // HELIOS ─────┐┌ SKYTRAFFIC ────────────┐┌ NETSTATUS ─────────────┐
│ M2.1  38km NE   4m ago ││ 7 contacts overhead    ││ UPLINK 14ms  SJC       │
│ Kp 3 ▂▃▃▅  G0 S0 R0    ││ UAL1234  FL340  ↗ 452kt││ US NOMINAL             │
└────────────────────────┘└────────────────────────┘└────────────────────────┘
 » diving SKYTRAFFIC in 12s ░ NOAA-SWPC: ICE, retry 30s ░ 2 ghosts cached
```

Every 45 seconds the rig takes over the screen with a full-detail dive into one node —
a radar scope, a bar chart, a block-font readout — then surfaces back to the grid.

## Install

```bash
cargo install ghostwire
```

Or build from source:

```bash
git clone https://github.com/AnthonyRaborn/ghostwire.git
cd ghostwire
cargo install --path .
```

## Try it without any setup

```bash
ghostwire --demo
```

Runs entirely on synthetic data — no network, no config, no API keys. Good for a first
look, or for leaving on a spare monitor without spending API quota.

## Running for real

```bash
ghostwire --init-config   # writes ~/.config/ghostwire/config.toml with commented examples
```

Set `[sector] lat`/`lon` in that file (needed for weather, nearby quakes, and air
traffic), then:

```bash
ghostwire
```

Everything else is keyless. Stock quotes need a free [Finnhub](https://finnhub.io) key
in `keys.toml`, next to the config file; every other feed works with no key at all.

## Controls

| Key | Grid | Dive |
|---|---|---|
| `1`–`6` | dive into that node | switch to that node |
| `space` | dive into the next node now | surface |
| `p` | hold the dive cycle | hold / release |
| `esc` | jack out | surface |
| `r` | re-breach: every source fetches now | same |
| `q` / `ctrl-c` | jack out | same |

## More

[GHOSTWIRE.md](GHOSTWIRE.md) has the full picture: every node and its data source,
the config file format, FX levels, and the architecture behind it.

## License

[MIT](LICENSE)
