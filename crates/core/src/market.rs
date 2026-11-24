use std::fmt;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const MINUTE_MS: u64 = 60_000;
const MAX_CANDLES: usize = 100;
const MAX_PAYLOAD_BYTES: usize = 65_536;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MarketError {
    #[error("Usa un par Spot USDT, por ejemplo BTC/USDT.")]
    InvalidPair,
    #[error("La respuesta de mercado no coincide con el formato de velas esperado.")]
    InvalidPayload,
    #[error("La fuente devolvió velas inválidas o con huecos.")]
    InvalidSeries,
    #[error("La fuente no devolvió suficientes velas cerradas y recientes.")]
    StaleOrMissing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotPair {
    base: String,
}

impl FromStr for SpotPair {
    type Err = MarketError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let raw = raw.trim();
        if raw.len() > 17 || !raw.is_ascii() {
            return Err(MarketError::InvalidPair);
        }
        let raw = raw.to_ascii_uppercase();
        let base = raw
            .strip_suffix("/USDT")
            .or_else(|| raw.strip_suffix("USDT"))
            .ok_or(MarketError::InvalidPair)?;
        if !(2..=12).contains(&base.len())
            || base == "USDT"
            || !base.bytes().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(MarketError::InvalidPair);
        }
        Ok(Self { base: base.into() })
    }
}

impl fmt::Display for SpotPair {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{}/USDT", self.base)
    }
}

impl SpotPair {
    pub fn stream_name(&self) -> String {
        format!("{}usdt@kline_1m", self.base.to_ascii_lowercase())
    }

