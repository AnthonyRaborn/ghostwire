mod bigtext;
mod dive;
mod globe;
mod nodes;
mod radar;
mod statusbar;
pub mod text;

use std::time::Instant;

use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

use crate::app::App;
use crate::fx::Fx;
use crate::source::NodeId;
use crate::{lexicon, theme};

const MIN_W: u16 = 60;
const MIN_H: u16 = 14;

pub fn draw(frame: &mut Frame, app: &App, fx: &mut Fx) {
    let area = frame.area();
    let instant = Instant::now();
    frame.render_widget(
        Block::new().style(Style::new().bg(theme::BG).fg(theme::TEXT)),
        area,
    );
    if fx.booting(instant) {
        fx.draw_boot(frame, instant);
        fx.screen(frame.buffer_mut(), area);
        return;
    }
    if area.width < MIN_W || area.height < MIN_H {
        let [_, middle, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(area);
        let msg = Line::styled(
            lexicon::too_small(MIN_W, MIN_H),
            Style::new().fg(theme::YELLOW),
        );
        frame.render_widget(Paragraph::new(msg).centered(), middle);
        return;
    }

    let [top, grid, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    let now = Utc::now();
    statusbar::draw_top(frame, top, app);
    match app.dive.diving() {
        Some(node) => dive::draw(frame, grid, app, node, now, instant, fx),
        None => {
            for (node, cell) in NodeId::ALL.into_iter().zip(grid_cells(grid)) {
                nodes::draw(frame, cell, app, node, now, instant, fx);
            }
        }
    }
    statusbar::draw_ticker(frame, bottom, app);
    fx.screen(frame.buffer_mut(), area);
}

/// Whether the screen shows a radar, whose sweep needs a steady frame rate.
pub fn radar_on_screen(app: &App) -> bool {
    matches!(app.dive.diving(), Some(NodeId::Seismic | NodeId::Sky))
}

/// Three across when there's room (or when the screen is short); two across otherwise.
fn grid_cells(area: Rect) -> Vec<Rect> {
    let cols: u32 = if area.width >= 120 || area.height < 20 {
        3
    } else {
        2
    };
    let rows = (NodeId::ALL.len() as u32).div_ceil(cols);
    Layout::vertical(vec![Constraint::Ratio(1, rows); rows as usize])
        .split(area)
        .iter()
        .flat_map(|r| {
            Layout::horizontal(vec![Constraint::Ratio(1, cols); cols as usize])
                .split(*r)
                .to_vec()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use tokio::sync::broadcast;

    use super::*;
    use crate::config::Config;
    use crate::event::Msg;
    use crate::feeds::demo::DemoFeed;
    use crate::feeds::{FeedMsg, FetchError};
    use crate::source::SourceId;

    fn app_with(results: Vec<(SourceId, Result<crate::reading::Reading, FetchError>)>) -> App {
        let started = results
            .iter()
            .map(|(id, _)| (*id, Duration::from_secs(60)))
            .collect();
        let mut app = App::new(
            Config::default(),
            true,
            true,
            started,
            broadcast::channel(1).0,
            None,
        );
        for (source, result) in results {
            app.handle(Msg::Feed(FeedMsg::Done {
                source,
                result,
                failures: 1,
                interval: Duration::from_secs(60),
                retry_in: Some(Duration::from_secs(30)),
            }));
        }
        app
    }

    fn demo_app() -> App {
        let config = Config::default();
        let mut rng = fastrand::Rng::with_seed(42);
        let results = SourceId::ALL
            .into_iter()
            .map(|id| (id, Ok(DemoFeed::new(id, &config).evolve(None, &mut rng))))
            .collect();
        app_with(results)
    }

    fn render(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        // Calm has no boot, and no effect runs without a signal, so frames are stable.
        let mut fx = Fx::new(
            crate::config::FxLevel::Calm,
            Vec::new(),
            Instant::now(),
            crate::colordepth::Depth::TrueColor,
        );
        terminal.draw(|frame| draw(frame, app, &mut fx)).unwrap();
        let buf = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_every_node_wide_and_narrow() {
        let app = demo_app();
        for (w, h) in [(132, 30), (80, 36)] {
            let screen = render(&app, w, h);
            println!("{screen}\n");
            for title in [
                "ZAIBATSU INDEX",
                "ATMOS // SECTOR-0",
                "INTERCEPTS",
                "SEISMIC",
                "HELIOS",
                "SKYTRAFFIC",
                "NETSTATUS",
            ] {
                assert!(screen.contains(title), "{title} missing at {w}x{h}");
            }
            assert!(screen.contains("uplink 12/12"));
            assert!(screen.contains("all links nominal"));
        }
    }

    #[test]
    fn troubled_sources_show_in_node_and_ticker() {
        let app = app_with(vec![
            (SourceId::Weather, Err(FetchError::Failed("reset".into()))),
            (
                SourceId::Stocks,
                Err(FetchError::NotConfigured("FINNHUB_API_KEY not set".into())),
            ),
            (SourceId::Swpc, Err(FetchError::RateLimited(None))),
        ]);
        let screen = render(&app, 132, 30);
        println!("{screen}");
        assert!(screen.contains("OPEN-METEO !! ICE // retry 30s"));
        assert!(screen.contains("FINNHUB: OFFLINE — FINNHUB_API_KEY not set"));
        assert!(screen.contains("NOAA-SWPC: TRACE ACTIVE, dark 30s"));
        assert!(screen.contains("NO UPLINK"));
    }

    /// Hits every real API once (one OpenSky credit):
    /// `cargo test live -- --ignored --nocapture`. Stocks stay OFFLINE unless
    /// a Finnhub key is in keys.toml or FINNHUB_API_KEY.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_feeds_render() {
        use crate::feeds::Feed;
        use crate::feeds::{coingecko, finnhub, hn, kev, open_meteo, opensky, orbit, swpc, usgs};
        use crate::keys::{self, Keys};
        use crate::source::Link;

        let config =
            Config::parse("[sector]\nname = \"TEST\"\nlat = 37.77\nlon = -122.42").unwrap();
        let http = reqwest::Client::builder()
            .user_agent("ghostwire-test")
            .build()
            .unwrap();
        // Real keys from the usual place, if any, so a configured rig tests its stocks too.
        let keys_path = keys::path_beside(&crate::paths::config_path().unwrap());
        let (keys, _) = Keys::load(&keys_path).unwrap();
        let has_finnhub = keys.finnhub().is_some();
        let (stocks, crypto, weather, sky, quakes, orbit) = (
            finnhub::Finnhub::new(&config, keys.finnhub(), None),
            coingecko::CoinGecko::new(&config, keys.coingecko()),
            open_meteo::OpenMeteo::new(&config),
            opensky::OpenSky::new(&config),
            usgs::Usgs::new(&config),
            orbit::Orbit::new(&config),
        );
        let (stocks, crypto, weather, news, vulns, quakes, space, sky, iss) = tokio::join!(
            stocks.fetch(&http),
            crypto.fetch(&http),
            weather.fetch(&http),
            hn::HackerNews.fetch(&http),
            kev::Kev.fetch(&http),
            quakes.fetch(&http),
            swpc::Swpc.fetch(&http),
            sky.fetch(&http),
            orbit.fetch(&http),
        );
        let mut app = app_with(vec![
            (SourceId::Stocks, stocks),
            (SourceId::Crypto, crypto),
            (SourceId::Weather, weather),
            (SourceId::Hn, news),
            (SourceId::Kev, vulns),
            (SourceId::Quakes, quakes),
            (SourceId::Swpc, space),
            (SourceId::OpenSky, sky),
            (SourceId::Orbit, iss),
        ]);
        app.config = config;
        let screen = render(&app, 132, 34);
        println!("{screen}");
        for (id, state) in &app.sources {
            let expected_offline = *id == SourceId::Stocks && !has_finnhub;
            if expected_offline {
                assert!(
                    matches!(state.link, Link::Offline(_)),
                    "{id:?}: {:?}",
                    state.link
                );
            } else {
                assert_eq!(state.link, Link::Live, "{id:?}: {:?}", state.last_error);
            }
        }
    }

    #[test]
    fn every_node_dives() {
        let expected = [
            (NodeId::Zaibatsu, vec!["CRYPTO // 7 DAYS"]),
            (NodeId::Atmos, vec!["NEXT 24H //", "PRECIP NOWCAST //"]),
            (
                NodeId::Intercepts,
                vec!["HACKER NEWS // FRONT PAGE", "RSS // INTERCEPTED FEEDS"],
            ),
            (
                NodeId::Seismic,
                vec!["EVENTS // NEARBY 7 DAYS", "Kp // LAST 72H"],
            ),
            (NodeId::Sky, vec!["CONTACTS //"]),
            (NodeId::Netstatus, vec!["IODA // COUNTRY OUTAGES"]),
        ];
        for (node, markers) in expected {
            let mut app = demo_app();
            app.dive.dive_now(node, Instant::now());
            let screen = render(&app, 132, 34);
            println!("{screen}\n");
            assert!(screen.contains("◢ DIVE //"), "{node:?}");
            for marker in markers {
                assert!(screen.contains(marker), "{node:?} missing {marker:?}");
            }
            assert!(screen.contains("[esc] surface"), "{node:?}");
        }
    }

    #[test]
    fn ticker_announces_the_next_dive() {
        let screen = render(&demo_app(), 132, 30);
        assert!(
            screen.contains("» diving ZAIBATSU INDEX in 30s"),
            "{screen}"
        );
        assert!(screen.contains("1·ZAIBATSU INDEX"));
    }

    #[test]
    fn tiny_terminal_says_so() {
        let screen = render(&demo_app(), 40, 10);
        assert!(screen.contains("TOO SMALL"));
    }

    #[test]
    fn minimum_size_renders_the_grid_not_the_fallback() {
        let screen = render(&demo_app(), MIN_W, MIN_H);
        assert!(!screen.contains("TOO SMALL"), "{screen}");
        assert!(screen.contains("ZAIBATSU"), "{screen}");
    }

    #[test]
    fn one_row_short_falls_back() {
        let screen = render(&demo_app(), MIN_W, MIN_H - 1);
        assert!(screen.contains("TOO SMALL"), "{screen}");
    }

    #[test]
    fn one_column_short_falls_back() {
        let screen = render(&demo_app(), MIN_W - 1, MIN_H);
        assert!(screen.contains("TOO SMALL"), "{screen}");
    }

    #[test]
    fn dive_renders_at_minimum_size_without_panicking() {
        for node in NodeId::ALL {
            let mut app = demo_app();
            app.dive.dive_now(node, Instant::now());
            let screen = render(&app, MIN_W, MIN_H);
            assert!(screen.contains("◢ DIVE //"), "{node:?}: {screen}");
        }
    }

    #[test]
    fn orbit_stays_on_screen_in_a_busy_airspace() {
        use crate::reading::{Contact, Satellite};

        let contacts = (0..20)
            .map(|i| Contact {
                callsign: format!("TST{i}"),
                altitude_m: Some(5_000.0),
                speed_ms: Some(200.0),
                heading: Some(90.0),
                distance_km: f64::from(i),
                bearing: 90.0,
            })
            .collect();
        let satellite = vec![Satellite {
            name: "ISS".into(),
            altitude_km: 417.0,
            velocity_kmh: 27_600.0,
            sunlit: true,
            distance_km: 100.0,
            bearing: 45.0,
            // Below the intercept threshold, so this doesn't also land in the ticker
            // as a priority-intercept banner and give a false pass.
            elevation_deg: 5.0,
        }];
        let app = app_with(vec![
            (
                SourceId::OpenSky,
                Ok(crate::reading::Reading::OpenSky(contacts)),
            ),
            (
                SourceId::Orbit,
                Ok(crate::reading::Reading::Orbit(satellite)),
            ),
        ]);
        let screen = render(&app, 132, 30);
        assert!(!screen.contains("PRIORITY INTERCEPT"), "{screen}");
        assert!(screen.contains("ISS"), "{screen}");
    }

    #[test]
    fn rss_stays_on_screen_alongside_a_full_kev_and_hn() {
        use crate::reading::{Headline, Story, Vuln};

        let vulns = (0..8)
            .map(|i| Vuln {
                cve: format!("CVE-2026-{i}"),
                vendor: "Acme".into(),
                product: "Router".into(),
                name: String::new(),
                added: Utc::now().date_naive(),
                ransomware: false,
            })
            .collect();
        let stories = (0..10)
            .map(|i| Story {
                id: i,
                title: format!("Story {i}"),
                score: 100,
                comments: 10,
                posted: Utc::now(),
            })
            .collect();
        let headlines = vec![Headline {
            title: "Canary headline".into(),
            source: "Test Feed".into(),
            link: None,
            published: Some(Utc::now()),
        }];
        let app = app_with(vec![
            (SourceId::Kev, Ok(crate::reading::Reading::Kev(vulns))),
            (SourceId::Hn, Ok(crate::reading::Reading::Hn(stories))),
            (SourceId::Rss, Ok(crate::reading::Reading::Rss(headlines))),
        ]);
        let screen = render(&app, 132, 30);
        assert!(screen.contains("Canary headline"), "{screen}");
    }

    #[test]
    fn atmos_drops_the_nowcast_radar_when_narrow() {
        let mut app = demo_app();
        app.dive.dive_now(NodeId::Atmos, Instant::now());
        let screen = render(&app, MIN_W, 34);
        assert!(screen.contains("NEXT 24H //"), "{screen}");
        assert!(!screen.contains("PRECIP NOWCAST"), "{screen}");
    }
}
