use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::market::{Candle, MINUTE_MS, validate_series};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TechnicalFrame {
    pub minutes: u64,
    pub candles: usize,
    pub close: f64,
    pub rsi14: Option<f64>,
    pub ema9: Option<f64>,
    pub ema20: Option<f64>,
    pub ema50: Option<f64>,
    pub ema100: Option<f64>,
    pub ema200: Option<f64>,
    pub sma20: Option<f64>,
    pub atr14: Option<f64>,
    pub macd: Option<f64>,
    pub macd_signal: Option<f64>,
    pub bollinger_low: Option<f64>,
    pub bollinger_high: Option<f64>,
    pub sample_vwap: Option<f64>,
    pub obv: f64,
    pub prior_support: Option<f64>,
    pub prior_resistance: Option<f64>,
    pub range_position: &'static str,
}

#[derive(Clone, Copy)]
struct Bar {
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

pub fn technical_frames(candles: &[Candle]) -> Vec<TechnicalFrame> {
    if validate_series(candles).is_err() {
        return Vec::new();
    }
    [1, 5, 15]
        .into_iter()
        .filter_map(|minutes| {
            let series = aggregate(candles, minutes);
            summarize(&series, minutes)
        })
        .collect()
}

pub(crate) fn summarize_candles(candles: &[Candle], minutes: u64) -> Option<TechnicalFrame> {
    let bars = candles
        .iter()
        .map(|bar| {
            Some(Bar {
                high: bar.high.to_f64()?,
                low: bar.low.to_f64()?,
                close: bar.close.to_f64()?,
                volume: bar.base_volume.to_f64()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    summarize(&bars, minutes)
}

fn aggregate(source: &[Candle], minutes: u64) -> Vec<Bar> {
    let Some(first) = source.first() else {
        return Vec::new();
    };
    let span = minutes * MINUTE_MS;
    let start_offset = ((span - first.open_time_ms % span) % span / MINUTE_MS) as usize;
    source
        .get(start_offset..)
        .unwrap_or_default()
        .chunks_exact(minutes as usize)
        .filter_map(|group| {
            let last = group.last()?;
            Some(Bar {
                high: group.iter().map(|bar| bar.high).max()?.to_f64()?,
                low: group.iter().map(|bar| bar.low).min()?.to_f64()?,
                close: last.close.to_f64()?,
                volume: group
                    .iter()
                    .filter_map(|bar| bar.base_volume.to_f64())
                    .sum::<f64>(),
            })
        })
        .collect()
}

fn summarize(bars: &[Bar], minutes: u64) -> Option<TechnicalFrame> {
    let last = bars.last()?;
    let closes: Vec<_> = bars.iter().map(|bar| bar.close).collect();
    let volume: f64 = bars.iter().map(|bar| bar.volume).sum();
    let weighted: f64 = bars
        .iter()
        .map(|bar| (bar.high + bar.low + bar.close) / 3.0 * bar.volume)
        .sum();
    let sma20 = mean_tail(&closes, 20);
    let deviation = sma20.map(|mean| {
        let squares: f64 = closes[closes.len() - 20..]
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum();
        (squares / 20.0).sqrt()
    });
    let fast = ema(&closes, 12);
    let slow = ema(&closes, 26);
    let difference: Vec<_> = fast
        .into_iter()
        .skip(14)
        .zip(slow)
        .map(|(left, right)| left - right)
        .collect();
    let prior = bars
        .get(bars.len().saturating_sub(21)..bars.len() - 1)
        .filter(|prior| prior.len() == 20);
    let support = prior.map(|bars| bars.iter().map(|bar| bar.low).fold(f64::INFINITY, f64::min));
    let resistance = prior.map(|bars| {
        bars.iter()
            .map(|bar| bar.high)
            .fold(f64::NEG_INFINITY, f64::max)
    });
    let true_ranges: Vec<_> = bars
        .windows(2)
        .map(|pair| pair[1].high.max(pair[0].close) - pair[1].low.min(pair[0].close))
        .collect();
    let obv: f64 = bars
        .windows(2)
        .map(|pair| {
            if pair[1].close > pair[0].close {
                pair[1].volume
            } else if pair[1].close < pair[0].close {
                -pair[1].volume
            } else {
                0.0
            }
        })
        .sum();

    Some(TechnicalFrame {
        minutes,
        candles: bars.len(),
        close: last.close,
        rsi14: rsi(&closes),
        ema9: ema(&closes, 9).last().copied(),
        ema20: ema(&closes, 20).last().copied(),
        ema50: ema(&closes, 50).last().copied(),
        ema100: ema(&closes, 100).last().copied(),
        ema200: ema(&closes, 200).last().copied(),
        sma20,
        atr14: wilder(&true_ranges, 14),
        macd: difference.last().copied(),
        macd_signal: ema(&difference, 9).last().copied(),
        bollinger_low: sma20.zip(deviation).map(|(mean, std)| mean - 2.0 * std),
        bollinger_high: sma20.zip(deviation).map(|(mean, std)| mean + 2.0 * std),
        sample_vwap: (volume > 0.0).then_some(weighted / volume),
        obv,
        prior_support: support,
        prior_resistance: resistance,
        range_position: match (support, resistance) {
            (_, Some(high)) if last.close > high => "above",
            (Some(low), _) if last.close < low => "below",
            (Some(_), Some(_)) => "inside",
            _ => "unknown",
        },
    })
}

fn mean_tail(values: &[f64], period: usize) -> Option<f64> {
    let start = values.len().checked_sub(period)?;
    Some(values[start..].iter().sum::<f64>() / period as f64)
}

fn ema(values: &[f64], period: usize) -> Vec<f64> {
    let Some(seed) = values.get(..period) else {
        return Vec::new();
    };
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut current = seed.iter().sum::<f64>() / period as f64;
    let mut result = Vec::with_capacity(values.len() - period + 1);
    result.push(current);
    for value in &values[period..] {
        current += (value - current) * alpha;
        result.push(current);
    }
    result
}

fn wilder(values: &[f64], period: usize) -> Option<f64> {
    let seed = values.get(..period)?.iter().sum::<f64>() / period as f64;
    Some(values[period..].iter().fold(seed, |previous, value| {
        (previous * (period as f64 - 1.0) + value) / period as f64
    }))
}

fn rsi(values: &[f64]) -> Option<f64> {
    let (up, down): (Vec<_>, Vec<_>) = values
        .windows(2)
        .map(|pair| {
            let delta = pair[1] - pair[0];
            (delta.max(0.0), (-delta).max(0.0))
        })
        .unzip();
    let gains = wilder(&up, 14)?;
    let losses = wilder(&down, 14)?;
    Some(if gains == 0.0 && losses == 0.0 {
        50.0
    } else {
        100.0 * gains / (gains + losses)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::synthetic_snapshot;

    #[test]
    fn uses_closed_utc_buckets_and_reports_unavailable_warmups() {
        let sample = synthetic_snapshot().unwrap();
        let frames = technical_frames(&sample.candles);
        assert_eq!(frames[0].candles, 32);
        assert!(frames[0].rsi14.is_some());
        assert!(frames[0].ema200.is_none());
        assert!(frames[1].candles < frames[0].candles);
        assert!(frames[1].rsi14.is_none());
        let mut invalid = sample.candles;
        invalid.remove(1);
        assert!(technical_frames(&invalid).is_empty());
    }
}
