use rust_decimal::Decimal;
use serde::Serialize;
use thiserror::Error;

use crate::market::{Candle, MarketError, validate_series};
use crate::risk::{PositionScenario, RiskError, estimate_automated_spot_position};

pub const SCANNER_LOOKBACK: usize = 21;
const FAST_WINDOW: usize = 9;
const VOLUME_WINDOW: usize = 20;
const ATR_WINDOW: usize = 14;
const VOLUME_THRESHOLD: Decimal = Decimal::from_parts(125, 0, 0, false, 2);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ScanError {
    #[error("Ingresa un capital simulado en USDT mayor que cero.")]
    InvalidCapital,
    #[error("El historial no contiene velas.")]
    Empty,
    #[error(transparent)]
    Market(#[from] MarketError),
    #[error("Los datos exceden la precisión de los indicadores.")]
    Arithmetic,
}

pub fn parse_capital(input: &str) -> Result<Decimal, ScanError> {
    let amount = Decimal::from_str_exact(input.trim()).map_err(|_| ScanError::InvalidCapital)?;
    if amount <= Decimal::ZERO {
        return Err(ScanError::InvalidCapital);
    }
    Ok(amount)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Regime {
    Unknown,
    Bullish,
    Bearish,
    Sideways,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationReason {
    WarmingUp,
    NoUptrend,
    NoBreakout,
    LowVolume,
    FlatRange,
    RiskRejected,
    BreakoutWithVolume,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketEvidence {
    pub sma_fast: Decimal,
    pub sma_slow: Decimal,
    pub atr: Decimal,
    pub relative_volume: Option<Decimal>,
    pub prior_high: Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanObservation {
    pub close_time_ms: u64,
    pub close: Decimal,
    pub regime: Regime,
    pub reason: ObservationReason,
    pub evidence: Option<MarketEvidence>,
    pub scenario: Option<PositionScenario>,
    pub risk_rejection: Option<String>,
}

pub fn evaluate(candles: &[Candle], equity: Decimal) -> Result<ScanObservation, ScanError> {
    if equity <= Decimal::ZERO {
        return Err(ScanError::InvalidCapital);
    }
    let window = &candles[candles.len().saturating_sub(SCANNER_LOOKBACK)..];
    let latest = window.last().ok_or(ScanError::Empty)?;
    validate_series(window)?;

    let mut result = ScanObservation {
        close_time_ms: latest.close_time_ms,
        close: latest.close,
        regime: Regime::Unknown,
        reason: ObservationReason::WarmingUp,
        evidence: None,
        scenario: None,
        risk_rejection: None,
    };
    if window.len() < SCANNER_LOOKBACK {
        return Ok(result);
    }

    let fast = average(
        window[SCANNER_LOOKBACK - FAST_WINDOW..]
            .iter()
            .map(|bar| bar.close),
        FAST_WINDOW,
    )?;
    let slow = average(window.iter().map(|bar| bar.close), SCANNER_LOOKBACK)?;
    let atr = average(
        window[SCANNER_LOOKBACK - ATR_WINDOW - 1..]
            .windows(2)
            .map(|bars| {
                let previous = &bars[0];
                let current = &bars[1];
                (current.high.max(previous.close)) - (current.low.min(previous.close))
            }),
        ATR_WINDOW,
    )?;
    let prior_volume = average(
        window[..VOLUME_WINDOW].iter().map(|bar| bar.base_volume),
        VOLUME_WINDOW,
    )?;
    let relative_volume = if prior_volume > Decimal::ZERO {
        Some(
            latest
                .base_volume
                .checked_div(prior_volume)
                .ok_or(ScanError::Arithmetic)?,
        )
    } else {
        None
    };
    let prior_high = window[SCANNER_LOOKBACK - 2].high;

    result.evidence = Some(MarketEvidence {
        sma_fast: fast,
        sma_slow: slow,
        atr,
        relative_volume,
        prior_high,
    });
    result.regime = match (fast.cmp(&slow), latest.close.cmp(&slow)) {
        (std::cmp::Ordering::Greater, std::cmp::Ordering::Greater) => Regime::Bullish,
        (std::cmp::Ordering::Less, std::cmp::Ordering::Less) => Regime::Bearish,
        _ => Regime::Sideways,
    };
    result.reason = if result.regime != Regime::Bullish {
        ObservationReason::NoUptrend
    } else if latest.close <= prior_high {
        ObservationReason::NoBreakout
    } else if relative_volume.is_none_or(|ratio| ratio < VOLUME_THRESHOLD) {
        ObservationReason::LowVolume
    } else if atr <= Decimal::ZERO {
        ObservationReason::FlatRange
    } else {
        let stop_distance = atr
            .checked_mul(Decimal::new(15, 1))
            .ok_or(ScanError::Arithmetic)?;
        let stop = latest
            .close
            .checked_sub(stop_distance)
            .ok_or(ScanError::Arithmetic)?;
        match estimate_automated_spot_position(equity, latest.close, stop) {
            Ok(scenario) => {
                result.scenario = Some(scenario);
                ObservationReason::BreakoutWithVolume
            }
            Err(error) => {
                result.risk_rejection = Some(risk_text(error));
                ObservationReason::RiskRejected
            }
        }
    };
    Ok(result)
}

fn risk_text(error: RiskError) -> String {
    match error {
        RiskError::InvalidPrices => "Los datos no permiten calcular un stop positivo.".into(),
        other => other.to_string(),
    }
}

fn average(mut values: impl Iterator<Item = Decimal>, count: usize) -> Result<Decimal, ScanError> {
    values
        .try_fold(Decimal::ZERO, Decimal::checked_add)
        .and_then(|sum| sum.checked_div(Decimal::from(count)))
        .ok_or(ScanError::Arithmetic)
}

pub fn replay(candles: &[Candle], equity: Decimal) -> Result<Vec<ScanObservation>, ScanError> {
    validate_series(candles)?;
    (1..=candles.len())
        .map(|end| evaluate(&candles[..end], equity))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::{MINUTE_MS, synthetic_snapshot};

    #[test]
    fn replay_matches_stepwise_analysis_and_rejects_gaps() {
        let fixture = synthetic_snapshot().unwrap();
        let capital = Decimal::from(1000);
        let observations = replay(&fixture.candles, capital).unwrap();
        assert_eq!(replay(&fixture.candles, capital).unwrap(), observations);
        assert_eq!(observations[0].reason, ObservationReason::WarmingUp);
        assert_eq!(
            observations.last().unwrap(),
            &evaluate(&fixture.candles, capital).unwrap()
        );
        let mut with_gap = fixture.candles.clone();
        with_gap.remove(8);
        assert_eq!(
            replay(&with_gap, capital),
            Err(ScanError::Market(MarketError::InvalidSeries))
        );
    }

    #[test]
    fn derives_simulated_prices_and_sizing_from_only_capital() {
        let mut window = Vec::new();
        for index in 0..SCANNER_LOOKBACK {
            let price = Decimal::from(100 + index);
            let open_time_ms = (index + 1) as u64 * MINUTE_MS;
            window.push(Candle {
                open_time_ms,
                close_time_ms: open_time_ms + MINUTE_MS - 1,
                open: price,
                high: price + Decimal::new(9, 1),
                low: price - Decimal::ONE,
                close: price + Decimal::new(5, 1),
                base_volume: if index == SCANNER_LOOKBACK - 1 {
                    Decimal::from(20)
                } else {
                    Decimal::from(10)
                },
            });
        }
        let capital = parse_capital("1000.00").unwrap();
        let observation = evaluate(&window, capital).unwrap();
        assert_eq!(observation.reason, ObservationReason::BreakoutWithVolume);
        let scenario = observation.scenario.unwrap();
        assert!(scenario.stop < scenario.entry && scenario.target > scenario.entry);
        assert!(scenario.estimate.entry_cost <= capital);
        assert!(scenario.estimate.estimated_loss <= Decimal::new(250, 2));
        assert!(scenario.estimate.reward_to_risk >= Decimal::from(2));
        assert_eq!(
            evaluate(&window, Decimal::new(1, 28)).unwrap().reason,
            ObservationReason::RiskRejected
        );
        assert_eq!(parse_capital("0"), Err(ScanError::InvalidCapital));
    }
}
