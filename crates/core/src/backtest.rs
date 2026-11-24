use rust_decimal::{Decimal, RoundingStrategy, prelude::ToPrimitive};
use serde::Serialize;

use crate::market::Candle;
use crate::paper::{PaperAccount, PaperBar, PaperError, PaperEvent, buy_terms, sell_terms};
use crate::scanner::ScanObservation;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EquityPoint {
    pub close_time_ms: u64,
    pub equity: Decimal,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestReport {
    pub account: PaperAccount,
    pub net_pnl: Decimal,
    pub return_ratio: Decimal,
    pub buy_and_hold_return: Decimal,
    pub win_rate: Option<Decimal>,
    pub profit_factor: Option<Decimal>,
    pub expectancy: Option<Decimal>,
    pub sharpe_per_bar: Option<f64>,
    pub sortino_per_bar: Option<f64>,
    pub equity_curve: Vec<EquityPoint>,
    pub events: Vec<PaperEvent>,
}

pub fn backtest(
    pair: &str,
    capital: Decimal,
    candles: &[Candle],
    observations: &[ScanObservation],
) -> Result<BacktestReport, PaperError> {
    if candles.len() != observations.len()
        || candles.iter().zip(observations).any(|(bar, observation)| {
            bar.close_time_ms != observation.close_time_ms || bar.close != observation.close
        })
    {
        return Err(PaperError::Discontinuous);
    }
    let first = candles.first().ok_or(PaperError::Discontinuous)?;
    let last = candles.last().ok_or(PaperError::Discontinuous)?;
    let mut account = PaperAccount::new(capital, &[pair])?;
    let mut equity_curve = Vec::with_capacity(candles.len());
    let mut events = Vec::new();
    let mut returns = Vec::new();
    for (bar, observation) in candles.iter().zip(observations) {
        let previous_equity = account.equity;
        events.extend(account.advance(
            &[PaperBar {
                pair,
                candle: bar,
                signal: observation.scenario.as_ref(),
            }],
            false,
        )?);
        equity_curve.push(EquityPoint {
            close_time_ms: bar.close_time_ms,
            equity: account.equity,
        });
        if let Some(value) = ratio_change(previous_equity, account.equity)
            .ok()
            .and_then(|value| value.to_f64())
        {
            returns.push(value);
        }
    }

    let closed = Decimal::from(account.wins + account.losses + account.flat_exits);
    let realized = account
        .gross_profit
        .checked_sub(account.gross_loss)
        .ok_or(PaperError::Arithmetic)?;
    let net_pnl = account
        .equity
        .checked_sub(capital)
        .ok_or(PaperError::Arithmetic)?;
    let return_ratio = ratio_change(capital, account.equity)?;

    let unit_cost = buy_terms(first.open, Decimal::ONE)?.2;
    let benchmark_qty = capital
        .checked_div(unit_cost)
        .ok_or(PaperError::Arithmetic)?
        .round_dp_with_strategy(8, RoundingStrategy::ToZero);
    let benchmark_entry = buy_terms(first.open, benchmark_qty)?.2;
    let benchmark_exit = sell_terms(last.close, benchmark_qty)?.2;
    let benchmark_final = capital
        .checked_sub(benchmark_entry)
        .and_then(|unused| unused.checked_add(benchmark_exit))
        .ok_or(PaperError::Arithmetic)?;
    let buy_and_hold_return = ratio_change(capital, benchmark_final)?;
    let (sharpe_per_bar, sortino_per_bar) = return_statistics(&returns);
    let win_rate = Decimal::from(account.wins).checked_div(closed);
    let profit_factor = account.gross_profit.checked_div(account.gross_loss);
    let expectancy = realized.checked_div(closed);

    Ok(BacktestReport {
        account,
        net_pnl,
        return_ratio,
        buy_and_hold_return,
        win_rate,
        profit_factor,
        expectancy,
        sharpe_per_bar,
        sortino_per_bar,
        equity_curve,
        events,
    })
}

fn ratio_change(before: Decimal, after: Decimal) -> Result<Decimal, PaperError> {
    after
        .checked_sub(before)
        .and_then(|difference| difference.checked_div(before))
        .ok_or(PaperError::Arithmetic)
}

fn return_statistics(returns: &[f64]) -> (Option<f64>, Option<f64>) {
    if returns.len() < 2 {
        return (None, None);
    }
    let count = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / count;
    let total_variance: f64 = returns.iter().map(|value| (value - mean).powi(2)).sum();
    let downside: f64 = returns.iter().map(|value| value.min(0.0).powi(2)).sum();
    let ratio = |deviation: f64| {
        (deviation > 0.0)
            .then(|| mean / deviation)
            .filter(|value| value.is_finite())
    };
    (
        ratio((total_variance / (count - 1.0)).sqrt()),
        ratio((downside / count).sqrt()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{market::synthetic_snapshot, scanner::replay};

    #[test]
    fn backtest_is_repeatable_and_marks_the_ending_balance() {
        let fixture = synthetic_snapshot().unwrap();
        let capital = Decimal::from(1000);
        let decisions = replay(&fixture.candles, capital).unwrap();
        let first = backtest(&fixture.pair, capital, &fixture.candles, &decisions).unwrap();
        let second = backtest(&fixture.pair, capital, &fixture.candles, &decisions).unwrap();
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&second).unwrap()
        );
        assert_eq!(first.equity_curve.len(), fixture.candles.len());
        assert_eq!(first.net_pnl, first.account.equity - capital);
        assert!(first.buy_and_hold_return > Decimal::ZERO);
    }
}
