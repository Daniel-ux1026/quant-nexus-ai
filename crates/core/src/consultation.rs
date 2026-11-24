use serde::Serialize;
use serde_json::Value;
use time::{Date, Month, OffsetDateTime};

use crate::{
    Decimal,
    market::{Candle, MINUTE_MS},
    technical::{TechnicalFrame, summarize_candles},
};

const DAY: u64 = 24 * 60 * MINUTE_MS;
const WEEK: u64 = 7 * DAY;
const MONDAY: u64 = 4 * DAY;
const MONTH: u64 = 31 * DAY;
pub const PAGE_CANDLES: usize = 500;
const MAX_BASE_CANDLES: usize = 4_500;
const MIN_CANDLES: usize = 21;
const TARGET_CANDLES: usize = 205;
const FIXED_INTERVALS: &[(&str, u64)] = &[
    ("3d", 3 * DAY),
    ("1d", DAY),
    ("12h", DAY / 2),
    ("8h", DAY / 3),
    ("6h", DAY / 4),
    ("4h", DAY / 6),
    ("2h", DAY / 12),
    ("1h", DAY / 24),
    ("30m", 30 * MINUTE_MS),
    ("15m", 15 * MINUTE_MS),
    ("5m", 5 * MINUTE_MS),
    ("3m", 3 * MINUTE_MS),
    ("1m", MINUTE_MS),
    ("1s", 1_000),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interval {
    Fixed { ms: u64, offset: u64 },
    Months(u64),
}

impl Interval {
    fn max_ms(self) -> u64 {
        match self {
            Self::Fixed { ms, .. } => ms,
            Self::Months(count) => count * MONTH,
        }
    }

    fn is_boundary(self, timestamp: u64) -> bool {
        match self {
            Self::Fixed { ms, offset } => timestamp % ms == offset % ms,
            Self::Months(count) => {
                if !timestamp.is_multiple_of(DAY) {
                    return false;
                }
                OffsetDateTime::from_unix_timestamp((timestamp / 1_000) as i64)
                    .ok()
                    .is_some_and(|date| {
                        let month = i64::from(date.year() - 1970) * 12
                            + i64::from(u8::from(date.month()) - 1);
                        date.day() == 1 && month.rem_euclid(count as i64) == 0
                    })
            }
        }
    }

    fn valid_bar(self, candle: &Candle) -> bool {
        let Some(end) = candle.close_time_ms.checked_add(1) else {
            return false;
        };
        let Some(duration) = end.checked_sub(candle.open_time_ms) else {
            return false;
        };
        let good_duration = match self {
            Self::Fixed { ms, .. } => duration == ms,
            Self::Months(count) => (count * 28 * DAY..=count * MONTH).contains(&duration),
        };
        good_duration
            && self.is_boundary(candle.open_time_ms)
            && self.is_boundary(end)
            && candle.has_valid_prices()
    }
}

#[derive(Debug)]
pub struct Frame {
    pub label: String,
    pub base_code: &'static str,
    base: Interval,
    output: Interval,
    factor: usize,
}

impl Frame {
    pub fn parse(input: &str, spot: bool) -> Result<Self, &'static str> {
        let input = input.trim();
        if input.len() > 40 {
            return Err("La temporalidad es demasiado larga.");
        }
        let split = input
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(input.len());
        let count: u64 = input[..split]
            .parse()
            .ok()
            .filter(|count| (1..=1_000_000).contains(count))
            .ok_or("Escribe una temporalidad positiva, por ejemplo 15m, 4h o 1 semana.")?;
        let unit = input[split..].trim();
        let normalized = unit.to_lowercase();
        let fixed = |ms, offset| Interval::Fixed { ms, offset };
        let (alias, output) = match (unit, normalized.as_str()) {
            ("M", _) | (_, "mes" | "meses" | "mo") => ("M", Interval::Months(count)),
            (_, "s" | "seg" | "segundo" | "segundos") => ("s", fixed(count * 1_000, 0)),
            (_, "m" | "min" | "minuto" | "minutos") => ("m", fixed(count * MINUTE_MS, 0)),
            (_, "h" | "hora" | "horas") => ("h", fixed(count * DAY / 24, 0)),
            (_, "d" | "dia" | "días" | "día" | "dias") => ("d", fixed(count * DAY, 0)),
            (_, "w" | "sem" | "semana" | "semanas") => ("w", fixed(count * WEEK, MONDAY)),
            _ => return Err("Usa segundos, minutos, horas, días, semanas o meses. Ejemplo: 45m."),
        };
        let (base_code, base, factor) = match output {
            Interval::Months(count) => ("1M", Interval::Months(1), count),
            Interval::Fixed { ms, offset: MONDAY } => ("1w", fixed(WEEK, MONDAY), ms / WEEK),
            Interval::Fixed { ms, .. } => {
                let (code, span) = FIXED_INTERVALS.iter().copied()
                    .find(|(code, span)| (spot || *code != "1s") && ms.is_multiple_of(*span))
                    .ok_or("El mercado de futuros ofrece velas desde 1 minuto. Los intervalos en segundos requieren Spot.")?;
                // Binance anchors three-day candles to 1970-01-02 UTC.
                let offset = if code == "3d" { DAY } else { 0 };
                (code, fixed(span, offset), ms / span)
            }
        };
        let factor = factor as usize;
        if factor > MAX_BASE_CANDLES / (MIN_CANDLES + 1) {
            return Err(
                "Esta temporalidad necesita demasiadas velas base. Prueba un intervalo cercano.",
            );
        }
        let output = if base_code == "3d" {
            fixed(output.max_ms(), DAY)
        } else {
            output
        };
        Ok(Self {
            label: format!("{count}{alias}"),
            base_code,
            base,
            output,
            factor,
        })
    }

    pub fn base_candles_needed(&self) -> usize {
        ((TARGET_CANDLES + 1) * self.factor + 1).min(MAX_BASE_CANDLES)
    }

    pub fn max_duration_ms(&self) -> u64 {
        self.output.max_ms()
    }

    pub fn next_close_ms(&self, previous: u64) -> Option<u64> {
        match self.output {
            Interval::Fixed { ms, .. } => previous.checked_add(ms),
            Interval::Months(count) => {
                let open = OffsetDateTime::from_unix_timestamp(
                    (previous.checked_add(1)? / 1_000).try_into().ok()?,
                )
                .ok()?;
                let month = i64::from(open.year()) * 12
                    + i64::from(u8::from(open.month()) - 1)
                    + count as i64;
                let next = Date::from_calendar_date(
                    month.div_euclid(12).try_into().ok()?,
                    Month::try_from(month.rem_euclid(12) as u8 + 1).ok()?,
                    1,
                )
                .ok()?
                .midnight()
                .assume_utc()
                .unix_timestamp();
                u64::try_from(next).ok()?.checked_mul(1_000)?.checked_sub(1)
            }
        }
    }

    pub fn decode_page(&self, body: &[u8], cutoff: u64) -> Result<Vec<Candle>, &'static str> {
        let payload: Vec<Vec<Value>> = serde_json::from_slice(body)
            .map_err(|_| "Binance devolvió un formato de velas inválido.")?;
        if payload.len() > PAGE_CANDLES {
            return Err("Binance devolvió demasiadas velas en una sola página.");
        }
        let mut candles = Vec::with_capacity(payload.len());
        for row in payload {
            let bar =
                Candle::from_binance(&row).map_err(|_| "Binance devolvió una vela incompleta.")?;
            if !self.base.valid_bar(&bar) {
                return Err("Binance devolvió una vela fuera del intervalo solicitado.");
            }
            if bar.close_time_ms < cutoff {
                candles.push(bar);
            }
        }
        Ok(candles)
    }

    pub fn complete(&self, base: &[Candle], clock: u64) -> Result<Vec<Candle>, &'static str> {
        if base
            .windows(2)
            .any(|pair| pair[0].close_time_ms.checked_add(1) != Some(pair[1].open_time_ms))
        {
            return Err("La serie de Binance tiene huecos o velas duplicadas. Vuelve a consultar.");
        }
        let start = base
            .iter()
            .position(|bar| self.output.is_boundary(bar.open_time_ms))
            .unwrap_or(base.len());
        let mut candles = Vec::new();
        for group in base[start..].chunks_exact(self.factor) {
            let first = &group[0];
            let last = &group[group.len() - 1];
            let bar = Candle {
                open_time_ms: first.open_time_ms,
                close_time_ms: last.close_time_ms,
                open: first.open,
                high: group.iter().map(|candle| candle.high).max().unwrap(),
                low: group.iter().map(|candle| candle.low).min().unwrap(),
                close: last.close,
                base_volume: group
                    .iter()
                    .try_fold(Decimal::ZERO, |sum, candle| {
                        sum.checked_add(candle.base_volume)
                    })
                    .ok_or("El volumen agregado supera el rango admitido.")?,
            };
            if !self.output.valid_bar(&bar) || bar.close_time_ms >= clock {
                return Err(
                    "Las velas agregadas no coinciden con la temporalidad o la hora del mercado.",
                );
            }
            candles.push(bar);
        }
        if candles.len() < MIN_CANDLES {
            return Err(
                "Se necesitan al menos 21 velas cerradas en esa temporalidad. Prueba una más corta o un par con más historial.",
            );
        }
        if clock - candles.last().unwrap().close_time_ms
            > self.max_duration_ms() + self.base.max_ms()
        {
            return Err("La última vela de Binance quedó desactualizada para esta temporalidad.");
        }
        if candles.len() > TARGET_CANDLES {
            candles.drain(..candles.len() - TARGET_CANDLES);
        }
        Ok(candles)
    }

    pub fn indicators(&self, candles: &[Candle]) -> Option<TechnicalFrame> {
        summarize_candles(candles, self.max_duration_ms() / MINUTE_MS)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Levels {
    pub entry: Decimal,
    pub invalidation: Decimal,
    pub target: Decimal,
}

pub fn breakout(candles: &[Candle], long: bool) -> Option<Levels> {
    let recent = candles.get(candles.len().checked_sub(20)?..)?;
    let ranges = recent[5..]
        .windows(2)
        .try_fold(Decimal::ZERO, |sum, pair| {
            let range = pair[1]
                .high
                .max(pair[0].close)
                .checked_sub(pair[1].low.min(pair[0].close))?;
            sum.checked_add(range)
        })?;
    let average = ranges.checked_div(Decimal::from(14))?;
    let entry = if long {
        recent.iter().map(|bar| bar.high).max()?
    } else {
        recent.iter().map(|bar| bar.low).min()?
    };
    let decimals = (entry.normalize().scale() + 4).clamp(4, 24);
    let distance = average.checked_mul(Decimal::new(15, 1))?.round_dp(decimals);
    if distance <= Decimal::ZERO {
        return None;
    }
    let change = if long { distance } else { -distance };
    let invalidation = entry.checked_sub(change)?;
    let target = entry.checked_add(change.checked_mul(Decimal::TWO)?)?;
    (invalidation > Decimal::ZERO && target > Decimal::ZERO).then_some(Levels {
        entry,
        invalidation,
        target,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_custom_intervals_and_keeps_months_distinct_from_minutes() {
        assert_eq!(Frame::parse("45 minutos", true).unwrap().base_code, "15m");
        assert_eq!(Frame::parse("7m", true).unwrap().factor, 7);
        let three_days = Frame::parse("3d", true).unwrap();
        assert!(three_days.output.is_boundary(DAY));
        assert!(!three_days.output.is_boundary(0));
        assert_eq!(Frame::parse("2 meses", true).unwrap().base_code, "1M");
        assert_eq!(Frame::parse("1M", false).unwrap().base_code, "1M");
        assert_eq!(Frame::parse("1m", false).unwrap().base_code, "1m");
        assert!(Frame::parse("45s", false).is_err());
        assert!(Frame::parse("0h", true).is_err());
        assert!(Frame::parse("1000000 semanas", true).is_err());
    }

    #[test]
    fn aggregates_only_closed_aligned_bars_and_preserves_price_levels() {
        let frame = Frame::parse("7m", true).unwrap();
        let bars: Vec<_> = (1..156)
            .map(|minute| Candle {
                open_time_ms: minute * MINUTE_MS,
                close_time_ms: (minute + 1) * MINUTE_MS - 1,
                open: Decimal::from(100),
                high: Decimal::from(102),
                low: Decimal::from(98),
                close: Decimal::from(101),
                base_volume: Decimal::ONE,
            })
            .collect();
        let clock = 156 * MINUTE_MS;
        let candles = frame.complete(&bars, clock).unwrap();
        assert_eq!(candles.len(), 21);
        assert_eq!(candles[0].open_time_ms, 7 * MINUTE_MS);
        assert_eq!(candles[0].base_volume, Decimal::from(7));
        let long = breakout(&candles, true).unwrap();
        assert_eq!(
            (long.entry, long.invalidation, long.target),
            (102.into(), 96.into(), 114.into())
        );
        let short = breakout(&candles, false).unwrap();
        assert_eq!(
            (short.entry, short.invalidation, short.target),
            (98.into(), 104.into(), 86.into())
        );
        assert!(frame.complete(&bars[0..15], clock).is_err());
        let mut missing = bars.clone();
        missing.remove(20);
        assert!(frame.complete(&missing, clock).is_err());
    }
}
