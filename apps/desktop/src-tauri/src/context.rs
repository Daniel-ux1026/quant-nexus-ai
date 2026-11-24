use futures_util::future::join3;
use quant_nexus_core::Decimal;
use quant_nexus_core::market::SpotPair;
use quick_xml::{Reader, events::Event};
use serde::Serialize;
use serde_json::Value;

use crate::{download, scanner::unix_ms};

const FED_URL: &str = "https://www.federalreserve.gov/feeds/press_monetary.xml";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketContext {
    pub pair: String,
    pub book: Option<Book>,
    pub derivatives: Option<Derivatives>,
    pub monetary_news: Vec<Bulletin>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    bid: Decimal,
    ask: Decimal,
    spread_bps: Decimal,
    imbalance: Decimal,
    last_update_id: u64,
    received_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Derivatives {
    mark: Decimal,
    index: Decimal,
    funding: Decimal,
    open_interest: Decimal,
    mark_time_ms: u64,
    interest_time_ms: u64,
    next_funding_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bulletin {
    title: String,
    published: String,
}

pub async fn fetch(pair: SpotPair) -> MarketContext {
    let symbol = pair.exchange_symbol();
    let (book, derivatives, bulletins) =
        join3(book(&symbol), derivatives(&symbol), bulletin()).await;
    let warnings = [
        book.as_ref().err(),
        derivatives.as_ref().err(),
        bulletins.as_ref().err(),
    ]
    .into_iter()
    .flatten()
    .map(|value| (*value).to_owned())
    .collect();
    MarketContext {
        pair: pair.to_string(),
        book: book.ok(),
        derivatives: derivatives.ok(),
        monetary_news: bulletins.unwrap_or_default(),
        warnings,
    }
}

async fn book(symbol: &str) -> Result<Book, &'static str> {
    let url = format!("https://data-api.binance.vision/api/v3/depth?symbol={symbol}&limit=20");
    let body = download(&url)
        .await
        .map_err(|_| "No se pudo consultar el libro Spot de Binance.")?;
    decode_book(&body).ok_or("Binance devolvió un libro vacío o inconsistente.")
}

fn decode_book(body: &[u8]) -> Option<Book> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let (bid, total_bid) = levels(&value["bids"], true)?;
    let (ask, total_ask) = levels(&value["asks"], false)?;
    if ask <= bid {
        return None;
    }
    let midpoint = ask.checked_add(bid)?.checked_div(Decimal::from(2))?;
    let spread_bps = ask
        .checked_sub(bid)?
        .checked_div(midpoint)?
        .checked_mul(Decimal::from(10_000))?;
    let imbalance = total_bid
        .checked_sub(total_ask)?
        .checked_div(total_bid.checked_add(total_ask)?)?;
    Some(Book {
        bid,
        ask,
        spread_bps,
        imbalance,
        last_update_id: value["lastUpdateId"].as_u64()?,
        received_ms: unix_ms(),
    })
}

fn levels(rows: &Value, descending: bool) -> Option<(Decimal, Decimal)> {
    let rows = rows.as_array()?;
    if rows.is_empty() || rows.len() > 20 {
        return None;
    }
    let mut previous = None;
    let mut first = None;
    let mut total = Decimal::ZERO;
    for row in rows.iter().take(10) {
        let row = row.as_array().filter(|entry| entry.len() == 2)?;
        let price = decimal(&row[0]).filter(|value| *value > Decimal::ZERO)?;
        let quantity = decimal(&row[1]).filter(|value| *value > Decimal::ZERO)?;
        if previous.is_some_and(|last| {
            if descending {
                price >= last
            } else {
                price <= last
            }
        }) {
            return None;
        }
        first.get_or_insert(price);
        total = total.checked_add(quantity)?;
        previous = Some(price);
    }
    Some((first?, total))
}

async fn derivatives(symbol: &str) -> Result<Derivatives, &'static str> {
    let mark_url = format!("https://fapi.binance.com/fapi/v1/premiumIndex?symbol={symbol}");
    let open_url = format!("https://fapi.binance.com/fapi/v1/openInterest?symbol={symbol}");
    let (mark, interest) = futures_util::future::try_join(download(&mark_url), download(&open_url))
        .await
        .map_err(|_| "La consulta pública de futuros USDⓈ-M no está disponible para este par.")?;
    decode_derivatives(symbol, &mark, &interest)
        .ok_or("Binance devolvió datos de futuros incompletos o inválidos.")
}

