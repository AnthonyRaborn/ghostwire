mod nodes;
mod statusbar;
pub mod text;

use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

use crate::app::App;
use crate::source::NodeId;
use crate::{lexicon, theme};

const MIN_W: u16 = 60;
const MIN_H: u16 = 14;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::new().style(Style::new().bg(theme::BG).fg(theme::TEXT)),
        area,
    );
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
    for (node, cell) in NodeId::ALL.into_iter().zip(grid_cells(grid)) {
        nodes::draw(frame, cell, app, node, now);
    }
    statusbar::draw_ticker(frame, bottom, app);
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
        terminal.draw(|frame| draw(frame, app)).unwrap();
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
            ] {
                assert!(screen.contains(title), "{title} missing at {w}x{h}");
            }
            assert!(screen.contains("uplink 8/8"));
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

    /// Hits the real Open-Meteo and USGS APIs: `cargo test live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_feeds_render() {
        use crate::feeds::Feed;
        use crate::feeds::open_meteo::OpenMeteo;
        use crate::feeds::usgs::Usgs;

        let config =
            Config::parse("[sector]\nname = \"TEST\"\nlat = 37.77\nlon = -122.42").unwrap();
        let http = reqwest::Client::builder()
            .user_agent("ghostwire-test")
            .build()
            .unwrap();
        let weather = OpenMeteo::new(&config).fetch(&http).await;
        let quakes = Usgs::new(&config).fetch(&http).await;
        let mut app = app_with(vec![
            (SourceId::Weather, weather),
            (SourceId::Quakes, quakes),
        ]);
        app.config = config;
        let screen = render(&app, 132, 30);
        println!("{screen}");
        assert_eq!(app.uplink(), (2, 2), "{screen}");
    }

    #[test]
    fn tiny_terminal_says_so() {
        let screen = render(&demo_app(), 40, 10);
        assert!(screen.contains("TOO SMALL"));
    }
}
