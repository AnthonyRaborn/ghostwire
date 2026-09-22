//! ZAIBATSU (crypto): price, 24h change, and a 7-day sparkline from CoinGecko's public
//! API. A free demo key (`coingecko` in keys.toml) is optional and raises the rate limit.

use std::time::Duration;

use serde::Deserialize;

use super::http::{get_text, get_text_keyed, parse_json};
use super::{Feed, FetchError};
use crate::config::Config;
use crate::keys::Secret;
use crate::reading::{Quote, Reading};
use crate::source::SourceId;

pub struct CoinGecko {
    ids: Vec<String>,
    key: Option<Secret>,
}

impl CoinGecko {
    pub fn new(config: &Config, key: Option<Secret>) -> Self {
        let ids = config
            .zaibatsu
            .coins
            .iter()
            .map(|id| id.trim().to_lowercase())
            .filter(|id| {
                let ok =
                    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
                if !ok {
                    tracing::warn!(
                        "ignoring coin id {id:?}: expected a CoinGecko id like \"bitcoin\""
                    );
                }
                ok
            })
            .collect();
        Self { ids, key }
    }
}

impl Feed for CoinGecko {
    fn source(&self) -> SourceId {
        SourceId::Crypto
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(2 * 60)
    }

    async fn fetch(&self, http: &reqwest::Client) -> Result<Reading, FetchError> {
        if self.ids.is_empty() {
            return Err(FetchError::NotConfigured(
                "no valid [zaibatsu] coins".into(),
            ));
        }
        let url = format!(
            "https://api.coingecko.com/api/v3/coins/markets?vs_currency=usd&ids={}\
             &sparkline=true&price_change_percentage=24h",
            self.ids.join(",")
        );
        let text = match &self.key {
            Some(key) => {
                get_text_keyed(http.get(url).header("x-cg-demo-api-key", key.expose())).await?
            }
            None => get_text(http.get(url)).await?,
        };
        let quotes = parse(&text, &self.ids).map_err(FetchError::Failed)?;
        if quotes.is_empty() {
            return Err(FetchError::NotConfigured(
                "no [zaibatsu] coins matched a CoinGecko id".into(),
            ));
        }
        Ok(Reading::Crypto(quotes))
    }
}

#[derive(Deserialize)]
struct Market {
    id: String,
    symbol: String,
    current_price: Option<f64>,
    price_change_percentage_24h: Option<f64>,
    sparkline_in_7d: Option<Sparkline>,
}

#[derive(Deserialize)]
struct Sparkline {
    price: Vec<Option<f64>>,
}

/// Quotes in the order the config lists the coins (CoinGecko sorts by market cap).
pub fn parse(text: &str, order: &[String]) -> Result<Vec<Quote>, String> {
    let markets: Vec<Market> = parse_json(text)?;
    let mut ranked: Vec<(usize, Quote)> = markets
        .into_iter()
        .filter_map(|m| {
            let rank = order
                .iter()
                .position(|id| *id == m.id)
                .unwrap_or(usize::MAX);
            let quote = Quote {
                symbol: m.symbol.to_uppercase(),
                price: m.current_price?,
                change_pct: m.price_change_percentage_24h.unwrap_or(0.0),
                spark: m
                    .sparkline_in_7d
                    .map(|s| s.price.into_iter().flatten().collect())
                    .unwrap_or_default(),
            };
            Some((rank, quote))
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    Ok(ranked.into_iter().map(|(_, q)| q).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKETS: &str = include_str!("../../tests/fixtures/coingecko_markets.json");

    #[test]
    fn parses_markets_in_config_order() {
        let order: Vec<String> = ["solana", "bitcoin", "ethereum"].map(String::from).into();
        let quotes = parse(MARKETS, &order).unwrap();
        let symbols: Vec<&str> = quotes.iter().map(|q| q.symbol.as_str()).collect();
        assert_eq!(symbols, ["SOL", "BTC", "ETH"]);
        let btc = &quotes[1];
        assert_eq!(btc.price, 85_696.0);
        assert!((btc.change_pct - 5.54247).abs() < 1e-9);
        assert_eq!(btc.spark.len(), 168);
    }

    #[test]
    fn empty_list_means_no_known_ids() {
        assert!(parse("[]", &["nope".into()]).unwrap().is_empty());
    }

    #[test]
    fn rejects_unsafe_ids() {
        let config =
            Config::parse("[zaibatsu]\ncoins = [\"bitcoin\", \"a&b=c\", \" Ethereum \"]").unwrap();
        assert_eq!(CoinGecko::new(&config, None).ids, ["bitcoin", "ethereum"]);
    }
}