    pub fn exchange_symbol(&self) -> String {
        format!("{}USDT", self.base)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpotVenue {
    #[default]
    Binance,
    Bitget,
    Bitunix,
}

impl SpotVenue {
    pub fn id(self) -> &'static str {
        match self {
            Self::Binance => "binance",
            Self::Bitget => "bitget",
            Self::Bitunix => "bitunix",
        }
    }

    pub fn source(self) -> MarketSource {
        match self {
            Self::Binance => MarketSource::BinancePublic,
            Self::Bitget => MarketSource::BitgetPublic,
            Self::Bitunix => MarketSource::BitunixPublic,
        }
    }

    pub fn candles_url(self, pair: &SpotPair) -> String {
        let symbol = pair.exchange_symbol();
        match self {
            Self::Binance => BinanceSpot::candles_url(pair),
            Self::Bitget => format!(
                "https://api.bitget.com/api/v3/market/candles?category=SPOT&symbol={symbol}&interval=1m&limit={MAX_CANDLES}"
            ),
            Self::Bitunix => format!(
                "https://openapi.bitunix.com/api/spot/v1/market/kline/history?symbol={symbol}&interval=1&limit={MAX_CANDLES}"
            ),
        }
    }

    pub fn decode(
        self,
        pair: SpotPair,
        body: &[u8],
        as_of_ms: u64,
    ) -> Result<MarketSnapshot, MarketError> {
        if self == Self::Binance {
            return BinanceSpot::decode(pair, body, as_of_ms);
        }
        if body.len() > MAX_PAYLOAD_BYTES || as_of_ms > i64::MAX as u64 {
            return Err(MarketError::InvalidPayload);
        }
        let wrapper: Value =
            serde_json::from_slice(body).map_err(|_| MarketError::InvalidPayload)?;
        let data = wrapper["data"]
            .as_array()
            .ok_or(MarketError::InvalidPayload)?;
        if data.len() > MAX_CANDLES {
            return Err(MarketError::InvalidPayload);
        }

        let mut candles = match self {
            Self::Bitget if wrapper["code"] == "00000" => data
                .iter()
                .map(decode_bitget)
                .collect::<Result<Vec<_>, _>>()?,
            Self::Bitunix
                if wrapper["success"] == true
                    && (wrapper["code"] == "0" || wrapper["code"] == 0) =>
            {
                let symbol = pair.exchange_symbol();
                data.iter()
                    .map(|row| decode_bitunix(row, &symbol))
                    .collect::<Result<Vec<_>, _>>()?
            }
            _ => return Err(MarketError::InvalidPayload),
        };
        candles.sort_unstable_by_key(|bar| bar.open_time_ms);
        if candles
            .windows(2)
            .any(|pair| pair[0].open_time_ms == pair[1].open_time_ms)
        {
            return Err(MarketError::InvalidSeries);
        }
        candles.retain(|bar| bar.close_time_ms < as_of_ms);
        if self == Self::Bitunix {
            let count = candles.len();
            let end = candles
                .iter()
                .rposition(|bar| bar.validate().is_ok())
                .ok_or(MarketError::InvalidSeries)?;
            let mut start = end;
            while start > 0
                && candles[start - 1].validate().is_ok()
                && candles[start - 1].close_time_ms.checked_add(1)
                    == Some(candles[start].open_time_ms)
            {
                start -= 1;
            }
            let recent = candles.drain(start..=end).collect::<Vec<_>>();
            let mut market = snapshot(pair, self.source(), as_of_ms, recent)?;
            market.omitted = count - market.candles.len();
            return Ok(market);
        }
        validate_series(&candles)?;
        snapshot(pair, self.source(), as_of_ms, candles)
    }
}

fn decode_bitget(value: &Value) -> Result<Candle, MarketError> {
    let row = value
        .as_array()
        .filter(|row| row.len() == 7)
        .ok_or(MarketError::InvalidPayload)?;
    let open_time_ms = row[0]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(MarketError::InvalidPayload)?;
    Ok(Candle {
        open_time_ms,
        close_time_ms: open_time_ms
            .checked_add(MINUTE_MS - 1)
            .ok_or(MarketError::InvalidPayload)?,
        open: read_decimal(&row[1])?,
        high: read_decimal(&row[2])?,
        low: read_decimal(&row[3])?,
        close: read_decimal(&row[4])?,
        base_volume: read_decimal(&row[5])?,
    })
}

fn decode_bitunix(row: &Value, expected_symbol: &str) -> Result<Candle, MarketError> {
    if row["symbol"] != expected_symbol {
        return Err(MarketError::InvalidPayload);
    }
    let timestamp = row["ts"].as_str().ok_or(MarketError::InvalidPayload)?;
    let seconds =
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
            .map_err(|_| MarketError::InvalidPayload)?;
    let open_time_ms = u64::try_from(seconds.unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| MarketError::InvalidPayload)?;
    Ok(Candle {
        open_time_ms,
        close_time_ms: open_time_ms
            .checked_add(MINUTE_MS - 1)
            .ok_or(MarketError::InvalidPayload)?,
        open: read_decimal(&row["open"])?,
        high: read_decimal(&row["high"])?,
        low: read_decimal(&row["low"])?,
        close: read_decimal(&row["close"])?,
        base_volume: read_decimal(&row["volume"])?,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketSource {
    BinancePublic,
    BitgetPublic,
    BitunixPublic,
    SyntheticFixture,
}

impl MarketSource {
    pub fn venue(self) -> Option<SpotVenue> {
        match self {
            Self::BinancePublic => Some(SpotVenue::Binance),
            Self::BitgetPublic => Some(SpotVenue::Bitget),
            Self::BitunixPublic => Some(SpotVenue::Bitunix),
            Self::SyntheticFixture => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candle {
    pub open_time_ms: u64,
    pub close_time_ms: u64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub base_volume: Decimal,
}

impl Candle {
    pub(crate) fn from_binance(row: &[Value]) -> Result<Self, MarketError> {
        if row.len() != 12 {
            return Err(MarketError::InvalidPayload);
        }

        let decimal = |index: usize| read_decimal(&row[index]);
        let time = |index: usize| row[index].as_u64().ok_or(MarketError::InvalidPayload);
        Ok(Self {
            open_time_ms: time(0)?,
            open: decimal(1)?,
            high: decimal(2)?,
            low: decimal(3)?,
            close: decimal(4)?,
            base_volume: decimal(5)?,
            close_time_ms: time(6)?,
        })
    }

    pub fn validate(&self) -> Result<(), MarketError> {
        if !self.open_time_ms.is_multiple_of(MINUTE_MS)
            || self.open_time_ms.checked_add(MINUTE_MS - 1) != Some(self.close_time_ms)
            || self.close_time_ms > i64::MAX as u64
            || !self.has_valid_prices()
        {
            return Err(MarketError::InvalidSeries);
        }

        Ok(())
    }

    pub(crate) fn has_valid_prices(&self) -> bool {
        self.low > Decimal::ZERO
            && self.low <= self.open.min(self.close)
            && self.high >= self.open.max(self.close)
            && self.base_volume >= Decimal::ZERO
    }
}

fn read_decimal(value: &Value) -> Result<Decimal, MarketError> {
    value
        .as_str()
        .and_then(|value| Decimal::from_str_exact(value).ok())
        .ok_or(MarketError::InvalidPayload)
}

pub fn validate_series(candles: &[Candle]) -> Result<(), MarketError> {
    for candle in candles {
        candle.validate()?;
    }
    if candles
        .windows(2)
        .any(|window| window[0].close_time_ms.checked_add(1) != Some(window[1].open_time_ms))
    {
        return Err(MarketError::InvalidSeries);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketSnapshot {
    pub pair: String,
    pub source: MarketSource,
    pub interval: &'static str,
    pub as_of_ms: u64,
    pub candles: Vec<Candle>,
    pub omitted: usize,
}

pub struct BinanceSpot;

impl BinanceSpot {
    pub const SERVER_TIME_URL: &str = "https://data-api.binance.vision/api/v3/time";

    pub fn candles_url(pair: &SpotPair) -> String {
        format!(
            "https://data-api.binance.vision/api/v3/klines?symbol={}USDT&interval=1m&limit={MAX_CANDLES}",
            pair.base
        )
    }

    pub fn archive_url(pair: &SpotPair, start_ms: u64, end_ms: u64) -> String {
        format!(
            "{}&startTime={start_ms}&endTime={end_ms}",
            Self::candles_url(pair)
        )
    }

    pub fn decode_stream(body: &[u8]) -> Result<Option<(SpotPair, Candle)>, MarketError> {
        if body.len() > MAX_PAYLOAD_BYTES {
            return Err(MarketError::InvalidPayload);
        }
        let wrapper: Value =
            serde_json::from_slice(body).map_err(|_| MarketError::InvalidPayload)?;
        let event = &wrapper["data"];
        let kline = &event["k"];
        let symbol = event["s"].as_str().ok_or(MarketError::InvalidPayload)?;
        let pair: SpotPair = symbol.parse()?;
        if event["e"] != "kline"
            || kline["i"] != "1m"
            || kline["s"] != symbol
            || wrapper["stream"].as_str() != Some(pair.stream_name().as_str())
        {
            return Err(MarketError::InvalidPayload);
        }
        if !kline["x"].as_bool().ok_or(MarketError::InvalidPayload)? {
            return Ok(None);
        }
        let time = |field: &str| kline[field].as_u64().ok_or(MarketError::InvalidPayload);
        let candle = Candle {
            open_time_ms: time("t")?,
            close_time_ms: time("T")?,
            open: read_decimal(&kline["o"])?,
            high: read_decimal(&kline["h"])?,
            low: read_decimal(&kline["l"])?,
            close: read_decimal(&kline["c"])?,
            base_volume: read_decimal(&kline["v"])?,
        };
        candle.validate()?;
        if event["E"]
            .as_u64()
            .is_none_or(|time| time < candle.close_time_ms)
        {
            return Err(MarketError::InvalidPayload);
        }
        Ok(Some((pair, candle)))
    }

    pub fn parse_server_time(body: &[u8]) -> Result<u64, MarketError> {
        if body.len() > MAX_PAYLOAD_BYTES {
            return Err(MarketError::InvalidPayload);
        }
        let value: Value = serde_json::from_slice(body).map_err(|_| MarketError::InvalidPayload)?;
        value["serverTime"]
            .as_u64()
            .filter(|time| *time >= MINUTE_MS && *time <= i64::MAX as u64)
            .ok_or(MarketError::InvalidPayload)
    }

    pub fn decode(
        pair: SpotPair,
        body: &[u8],
        server_time_ms: u64,
    ) -> Result<MarketSnapshot, MarketError> {
        Self::decode_with_source(pair, body, server_time_ms, MarketSource::BinancePublic)
    }

    fn decode_with_source(
        pair: SpotPair,
        body: &[u8],
        as_of_ms: u64,
        source: MarketSource,
    ) -> Result<MarketSnapshot, MarketError> {
        let candles = Self::decode_archive(body, as_of_ms)?;
        snapshot(pair, source, as_of_ms, candles)
    }

    pub fn decode_archive(body: &[u8], as_of_ms: u64) -> Result<Vec<Candle>, MarketError> {
        if body.len() > MAX_PAYLOAD_BYTES || as_of_ms > i64::MAX as u64 {
            return Err(MarketError::InvalidPayload);
        }
        let rows: Vec<Vec<Value>> =
            serde_json::from_slice(body).map_err(|_| MarketError::InvalidPayload)?;
        if rows.len() > MAX_CANDLES {
            return Err(MarketError::InvalidPayload);
        }
        let candles = rows
            .iter()
            .map(|row| Candle::from_binance(row))
            .collect::<Result<Vec<_>, _>>()?;
        validate_series(&candles)?;
        Ok(candles
            .into_iter()
            .take_while(|candle| candle.close_time_ms < as_of_ms)
            .collect())
    }
}

fn snapshot(
    pair: SpotPair,
    source: MarketSource,
    as_of_ms: u64,
    candles: Vec<Candle>,
) -> Result<MarketSnapshot, MarketError> {
    let last = candles.last().ok_or(MarketError::StaleOrMissing)?;
    if candles.len() < 2 || as_of_ms.saturating_sub(last.close_time_ms) > 2 * MINUTE_MS {
        return Err(MarketError::StaleOrMissing);
    }
    Ok(MarketSnapshot {
        pair: pair.to_string(),
        source,
        interval: "1m",
        as_of_ms,
        candles,
        omitted: 0,
    })
}

pub fn synthetic_snapshot() -> Result<MarketSnapshot, MarketError> {
    BinanceSpot::decode_with_source(
        "BTC/USDT".parse()?,
        include_bytes!("../fixtures/synthetic-binance-spot.json"),
        1_767_227_941_000,
        MarketSource::SyntheticFixture,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &[u8] = include_bytes!("../fixtures/synthetic-binance-spot.json");

    fn snapshot_at(time_ms: u64) -> Result<MarketSnapshot, MarketError> {
        BinanceSpot::decode("BTCUSDT".parse().unwrap(), BODY, time_ms)
    }

    #[test]
    fn labels_synthetic_payload_and_validates_market_origin() {
        let fixture = synthetic_snapshot().unwrap();
        assert_eq!(fixture.source, MarketSource::SyntheticFixture);
        assert_eq!(fixture.candles.len(), 32);
        let public = snapshot_at(fixture.as_of_ms).unwrap();
        assert_eq!(public.source, MarketSource::BinancePublic);
        assert_eq!(public.candles, fixture.candles);
    }

    #[test]
    fn drops_the_open_candle() {
        let fixture = synthetic_snapshot().unwrap();
        let last = fixture.candles.last().unwrap();
        assert_eq!(snapshot_at(last.close_time_ms).unwrap().candles.len(), 31);
    }

    #[test]
    fn rejects_stale_and_nonconsecutive_candles() {
        let fixture = synthetic_snapshot().unwrap();
        assert_eq!(
            snapshot_at(fixture.as_of_ms + 2 * MINUTE_MS),
            Err(MarketError::StaleOrMissing)
        );
        let mut rows: Vec<Vec<Value>> = serde_json::from_slice(BODY).unwrap();
        rows.remove(1);
        assert_eq!(
            BinanceSpot::decode(
                "BTCUSDT".parse().unwrap(),
                &serde_json::to_vec(&rows).unwrap(),
                fixture.as_of_ms
            ),
            Err(MarketError::InvalidSeries)
        );
    }

    #[test]
    fn rejects_unexpected_values_and_url_fragments() {
        let fixture = synthetic_snapshot().unwrap();
        let mut rows: Vec<Vec<Value>> = serde_json::from_slice(BODY).unwrap();
        rows[0][2] = Value::String("0.000001".into());
        assert_eq!(
            BinanceSpot::decode(
                "BTCUSDT".parse().unwrap(),
                &serde_json::to_vec(&rows).unwrap(),
                fixture.as_of_ms
            ),
            Err(MarketError::InvalidSeries)
        );
        assert_eq!(
            "BTC&limit=1000/USDT".parse::<SpotPair>(),
            Err(MarketError::InvalidPair)
        );
        assert!(BinanceSpot::parse_server_time(br#"{"serverTime":"123"}"#).is_err());
        let pair = "eth/usdt".parse().unwrap();
        assert!(BinanceSpot::candles_url(&pair).contains("symbol=ETHUSDT&"));
    }

    #[test]
    fn stream_admits_only_closed_candles_with_matching_symbol() {
        let mut message = serde_json::json!({
            "stream":"btcusdt@kline_1m",
            "data":{"e":"kline","E":120_000,"s":"BTCUSDT","k":{
                "t":60_000,"T":119_999,"s":"BTCUSDT","i":"1m","x":false,
                "o":"100","h":"102","l":"99","c":"101","v":"31.5"
            }}
        });
        assert_eq!(
            BinanceSpot::decode_stream(message.to_string().as_bytes()).unwrap(),
            None
        );
        message["data"]["k"]["x"] = true.into();
        let (pair, candle) = BinanceSpot::decode_stream(message.to_string().as_bytes())
            .unwrap()
            .unwrap();
        assert_eq!(pair.to_string(), "BTC/USDT");
        assert_eq!(candle.close, Decimal::from(101));
        message["stream"] = "ethusdt@kline_1m".into();
        assert!(BinanceSpot::decode_stream(message.to_string().as_bytes()).is_err());
    }

    #[test]
    fn bitunix_excludes_inconsistent_history_without_inventing_prices() {
        let make_bar = |minute, open| {
            serde_json::json!({
                "symbol": "BTCUSDT", "open": open, "high": "105", "low": "99", "close": "101",
                "volume": "2", "ts": format!("2026-11-24T14:{minute:02}:00Z")
            })
        };
        let payload = serde_json::json!({
            "code":"0","success":true,
            "data":[make_bar(32,"101"), make_bar(31,"101"), make_bar(30,"108"), make_bar(29,"101")]
        });
        let as_of = decode_bitunix(&payload["data"][0], "BTCUSDT")
            .unwrap()
            .close_time_ms
            + 1;
        let market = SpotVenue::Bitunix
            .decode(
                "BTCUSDT".parse().unwrap(),
                payload.to_string().as_bytes(),
                as_of,
            )
            .unwrap();
        assert_eq!(market.omitted, 2);
        assert_eq!(market.candles.len(), 2);
        assert_eq!(market.candles[0].open, Decimal::from(101));
    }
}
