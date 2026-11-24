use quant_nexus_core::Decimal;
use quant_nexus_core::market::{Candle, MINUTE_MS, SpotVenue};
use quant_nexus_core::paper::{PaperAccount, PaperBar, PaperEvent};
use serde::Serialize;
use sqlx::{SqlitePool, Transaction};

use crate::{
    scanner::{PairObservation, WATCHLIST, unix_ms},
    storage,
};

const HISTORY_LIMIT: usize = 20;
const MAX_RECOVERY_BARS: u64 = 1_440;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperView {
    pub account: PaperAccount,
    pub events: Vec<PaperEvent>,
    pub detail: Option<String>,
}

pub struct PaperSession {
    id: i64,
    revision: i64,
    view: PaperView,
}

async fn initialize(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS paper_account (
            id INTEGER PRIMARY KEY, capital TEXT NOT NULL UNIQUE, revision INTEGER NOT NULL,
            state_json TEXT NOT NULL, updated_ms INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS paper_event (
            account_id INTEGER NOT NULL REFERENCES paper_account(id),
            seq INTEGER NOT NULL, event_json TEXT NOT NULL,
            PRIMARY KEY (account_id, seq)
        ) WITHOUT ROWID",
    )
    .execute(pool)
    .await
    .map_err(storage_error)?;
    Ok(())
}

const SELECT_ACCOUNT: &str = "SELECT id, revision, state_json FROM paper_account";

impl PaperSession {
    pub async fn load(pool: &SqlitePool, capital: Decimal) -> Result<Self, String> {
        initialize(pool).await?;
        let account = PaperAccount::new(capital, &WATCHLIST).map_err(|error| error.to_string())?;
        let capital = capital.normalize().to_string();
        let json = serde_json::to_string(&account).map_err(storage_error)?;
        sqlx::query(
            "INSERT INTO paper_account (capital, revision, state_json, updated_ms)
             VALUES (?, 0, ?, ?) ON CONFLICT (capital) DO NOTHING",
        )
        .bind(&capital)
        .bind(json)
        .bind(unix_ms() as i64)
        .execute(pool)
        .await
        .map_err(storage_error)?;
        let row =
            sqlx::query_as::<_, (i64, i64, String)>(&format!("{SELECT_ACCOUNT} WHERE capital = ?"))
                .bind(&capital)
                .fetch_one(pool)
                .await
                .map_err(storage_error)?;
        load_record(pool, row).await
    }

    pub async fn latest(pool: &SqlitePool) -> Result<Option<PaperView>, String> {
        initialize(pool).await?;
        let row = sqlx::query_as::<_, (i64, i64, String)>(&format!(
            "{SELECT_ACCOUNT} ORDER BY updated_ms DESC, id DESC LIMIT 1"
        ))
        .fetch_optional(pool)
        .await
        .map_err(storage_error)?;
        Ok(match row {
            Some(row) => Some(load_record(pool, row).await?.view),
            None => None,
        })
    }

    pub fn view(&self) -> PaperView {
        self.view.clone()
    }

    pub fn equity(&self) -> Decimal {
        self.view.account.equity
    }

    pub fn last_open(&self) -> Option<u64> {
        self.view.account.last_close_ms?.checked_sub(MINUTE_MS - 1)
    }

    pub async fn recover(&mut self, pool: &SqlitePool, before_ms: u64) -> Result<(), String> {
        let Some(last_close) = self.view.account.last_close_ms else {
            return Ok(());
        };
        let from = last_close + 1;
        let Some(span) = before_ms.checked_sub(from) else {
            return Ok(());
        };
        let expected = span / MINUTE_MS;
        if expected == 0 {
            return Ok(());
        }
        if expected > MAX_RECOVERY_BARS {
            return Err("La cartera está en pausa. El intervalo pendiente supera el límite de recuperación de 24 h.".into());
        }
        let mut archives = Vec::with_capacity(WATCHLIST.len());
        for pair in WATCHLIST {
            let candles = storage::read_range(pool, SpotVenue::Binance, pair, from, before_ms)
                .await
                .map_err(storage_error)?;
            if candles.len() as u64 != expected {
                return Err(
                    "La cartera está en pausa hasta recuperar todas sus velas pendientes.".into(),
                );
            }
            archives.push(candles);
        }
        let mut next = self.view.account.clone();
        let mut events = Vec::new();
        for index in 0..expected as usize {
            let batch = WATCHLIST
                .iter()
                .zip(&archives)
                .map(|(pair, data)| PaperBar {
                    pair,
                    candle: &data[index],
                    signal: None,
                })
                .collect::<Vec<_>>();
            events.extend(
                next.advance(&batch, true)
                    .map_err(|error| error.to_string())?,
            );
        }
        self.commit(pool, next, events).await
    }

