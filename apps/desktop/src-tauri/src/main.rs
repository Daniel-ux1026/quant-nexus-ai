#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod consultation;
mod context;
mod https;
mod openai;
mod paper_store;
mod replay;
mod scanner;
mod storage;

use std::path::PathBuf;

use quant_nexus_core::market::{
    BinanceSpot, MarketSnapshot, SpotPair, SpotVenue, synthetic_snapshot,
};
use quant_nexus_core::risk::{PositionEstimate, PositionInput, estimate_spot_position};
use quant_nexus_core::technical::{TechnicalFrame, technical_frames};
use scanner::{FeedStatus, ScannerController, ScannerSnapshot};
use tauri::{Manager, State};
use tokio::sync::Semaphore;

struct AppState {
    local_data: PathBuf,
    public_request: Semaphore,
    ai_request: Semaphore,
    scanner: ScannerController,
}

#[derive(serde::Serialize)]
struct MarketInspection {
    #[serde(flatten)]
    market: MarketSnapshot,
    technical: Vec<TechnicalFrame>,
}

impl From<MarketSnapshot> for MarketInspection {
    fn from(market: MarketSnapshot) -> Self {
        let technical = technical_frames(&market.candles);
        Self { market, technical }
    }
}

#[tauri::command]
async fn test_openai(state: State<'_, AppState>) -> Result<&'static str, String> {
    let _permit = state
        .ai_request
        .try_acquire()
        .map_err(|_| "Ya hay una consulta a OpenAI en curso.".to_owned())?;
    openai::test_connection().await
}

#[tauri::command]
async fn consult_market(
    request: consultation::Request,
    state: State<'_, AppState>,
) -> Result<consultation::Report, String> {
    let _permit = state
        .ai_request
        .try_acquire()
        .map_err(|_| "Ya hay una consulta a OpenAI en curso.")?;
    consultation::run(request).await
}

#[tauri::command]
fn open_consultation_source(url: String) -> Result<(), String> {
    consultation::open_source(&url)
}

#[tauri::command]
async fn start_scanner(
    capital: String,
    paper: Option<bool>,
    state: State<'_, AppState>,
) -> Result<ScannerSnapshot, String> {
    state
        .scanner
        .start(state.local_data.clone(), &capital, paper.unwrap_or(false))
}

#[tauri::command]
fn pause_paper(paused: bool, state: State<'_, AppState>) -> Result<ScannerSnapshot, String> {
    state.scanner.pause_entries(paused)?;
    state.scanner.snapshot()
}

#[tauri::command]
fn close_paper_positions(state: State<'_, AppState>) -> Result<ScannerSnapshot, String> {
    state.scanner.request_paper_close()?;
    state.scanner.snapshot()
}

#[tauri::command]
async fn saved_paper(state: State<'_, AppState>) -> Result<Option<paper_store::PaperView>, String> {
    let pool = storage::open(&state.local_data)
        .await
        .map_err(|_| "No se pudo abrir la base local.")?;
    paper_store::PaperSession::latest(&pool).await
}

#[tauri::command]
fn stop_scanner(state: State<'_, AppState>) -> Result<ScannerSnapshot, String> {
    state.scanner.stop()
}

#[tauri::command]
fn scanner_status(state: State<'_, AppState>) -> Result<ScannerSnapshot, String> {
    state.scanner.snapshot()
}

#[tauri::command]
async fn replay_market(
    capital: String,
    request: replay::ReplayRequest,
    state: State<'_, AppState>,
) -> Result<replay::ReplayReport, String> {
    replay::run(&state.local_data, &capital, request).await
}

#[tauri::command]
async fn import_binance_history(
    capital: String,
    pair: String,
    from_ms: u64,
    before_ms: u64,
    state: State<'_, AppState>,
) -> Result<replay::ReplayReport, String> {
    let _permit = state
        .public_request
        .try_acquire()
        .map_err(|_| "Ya hay una consulta pública en curso.")?;
    replay::import_binance(&state.local_data, &capital, &pair, from_ms, before_ms).await
}

#[tauri::command]
async fn explain_scanner(
    pair: String,
    close_time_ms: u64,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let snapshot = state.scanner.snapshot()?;
    let item = snapshot
        .observations
        .iter()
        .find(|item| item.pair == pair && item.observation.close_time_ms == close_time_ms)
        .filter(|_| snapshot.status == FeedStatus::Live)
        .ok_or("La observación cambió o perdió vigencia. Consulta la vela más reciente.")?;
    let _permit = state
        .ai_request
        .try_acquire()
        .map_err(|_| "Ya hay una consulta a OpenAI en curso.")?;
    openai::explain(&item.pair, &item.observation).await
}

#[tauri::command]
fn load_fixture() -> Result<MarketInspection, String> {
    synthetic_snapshot()
        .map(MarketInspection::from)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn estimate_position(input: PositionInput) -> Result<PositionEstimate, String> {
    estimate_spot_position(&input).map_err(|error| error.to_string())
}

#[tauri::command]
async fn refresh_market(
    pair: String,
    venue: Option<SpotVenue>,
    state: State<'_, AppState>,
) -> Result<MarketInspection, String> {
    let pair: SpotPair = pair
        .parse()
        .map_err(|error: quant_nexus_core::market::MarketError| error.to_string())?;
    let _permit = state
        .public_request
        .try_acquire()
        .map_err(|_| "Ya hay una consulta de mercado en curso.".to_owned())?;
    let snapshot = fetch_market(venue.unwrap_or_default(), pair).await?;

    storage::save(&state.local_data, &snapshot)
        .await
        .map_err(|_| {
            "Las velas se recibieron, pero no pudieron guardarse en la base local.".to_owned()
        })?;
    Ok(snapshot.into())
}

#[tauri::command]
async fn market_context(
    pair: String,
    state: State<'_, AppState>,
) -> Result<context::MarketContext, String> {
    let pair = pair
        .parse::<SpotPair>()
        .map_err(|error| error.to_string())?;
    let _permit = state
        .public_request
        .try_acquire()
        .map_err(|_| "Ya hay una consulta pública en curso.")?;
    Ok(context::fetch(pair).await)
}

async fn fetch_market(venue: SpotVenue, pair: SpotPair) -> Result<MarketSnapshot, String> {
    let clock = match venue {
        SpotVenue::Binance => {
            BinanceSpot::parse_server_time(&download(BinanceSpot::SERVER_TIME_URL).await?)
                .map_err(|error| error.to_string())?
        }
        SpotVenue::Bitget => {
            let body = download("https://api.bitget.com/api/v3/market/time").await?;
            let value: serde_json::Value =
                serde_json::from_slice(&body).map_err(|_| "Bitget devolvió una hora inválida.")?;
            value["data"]["serverTime"]
                .as_str()
                .and_then(|value| value.parse().ok())
                .filter(|_| value["code"] == "00000")
                .ok_or("Bitget devolvió una hora inválida.")?
        }
        SpotVenue::Bitunix => scanner::unix_ms(),
    };
    let body = download(&venue.candles_url(&pair)).await?;
    venue
        .decode(pair, &body, clock)
        .map_err(|error| error.to_string())
}

async fn download(url: &str) -> Result<Vec<u8>, String> {
    let response = https::request(url, None).await?;
    if response.status == 200 {
        Ok(response.body)
    } else {
        Err(format!(
            "El exchange no devolvió datos de mercado. HTTP {}.",
            response.status
        ))
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(AppState {
                local_data: app.path().app_local_data_dir()?,
                public_request: Semaphore::new(1),
                ai_request: Semaphore::new(1),
                scanner: ScannerController::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_fixture,
            estimate_position,
            refresh_market,
            market_context,
            consult_market,
            open_consultation_source,
            test_openai,
            start_scanner,
            stop_scanner,
            scanner_status,
            pause_paper,
            close_paper_positions,
            saved_paper,
            replay_market,
            import_binance_history,
            explain_scanner
        ])
        .run(tauri::generate_context!())
        .expect("No se pudo iniciar Quant Nexus AI");
}

#[cfg(test)]
mod tests {
    use super::*;
    use quant_nexus_core::market::MarketSource;

    #[test]
    #[ignore = "Consulta los endpoints públicos de Binance por HTTPS"]
    fn public_binance_spot_smoke() {
        tauri::async_runtime::block_on(async {
            let snapshot = fetch_market(SpotVenue::Binance, "BTC/USDT".parse().unwrap())
                .await
                .unwrap();
            assert_eq!(snapshot.source, MarketSource::BinancePublic);
            assert_eq!(snapshot.pair, "BTC/USDT");
            assert!(snapshot.candles.len() >= 2);
            assert!(snapshot.candles.last().unwrap().close_time_ms < snapshot.as_of_ms);
        });
    }

    #[test]
    #[ignore = "Consulta los endpoints públicos de Bitget y Bitunix"]
    fn public_secondary_exchanges_smoke() {
        tauri::async_runtime::block_on(async {
            for venue in [SpotVenue::Bitget, SpotVenue::Bitunix] {
                let snapshot = fetch_market(venue, "BTC/USDT".parse().unwrap())
                    .await
                    .unwrap_or_else(|error| panic!("{} falló: {error}", venue.id()));
                assert_eq!(snapshot.source, venue.source());
                assert!(snapshot.candles.len() >= if venue == SpotVenue::Bitget { 95 } else { 2 });
                assert!(snapshot.candles.last().unwrap().close_time_ms < snapshot.as_of_ms);
            }
        });
    }
}