fn decode_derivatives(symbol: &str, mark: &[u8], interest: &[u8]) -> Option<Derivatives> {
    let mark: Value = serde_json::from_slice(mark).ok()?;
    let interest: Value = serde_json::from_slice(interest).ok()?;
    if mark["symbol"] != symbol || interest["symbol"] != symbol {
        return None;
    }
    let positive = |value: &Value| decimal(value).filter(|amount| *amount > Decimal::ZERO);
    Some(Derivatives {
        mark: positive(&mark["markPrice"])?,
        index: positive(&mark["indexPrice"])?,
        funding: decimal(&mark["lastFundingRate"])?,
        open_interest: decimal(&interest["openInterest"])
            .filter(|amount| *amount >= Decimal::ZERO)?,
        mark_time_ms: mark["time"].as_u64()?,
        interest_time_ms: interest["time"].as_u64()?,
        next_funding_ms: mark["nextFundingTime"].as_u64()?,
    })
}

fn decimal(value: &Value) -> Option<Decimal> {
    Decimal::from_str_exact(value.as_str()?).ok()
}

async fn bulletin() -> Result<Vec<Bulletin>, &'static str> {
    let body = download(FED_URL)
        .await
        .map_err(|_| "No se pudo consultar el boletín monetario de la Reserva Federal.")?;
    decode_bulletins(&body)
        .ok_or("El boletín monetario no contiene titulares en el formato esperado.")
}

fn decode_bulletins(body: &[u8]) -> Option<Vec<Bulletin>> {
    let mut reader = Reader::from_reader(body);
    reader.config_mut().trim_text(true);
    let mut in_item = false;
    let mut field = None;
    let mut title = String::new();
    let mut published = String::new();
    let mut records = Vec::new();
    loop {
        match reader.read_event().ok()? {
            Event::DocType(_) => return None,
            Event::Start(tag) => match tag.name().as_ref() {
                "item" => {
                    in_item = true;
                    title.clear();
                    published.clear();
                }
                "title" if in_item => field = Some("title"),
                "pubDate" if in_item => field = Some("published"),
                _ => {}
            },
            Event::Text(text) => {
                if let Some(selected) = field {
                    let value = if selected == "title" {
                        &mut title
                    } else {
                        &mut published
                    };
                    let decoded = text.xml10_content();
                    value.extend(
                        quick_xml::escape::unescape(&decoded)
                            .ok()?
                            .chars()
                            .take(320),
                    );
                }
            }
            Event::End(tag) => {
                field = None;
                if tag.name().as_ref() == "item" {
                    in_item = false;
                    if !title.is_empty() && !published.is_empty() {
                        records.push(Bulletin {
                            title: title.clone(),
                            published: published.clone(),
                        });
                        if records.len() == 6 {
                            break;
                        }
                    }
                }
            }
            Event::CData(text) => {
                if let Some(selected) = field {
                    let value = if selected == "title" {
                        &mut title
                    } else {
                        &mut published
                    };
                    value.extend(text.xml10_content().chars().take(320));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    (!records.is_empty()).then_some(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_crossed_levels_and_news_with_external_declarations() {
        let book = br#"{"lastUpdateId":1,"bids":[["100","3"],["99","1"]],"asks":[["101","1"],["102","1"]]}"#;
        let snapshot = decode_book(book).unwrap();
        assert!(snapshot.spread_bps > Decimal::ZERO);
        assert!(snapshot.imbalance > Decimal::ZERO);
        assert!(
            decode_book(br#"{"lastUpdateId":1,"bids":[["102","3"]],"asks":[["101","1"]]}"#)
                .is_none()
        );
        let rss = b"<rss><channel><item><title>Boletin oficial</title><pubDate><![CDATA[Mon, 01 Jan 2024 00:00:00 GMT]]></pubDate></item></channel></rss>";
        assert_eq!(decode_bulletins(rss).unwrap()[0].title, "Boletin oficial");
        assert!(decode_bulletins(b"<!DOCTYPE rss><rss/>").is_none());
    }

    #[test]
    #[ignore = "Consulta profundidad, derivados públicos y el boletín de la Reserva Federal"]
    fn public_context_smoke() {
        tauri::async_runtime::block_on(async {
            let report = fetch("BTC/USDT".parse().unwrap()).await;
            assert!(report.book.is_some(), "{:?}", report.warnings);
            assert!(report.derivatives.is_some(), "{:?}", report.warnings);
            assert!(!report.monetary_news.is_empty(), "{:?}", report.warnings);
        });
    }
}
