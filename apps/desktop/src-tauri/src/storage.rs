use std::path::Path;
use std::time::Duration;

use quant_nexus_core::Decimal;
use quant_nexus_core::market::{Candle, MarketSnapshot, SpotVenue};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

pub async fn open(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    std::fs::create_dir_all(path).map_err(sqlx::Error::Io)?;
    let options = SqliteConnectOptions::new()
        .filename(path.join("public-market.sqlite"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await?;
    initialize(&pool).await?;
    Ok(pool)
}

pub(crate) async fn initialize(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS public_candle (
           exchange TEXT NOT NULL,
           pair TEXT NOT NULL,
           interval TEXT NOT NULL,
           open_time_ms INTEGER NOT NULL,
           close_time_ms INTEGER NOT NULL,
           open TEXT NOT NULL,
           high TEXT NOT NULL,
           low TEXT NOT NULL,
           close TEXT NOT NULL,
           base_volume TEXT NOT NULL,
           PRIMARY KEY (exchange, pair, interval, open_time_ms)
        ) WITHOUT ROWID",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn save(path: &Path, snapshot: &MarketSnapshot) -> Result<(), sqlx::Error> {
    let venue = snapshot
        .source
        .venue()
        .filter(|_| snapshot.interval == "1m")
        .ok_or_else(|| {
            sqlx::Error::InvalidArgument("Solo se archivan velas públicas de un minuto".into())
        })?;
    let pool = open(path).await?;
    write_candles(&pool, venue, &snapshot.pair, &snapshot.candles).await?;
    pool.close().await;
    Ok(())
}

pub async fn write_candles(
    pool: &SqlitePool,
    venue: SpotVenue,
    pair: &str,
    candles: &[Candle],
) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    for candle in candles {
        sqlx::query(
            "INSERT INTO public_candle
             (exchange, pair, interval, open_time_ms, close_time_ms, open, high, low, close, base_volume)
             VALUES (?, ?, '1m', ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (exchange, pair, interval, open_time_ms) DO UPDATE SET
               close_time_ms = excluded.close_time_ms,
               open = excluded.open, high = excluded.high, low = excluded.low,
               close = excluded.close, base_volume = excluded.base_volume",
        )
        .bind(venue.id())
        .bind(pair)
        .bind(candle.open_time_ms as i64)
        .bind(candle.close_time_ms as i64)
        .bind(candle.open.to_string())
        .bind(candle.high.to_string())
        .bind(candle.low.to_string())
        .bind(candle.close.to_string())
        .bind(candle.base_volume.to_string())
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn latest_open(pool: &SqlitePool, pair: &str) -> Result<Option<u64>, sqlx::Error> {
    let latest: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(open_time_ms) FROM public_candle
         WHERE exchange = 'binance' AND pair = ? AND interval = '1m'",
    )
    .bind(pair)
    .fetch_one(pool)
    .await?;
    latest
        .map(u64::try_from)
        .transpose()
        .map_err(|err| sqlx::Error::Decode(Box::new(err)))
}

pub async fn read_range(
    pool: &SqlitePool,
    venue: SpotVenue,
    pair: &str,
    from_ms: u64,
    before_ms: u64,
) -> Result<Vec<Candle>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT open_time_ms, close_time_ms, open, high, low, close, base_volume
         FROM public_candle WHERE exchange = ? AND pair = ? AND interval = '1m'
         AND open_time_ms >= ? AND open_time_ms < ? ORDER BY open_time_ms",
    )
    .bind(venue.id())
    .bind(pair)
    .bind(from_ms as i64)
    .bind(before_ms as i64)
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| {
            let decimal = |name: &str| {
                let raw: &str = row.try_get(name)?;
                Decimal::from_str_exact(raw).map_err(|err| sqlx::Error::Decode(Box::new(err)))
            };
            let timestamp = |name: &str| {
                let raw: i64 = row.try_get(name)?;
                u64::try_from(raw).map_err(|err| sqlx::Error::Decode(Box::new(err)))
            };
            Ok(Candle {
                open_time_ms: timestamp("open_time_ms")?,
                close_time_ms: timestamp("close_time_ms")?,
                open: decimal("open")?,
                high: decimal("high")?,
                low: decimal("low")?,
                close: decimal("close")?,
                base_volume: decimal("base_volume")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quant_nexus_core::market::synthetic_snapshot;

    #[test]
    fn rereads_idempotent_archive_with_exact_decimals_and_inclusive_start() {
        tauri::async_runtime::block_on(async {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            initialize(&pool).await.unwrap();
            let fixture = synthetic_snapshot().unwrap();
            write_candles(&pool, SpotVenue::Binance, &fixture.pair, &fixture.candles)
                .await
                .unwrap();
            write_candles(&pool, SpotVenue::Binance, &fixture.pair, &fixture.candles)
                .await
                .unwrap();
            let begin = fixture.candles[1].open_time_ms;
            let end = fixture.candles.last().unwrap().open_time_ms;
            let restored = read_range(&pool, SpotVenue::Binance, &fixture.pair, begin, end)
                .await
                .unwrap();
            assert_eq!(restored, fixture.candles[1..fixture.candles.len() - 1]);
            assert_eq!(latest_open(&pool, &fixture.pair).await.unwrap(), Some(end));
            assert!(
                read_range(&pool, SpotVenue::Binance, "ETH/USDT", begin, end)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(
                read_range(&pool, SpotVenue::Bitget, &fixture.pair, begin, end)
                    .await
                    .unwrap()
                    .is_empty()
            );
        });
    }
}
