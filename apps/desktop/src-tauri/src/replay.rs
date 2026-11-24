use std::path::Path;

use quant_nexus_core::Decimal;
use quant_nexus_core::backtest::{BacktestReport, backtest};
use quant_nexus_core::market::{
    BinanceSpot, Candle, MINUTE_MS, MarketSource, SpotPair, SpotVenue, synthetic_snapshot,
};
use quant_nexus_core::research::{ResearchReport, walk_forward};
use quant_nexus_core::scanner::{ScanObservation, parse_capital, replay};
use serde::{Deserialize, Serialize};

use crate::{download, scanner::unix_ms, storage};

const MAX_REPLAY_MS: u64 = 360 * MINUTE_MS;

#[derive(Deserialize)]
#[serde(
    tag = "source",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ReplayRequest {
    Synthetic,
    Archive {
        pair: String,
        #[serde(default)]
        venue: SpotVenue,
        from_ms: u64,
        before_ms: u64,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReport {
    pair: String,
    source: MarketSource,
    candles: Vec<Candle>,
    observations: Vec<ScanObservation>,
    backtest: BacktestReport,
    research: Option<ResearchReport>,
}

pub async fn run(
    path: &Path,
    capital: &str,
    request: ReplayRequest,
) -> Result<ReplayReport, String> {
    let capital = parse_capital(capital).map_err(|error| error.to_string())?;
    let (pair, source, candles) = match request {
        ReplayRequest::Synthetic => {
            let snapshot = synthetic_snapshot().map_err(|error| error.to_string())?;
            (snapshot.pair, snapshot.source, snapshot.candles)
        }
        ReplayRequest::Archive {
            pair,
            venue,
            from_ms,
            before_ms,
        } => {
            let pair = pair
                .parse::<SpotPair>()
                .map_err(|error| error.to_string())?
                .to_string();
            let span = validate_range(from_ms, before_ms, unix_ms())?;
            let pool = storage::open(path)
                .await
                .map_err(|_| "No se pudo abrir el historial local.")?;
            let rows = storage::read_range(&pool, venue, &pair, from_ms, before_ms)
                .await
                .map_err(|_| "No se pudo leer el tramo elegido del historial.")?;
            if rows.len() as u64 != span / MINUTE_MS {
                return Err("El tramo tiene velas faltantes. Elige un intervalo cubierto por el historial local.".into());
            }
            (pair, venue.source(), rows)
        }
    };
    make_report(pair, source, candles, capital)
}

pub async fn import_binance(
    path: &Path,
    capital: &str,
    pair: &str,
    from_ms: u64,
    before_ms: u64,
) -> Result<ReplayReport, String> {
    let pair = pair
        .parse::<SpotPair>()
        .map_err(|error| error.to_string())?;
    let capital = parse_capital(capital).map_err(|error| error.to_string())?;
    let candles = fetch_binance_range(&pair, from_ms, before_ms).await?;
    let name = pair.to_string();
    let pool = storage::open(path)
        .await
        .map_err(|_| "No se pudo abrir el historial local.")?;
    storage::write_candles(&pool, SpotVenue::Binance, &name, &candles)
        .await
        .map_err(|_| "Las velas históricas no se guardaron en SQLite.")?;
    make_report(name, MarketSource::BinancePublic, candles, capital)
}

async fn fetch_binance_range(
    pair: &SpotPair,
    from_ms: u64,
    before_ms: u64,
) -> Result<Vec<Candle>, String> {
    let as_of = BinanceSpot::parse_server_time(&download(BinanceSpot::SERVER_TIME_URL).await?)
        .map_err(|error| error.to_string())?;
    let span = validate_range(from_ms, before_ms, as_of)?;
    let mut cursor = from_ms;
    let mut candles = Vec::with_capacity((span / MINUTE_MS) as usize);
    while cursor < before_ms {
        let body = download(&BinanceSpot::archive_url(pair, cursor, before_ms - 1)).await?;
        let page = BinanceSpot::decode_archive(&body, as_of).map_err(|error| error.to_string())?;
        if page.first().is_none_or(|bar| bar.open_time_ms != cursor)
            || page.last().is_none_or(|bar| bar.close_time_ms >= before_ms)
        {
            return Err("Binance no entregó un tramo continuo de velas cerradas.".into());
        }
        cursor = page
            .last()
            .ok_or("Binance devolvió una página vacía.")?
            .close_time_ms
            + 1;
        candles.extend(page);
    }
    Ok(candles)
}

fn make_report(
    pair: String,
    source: MarketSource,
    candles: Vec<Candle>,
    capital: Decimal,
) -> Result<ReplayReport, String> {
    let observations = replay(&candles, capital).map_err(|error| error.to_string())?;
    let backtest =
        backtest(&pair, capital, &candles, &observations).map_err(|error| error.to_string())?;
    let research = walk_forward(&candles);
    Ok(ReplayReport {
        pair,
        source,
        candles,
        observations,
        backtest,
        research,
    })
}

fn validate_range(from_ms: u64, before_ms: u64, now_ms: u64) -> Result<u64, String> {
    before_ms
        .checked_sub(from_ms)
        .filter(|span| {
            (MINUTE_MS..=MAX_REPLAY_MS).contains(span)
                && from_ms.is_multiple_of(MINUTE_MS)
                && before_ms.is_multiple_of(MINUTE_MS)
                && before_ms <= now_ms
        })
        .ok_or_else(|| "Selecciona un tramo cerrado entre 1 minuto y 6 horas, en UTC.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_range_requires_a_bounded_closed_minute_interval() {
        assert_eq!(
            validate_range(MINUTE_MS, 2 * MINUTE_MS, 2 * MINUTE_MS).unwrap(),
            MINUTE_MS
        );
        assert!(validate_range(MINUTE_MS + 1, 2 * MINUTE_MS, 3 * MINUTE_MS).is_err());
        assert!(validate_range(MINUTE_MS, 2 * MINUTE_MS, 2 * MINUTE_MS - 1).is_err());
        assert!(validate_range(MINUTE_MS, MAX_REPLAY_MS + 2 * MINUTE_MS, u64::MAX).is_err());
    }

    #[test]
    #[ignore = "Consulta un tramo histórico de seis horas de Binance Spot"]
    fn public_research_history_smoke() {
        tauri::async_runtime::block_on(async {
            let before = (unix_ms() / MINUTE_MS) * MINUTE_MS - 2 * MINUTE_MS;
            let pair: SpotPair = "BTC/USDT".parse().unwrap();
            let candles = fetch_binance_range(&pair, before - MAX_REPLAY_MS, before)
                .await
                .unwrap();
            assert_eq!(candles.len(), 360);
            let result = make_report(
                pair.to_string(),
                MarketSource::BinancePublic,
                candles,
                Decimal::from(1000),
            )
            .unwrap();
            let research = result.research.unwrap();
            assert_eq!(research.folds.len(), 3);
            assert_eq!(result.backtest.equity_curve.len(), 360);
            println!(
                "Muestras de prueba: {}. Brier: {:.6}. Referencia: {:.6}. PnL simulado: {}.",
                research.test_samples,
                research.brier,
                research.baseline_brier,
                result.backtest.net_pnl
            );
        });
    }
}
