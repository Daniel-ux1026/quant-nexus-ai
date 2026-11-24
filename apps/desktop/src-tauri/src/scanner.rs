use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt, future::try_join_all};
use quant_nexus_core::Decimal;
use quant_nexus_core::market::{
    BinanceSpot, Candle, MINUTE_MS, SpotPair, SpotVenue, validate_series,
};
use quant_nexus_core::scanner::{ScanObservation, evaluate, parse_capital};
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async_with_config};

use crate::{
    download,
    paper_store::{PaperSession, PaperView},
    storage,
};

pub const WATCHLIST: [&str; 3] = ["BTC/USDT", "ETH/USDT", "SOL/USDT"];
const WINDOW: usize = 100;
const MAX_BACKFILL_MS: u64 = 24 * 60 * MINUTE_MS;
const SOCKET_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_STALENESS_MS: u64 = 2 * MINUTE_MS + 10_000;
type Connection = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedStatus {
    #[default]
    Stopped,
    Syncing,
    Live,
    Reconnecting,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairObservation {
    pub pair: String,
    pub observation: ScanObservation,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannerSnapshot {
    pub status: FeedStatus,
    pub capital: Option<Decimal>,
    pub detail: Option<String>,
    pub observations: Vec<PairObservation>,
    pub paper: Option<PaperView>,
    pub paper_mode: bool,
    pub entries_paused: bool,
    pub emergency_pending: bool,
}

#[derive(Default)]
struct Session {
    generation: u64,
    task: Option<JoinHandle<()>>,
    snapshot: ScannerSnapshot,
    last_live_update: Option<Instant>,
    entries_paused: bool,
    emergency_after: Option<u64>,
}

#[derive(Clone, Default)]
pub struct ScannerController(Arc<Mutex<Session>>);

impl ScannerController {
    pub fn start(
        &self,
        path: PathBuf,
        capital: &str,
        paper_mode: bool,
    ) -> Result<ScannerSnapshot, String> {
        let capital = parse_capital(capital).map_err(|error| error.to_string())?;
        let mut session = self
            .0
            .lock()
            .map_err(|_| "El escáner no está disponible.")?;
        if session
            .task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return Err("El escáner ya está en marcha.".into());
        }

        session.generation = session.generation.wrapping_add(1);
        session.last_live_update = None;
        session.entries_paused = false;
        session.emergency_after = None;
        session.snapshot = ScannerSnapshot {
            status: FeedStatus::Syncing,
            capital: Some(capital),
            detail: Some("Consultando velas cerradas en Binance.".into()),
            observations: Vec::new(),
            paper_mode,
            ..ScannerSnapshot::default()
        };
        let generation = session.generation;
        let controller = self.clone();
        session.task = Some(tokio::spawn(async move {
            controller.run(&path, capital, generation, paper_mode).await;
        }));
        Ok(session.snapshot.clone())
    }

    pub fn pause_entries(&self, paused: bool) -> Result<(), String> {
        let mut session = self
            .0
            .lock()
            .map_err(|_| "El escáner no está disponible.")?;
        if !session.snapshot.paper_mode || session.snapshot.status == FeedStatus::Stopped {
            return Err("Inicia paper trading para controlar sus entradas.".into());
        }
        if !paused && session.emergency_after.is_some() {
            return Err(
                "Espera a que termine el cierre simulado antes de reanudar entradas.".into(),
            );
        }
        session.entries_paused = paused;
        Ok(())
    }

    pub fn request_paper_close(&self) -> Result<(), String> {
        let mut session = self
            .0
            .lock()
            .map_err(|_| "El escáner no está disponible.")?;
        if !session.snapshot.paper_mode || session.snapshot.status == FeedStatus::Stopped {
            return Err("Inicia paper trading para solicitar un cierre simulado.".into());
        }
        session.entries_paused = true;
        session.emergency_after.get_or_insert_with(unix_ms);
        Ok(())
    }

    fn emergency_after(&self, generation: u64) -> Option<u64> {
        let session = self.0.lock().ok()?;
        (session.generation == generation)
            .then_some(session.emergency_after)
            .flatten()
    }

    fn entries_paused(&self, generation: u64) -> bool {
        self.0.lock().map_or(true, |session| {
            session.generation != generation || session.entries_paused
        })
    }

    pub fn stop(&self) -> Result<ScannerSnapshot, String> {
        let mut session = self
            .0
            .lock()
            .map_err(|_| "El escáner no está disponible.")?;
        session.generation = session.generation.wrapping_add(1);
        if let Some(task) = session.task.take() {
            task.abort();
        }
        session.snapshot = ScannerSnapshot::default();
        session.last_live_update = None;
        session.entries_paused = true;
        session.emergency_after = None;
        Ok(session.snapshot.clone())
    }

    pub fn snapshot(&self) -> Result<ScannerSnapshot, String> {
        let session = self
            .0
            .lock()
            .map_err(|_| "El escáner no está disponible.")?;
        let mut snapshot = session.snapshot.clone();
        snapshot.entries_paused = session.entries_paused;
        snapshot.emergency_pending = session.emergency_after.is_some();
        if snapshot.status == FeedStatus::Live
            && session
                .last_live_update
                .is_none_or(|updated| updated.elapsed() > Duration::from_millis(MAX_STALENESS_MS))
        {
            snapshot.status = FeedStatus::Reconnecting;
            snapshot.detail =
                Some("Los datos perdieron vigencia. Las señales están en pausa.".into());
            snapshot.observations.clear();
        }
        Ok(snapshot)
    }

    fn publish(&self, generation: u64, mut snapshot: ScannerSnapshot) {
        if let Ok(mut session) = self.0.lock()
            && session.generation == generation
        {
            session.last_live_update = (snapshot.status == FeedStatus::Live).then(Instant::now);
            snapshot.paper_mode = session.snapshot.paper_mode;
            if let Some(view) = snapshot.paper.as_ref()
                && let Some(after) = session.emergency_after
                && view
                    .account
                    .last_close_ms
                    .is_some_and(|close| close >= after)
                && view.account.positions.is_empty()
            {
                session.emergency_after = None;
            }
            snapshot.paper = snapshot.paper.or_else(|| session.snapshot.paper.clone());
            session.snapshot = snapshot;
        }
    }

    async fn run(&self, path: &Path, capital: Decimal, generation: u64, paper_mode: bool) {
        let pairs: Vec<SpotPair> = WATCHLIST
            .iter()
            .map(|pair| pair.parse().expect("El par configurado debe ser válido"))
            .collect();
        let mut last_saved = vec![None; pairs.len()];
        let mut delay = 2;
        loop {
            let started = Instant::now();
            let fault = match storage::open(path).await {
                Ok(pool) => match load_paper(&pool, capital, paper_mode).await {
                    Ok(mut paper) => {
                        self.feed(
                            &pool,
                            &pairs,
                            &mut last_saved,
                            capital,
                            generation,
                            &mut paper,
                        )
                        .await
                    }
                    Err(problem) => Err(problem),
                },
                Err(_) => Err("No se pudo abrir el historial local.".into()),
            };
            self.publish(
                generation,
                ScannerSnapshot {
                    status: FeedStatus::Reconnecting,
                    capital: Some(capital),
                    detail: fault.err(),
                    observations: Vec::new(),
                    ..ScannerSnapshot::default()
                },
            );
            if started.elapsed() > Duration::from_secs(60) {
                delay = 2;
            }
            sleep(Duration::from_secs(delay)).await;
            delay = (delay * 2).min(30);
        }
    }

    async fn feed(
        &self,
        pool: &SqlitePool,
        pairs: &[SpotPair],
        last_saved: &mut [Option<u64>],
        capital: Decimal,
        generation: u64,
        paper: &mut Option<PaperSession>,
    ) -> Result<(), String> {
        let server_time =
            BinanceSpot::parse_server_time(&download(BinanceSpot::SERVER_TIME_URL).await?)
                .map_err(|error| error.to_string())?;
        let anchor = Instant::now();
        let current_minute = server_time - (server_time % MINUTE_MS);
        let paper_cursor = paper.as_ref().and_then(PaperSession::last_open);
        let mut windows = try_join_all(pairs.iter().zip(last_saved.iter().copied()).map(
            |(pair, previous)| {
                let previous = match (previous, paper_cursor) {
                    (Some(local), Some(account)) => Some(local.min(account)),
                    (local, account) => local.or(account),
                };
                sync_pair(pool, pair, previous, current_minute, server_time)
            },
        ))
        .await?;
        for (recorded, window) in last_saved.iter_mut().zip(&windows) {
            *recorded = window.last().map(|bar| bar.open_time_ms);
        }
        let mut socket = connect(pairs).await?;
        if let Some(session) = paper {
            session.recover(pool, current_minute).await?;
        }
        self.publish_live(pool, pairs, &windows, capital, generation, paper)
            .await?;

        loop {
            let frame = timeout(SOCKET_TIMEOUT, socket.next())
                .await
                .map_err(|_| "Binance dejó de enviar datos. Se reanudará desde el historial.")?
                .ok_or("Binance cerró el flujo.")?
                .map_err(|_| "La conexión WebSocket con Binance se interrumpió.")?;
            let market_now = server_time.saturating_add(anchor.elapsed().as_millis() as u64);
            if windows.iter().any(|window| {
                window.last().is_none_or(|bar| {
                    market_now.saturating_sub(bar.close_time_ms) > MAX_STALENESS_MS
                })
            }) {
                return Err("Faltan velas recientes. Se recuperará el tramo pendiente.".into());
            }
            let text = match frame {
                Message::Text(text) => text,
                Message::Ping(_) => {
                    timeout(SOCKET_TIMEOUT, socket.flush())
                        .await
                        .map_err(|_| "Binance no recibió la respuesta al ping.")?
                        .map_err(|_| "No se pudo confirmar el estado del WebSocket.")?;
                    continue;
                }
                Message::Close(_) => return Err("Binance cerró el flujo. Reconectando…".into()),
                Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => continue,
            };
            let Some((pair, candle)) =
                BinanceSpot::decode_stream(text.as_bytes()).map_err(|error| error.to_string())?
            else {
                continue;
            };
            let index = pairs
                .iter()
                .position(|candidate| *candidate == pair)
                .ok_or("Binance envió un símbolo ajeno a la lista observada.")?;
            let window = &mut windows[index];
            if candle.close_time_ms > market_now.saturating_add(10_000) {
                return Err("Binance envió una vela posterior al reloj sincronizado.".into());
            }
            match incoming(window, &candle)? {
                Arrival::Duplicate => continue,
                Arrival::Next => {}
            }
            storage::write_candles(
                pool,
                SpotVenue::Binance,
                &pair.to_string(),
                std::slice::from_ref(&candle),
            )
            .await
            .map_err(|_| "No se pudo archivar la vela. Se pausaron las señales.")?;
            last_saved[index] = Some(candle.open_time_ms);
            if window.len() == WINDOW {
                window.remove(0);
            }
            window.push(candle);
            self.publish_live(pool, pairs, &windows, capital, generation, paper)
                .await?;
        }
    }

    async fn publish_live(
        &self,
        pool: &SqlitePool,
        pairs: &[SpotPair],
        windows: &[Vec<Candle>],
        capital: Decimal,
        generation: u64,
        paper: &mut Option<PaperSession>,
    ) -> Result<(), String> {
        let signal_capital = paper.as_ref().map_or(capital, PaperSession::equity);
        let mut snapshot = live_snapshot(pairs, windows, signal_capital)?;
        snapshot.capital = Some(capital);
        if let Some(session) = paper {
            session
                .live(
                    pool,
                    windows,
                    &snapshot.observations,
                    self.entries_paused(generation),
                    self.emergency_after(generation),
                )
                .await?;
            snapshot.paper = Some(session.view());
        }
        self.publish(generation, snapshot);
        Ok(())
    }
}

async fn load_paper(
    pool: &SqlitePool,
    capital: Decimal,
    enabled: bool,
) -> Result<Option<PaperSession>, String> {
    if enabled {
        PaperSession::load(pool, capital).await.map(Some)
    } else {
        Ok(None)
    }
}

fn live_snapshot(
    pairs: &[SpotPair],
    windows: &[Vec<Candle>],
    capital: Decimal,
) -> Result<ScannerSnapshot, String> {
    let observations = pairs
        .iter()
        .zip(windows)
        .map(|(pair, window)| {
            let observation = evaluate(window, capital).map_err(|error| error.to_string())?;
            Ok(PairObservation {
                pair: pair.to_string(),
                observation,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(ScannerSnapshot {
        status: FeedStatus::Live,
        capital: Some(capital),
        detail: None,
        observations,
        ..ScannerSnapshot::default()
    })
}

enum Arrival {
    Duplicate,
    Next,
}

fn incoming(window: &[Candle], candle: &Candle) -> Result<Arrival, String> {
    let last = window
        .last()
        .ok_or("Hace falta sincronizar el historial inicial.")?;
    if candle.open_time_ms <= last.open_time_ms {
        if window
            .iter()
            .any(|bar| bar.open_time_ms == candle.open_time_ms && bar != candle)
        {
            return Err("Binance publicó una revisión. Se sincronizará la serie.".into());
        }
        return Ok(Arrival::Duplicate);
    }
    if last.close_time_ms.checked_add(1) != Some(candle.open_time_ms) {
        return Err(
            "Se detectó un hueco en Binance. Las señales se pausaron para recuperar las velas."
                .into(),
        );
    }
    Ok(Arrival::Next)
}

async fn connect(pairs: &[SpotPair]) -> Result<Connection, String> {
    let streams = pairs
        .iter()
        .map(SpotPair::stream_name)
        .collect::<Vec<_>>()
        .join("/");
    let url = format!("wss://data-stream.binance.vision/stream?streams={streams}");
    let config = WebSocketConfig::default()
        .max_frame_size(Some(65_536))
        .max_message_size(Some(65_536));
    let (connection, _) = timeout(
        SOCKET_TIMEOUT,
        connect_async_with_config(url, Some(config), false),
    )
    .await
    .map_err(|_| "La conexión con Binance tardó demasiado.")?
    .map_err(|_| "No se pudo abrir el WebSocket público de Binance.")?;
    Ok(connection)
}

async fn sync_pair(
    pool: &SqlitePool,
    pair: &SpotPair,
    previous: Option<u64>,
    before_ms: u64,
    server_time: u64,
) -> Result<Vec<Candle>, String> {
    let pair_name = pair.to_string();
    let archived = storage::latest_open(pool, &pair_name)
        .await
        .map_err(|_| "No se pudo consultar el progreso del historial.")?;
    let recent_from = before_ms.saturating_sub(WINDOW as u64 * MINUTE_MS);
    let mut from_ms = previous
        .or(archived)
        .and_then(|time| time.checked_add(MINUTE_MS))
        .filter(|time| *time < before_ms && before_ms - *time <= MAX_BACKFILL_MS)
        .map_or(recent_from, |time| time.min(recent_from));
    while from_ms < before_ms {
        let body = download(&BinanceSpot::archive_url(pair, from_ms, before_ms - 1)).await?;
        let page =
            BinanceSpot::decode_archive(&body, server_time).map_err(|error| error.to_string())?;
        if page.first().is_none_or(|bar| bar.open_time_ms != from_ms)
            || page.last().is_none_or(|bar| bar.close_time_ms >= before_ms)
        {
            return Err(format!(
                "Binance todavía no entregó todas las velas de {pair}."
            ));
        }
        storage::write_candles(pool, SpotVenue::Binance, &pair_name, &page)
            .await
            .map_err(|_| "No se pudo guardar la recuperación de mercado.")?;
        from_ms = page
            .last()
            .ok_or("Binance devolvió una página vacía.")?
            .close_time_ms
            + 1;
    }
    let saved = storage::read_range(pool, SpotVenue::Binance, &pair_name, recent_from, before_ms)
        .await
        .map_err(|_| "No se pudo leer el historial recuperado.")?;
    if saved.len() != WINDOW {
        return Err(format!("Faltan velas para analizar {pair}."));
    }
    validate_series(&saved).map_err(|error| error.to_string())?;
    Ok(saved)
}

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quant_nexus_core::market::synthetic_snapshot;

    #[test]
    fn checks_duplicate_events_and_forces_recovery_on_gaps_or_revisions() {
        let fixture = synthetic_snapshot().unwrap();
        let window = &fixture.candles[..9];
        assert!(matches!(
            incoming(window, &fixture.candles[8]),
            Ok(Arrival::Duplicate)
        ));
        assert!(matches!(
            incoming(window, &fixture.candles[9]),
            Ok(Arrival::Next)
        ));
        assert!(incoming(window, &fixture.candles[10]).is_err());
        let mut revised = fixture.candles[8].clone();
        revised.base_volume += Decimal::ONE;
        assert!(incoming(window, &revised).is_err());
        assert!(matches!(
            incoming(&fixture.candles[..11], &fixture.candles[10]),
            Ok(Arrival::Duplicate)
        ));
    }

    #[test]
    #[ignore = "Abre el WebSocket público de Binance"]
    fn binance_websocket_smoke() {
        tauri::async_runtime::block_on(async {
            let pairs = WATCHLIST
                .iter()
                .map(|pair| pair.parse().unwrap())
                .collect::<Vec<_>>();
            let mut socket = connect(&pairs).await.unwrap();
            let mut symbols = std::collections::BTreeSet::new();
            timeout(Duration::from_secs(15), async {
                while symbols.len() < WATCHLIST.len() {
                    let frame = socket.next().await.unwrap().unwrap();
                    if let Message::Text(text) = frame {
                        BinanceSpot::decode_stream(text.as_bytes()).unwrap();
                        let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
                        symbols.insert(payload["stream"].as_str().unwrap().to_owned());
                    }
                }
            })
            .await
            .unwrap();
            let expected: std::collections::BTreeSet<_> =
                pairs.iter().map(SpotPair::stream_name).collect();
            assert_eq!(symbols, expected);
        });
    }

    #[test]
    #[ignore = "Consulta velas públicas en Binance y simula un cursor de desconexión"]
    fn binance_gap_recovery_smoke() {
        tauri::async_runtime::block_on(async {
            let pair: SpotPair = WATCHLIST[0].parse().unwrap();
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            storage::initialize(&pool).await.unwrap();
            let server_time = BinanceSpot::parse_server_time(
                &download(BinanceSpot::SERVER_TIME_URL).await.unwrap(),
            )
            .unwrap();
            let before_ms = server_time - (server_time % MINUTE_MS);
            let start_ms = before_ms - 120 * MINUTE_MS;
            let last_saved = Some(start_ms - MINUTE_MS);
            let first_window = sync_pair(&pool, &pair, last_saved, before_ms, server_time)
                .await
                .unwrap();
            assert_eq!(first_window.len(), WINDOW);
            let second_window = sync_pair(&pool, &pair, last_saved, before_ms, server_time)
                .await
                .unwrap();
            assert_eq!(first_window, second_window);
            let recovered = storage::read_range(
                &pool,
                SpotVenue::Binance,
                &pair.to_string(),
                start_ms,
                before_ms,
            )
            .await
            .unwrap();
            assert_eq!(recovered.len(), 120);
            validate_series(&recovered).unwrap();
        });
    }
}