    pub async fn live(
        &mut self,
        pool: &SqlitePool,
        windows: &[Vec<Candle>],
        observations: &[PairObservation],
        entry_paused: bool,
        emergency_after: Option<u64>,
    ) -> Result<(), String> {
        let Some(next_close) = windows
            .first()
            .and_then(|window| window.last())
            .map(|bar| bar.close_time_ms)
        else {
            return Ok(());
        };
        if windows.iter().any(|window| {
            window
                .last()
                .is_none_or(|bar| bar.close_time_ms != next_close)
        }) || self
            .view
            .account
            .last_close_ms
            .is_some_and(|previous| previous >= next_close)
        {
            return Ok(());
        }
        let batch = windows
            .iter()
            .zip(observations)
            .map(|(window, item)| PaperBar {
                pair: &item.pair,
                candle: window
                    .last()
                    .expect("El cierre de esta ventana ya fue comprobado"),
                signal: item.observation.scenario.as_ref(),
            })
            .collect::<Vec<_>>();
        let mut next = self.view.account.clone();
        let events = next
            .advance_controlled(
                &batch,
                entry_paused,
                emergency_after.is_some_and(|requested| next_close >= requested),
            )
            .map_err(|error| error.to_string())?;
        self.commit(pool, next, events).await
    }

    async fn commit(
        &mut self,
        pool: &SqlitePool,
        account: PaperAccount,
        events: Vec<PaperEvent>,
    ) -> Result<(), String> {
        let detail = account
            .circuit()
            .map_err(|error| error.to_string())?
            .map(str::to_owned);
        let mut transaction = pool.begin().await.map_err(storage_error)?;
        let changed = sqlx::query(
            "UPDATE paper_account SET revision = revision + 1, state_json = ?, updated_ms = ?
             WHERE id = ? AND revision = ?",
        )
        .bind(serde_json::to_string(&account).map_err(storage_error)?)
        .bind(unix_ms() as i64)
        .bind(self.id)
        .bind(self.revision)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?
        .rows_affected();
        if changed != 1 {
            return Err(
                "Otra instancia modificó la cartera. La sesión se recargará antes de continuar."
                    .into(),
            );
        }
        insert_events(&mut transaction, self.id, &events).await?;
        transaction.commit().await.map_err(storage_error)?;
        self.revision += 1;
        self.view.detail = detail;
        self.view.account = account;
        self.view.events.splice(0..0, events.into_iter().rev());
        self.view.events.truncate(HISTORY_LIMIT);
        Ok(())
    }
}

async fn load_record(
    pool: &SqlitePool,
    (id, revision, json): (i64, i64, String),
) -> Result<PaperSession, String> {
    let account: PaperAccount = serde_json::from_str(&json).map_err(storage_error)?;
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT event_json FROM paper_event WHERE account_id = ? ORDER BY seq DESC LIMIT ?",
    )
    .bind(id)
    .bind(HISTORY_LIMIT as i64)
    .fetch_all(pool)
    .await
    .map_err(storage_error)?;
    let events = rows
        .iter()
        .map(|event| serde_json::from_str(event).map_err(storage_error))
        .collect::<Result<Vec<_>, _>>()?;
    let detail = account
        .circuit()
        .map_err(|error| error.to_string())?
        .map(str::to_owned);
    Ok(PaperSession {
        id,
        revision,
        view: PaperView {
            account,
            events,
            detail,
        },
    })
}

async fn insert_events(
    transaction: &mut Transaction<'_, sqlx::Sqlite>,
    id: i64,
    events: &[PaperEvent],
) -> Result<(), String> {
    for event in events {
        sqlx::query("INSERT INTO paper_event (account_id, seq, event_json) VALUES (?, ?, ?)")
            .bind(id)
            .bind(i64::try_from(event.sequence).map_err(storage_error)?)
            .bind(serde_json::to_string(event).map_err(storage_error)?)
            .execute(&mut **transaction)
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

fn storage_error(_: impl std::fmt::Display) -> String {
    "No se pudo guardar o consultar el registro de paper trading.".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quant_nexus_core::{market::synthetic_snapshot, scanner::evaluate};

    #[test]
    fn restart_recovers_exactly_and_blocks_a_concurrent_writer() {
        tauri::async_runtime::block_on(async {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            let capital = Decimal::from(1000);
            let mut session = PaperSession::load(&pool, capital).await.unwrap();
            let mut competitor = PaperSession::load(&pool, capital).await.unwrap();
            let fixture = synthetic_snapshot().unwrap();
            let windows = vec![fixture.candles[..21].to_vec(); 3];
            let observations = WATCHLIST
                .iter()
                .map(|pair| PairObservation {
                    pair: (*pair).into(),
                    observation: evaluate(&windows[0], capital).unwrap(),
                })
                .collect::<Vec<_>>();
            session
                .live(&pool, &windows, &observations, false, None)
                .await
                .unwrap();
            assert!(
                competitor
                    .live(&pool, &windows, &observations, false, None)
                    .await
                    .is_err()
            );
            let reloaded = PaperSession::load(&pool, capital).await.unwrap();
            assert_eq!(reloaded.view.account, session.view.account);
            assert_eq!(reloaded.view.events, session.view.events);
        });
    }
}
