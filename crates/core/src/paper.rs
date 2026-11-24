use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::market::{Candle, SpotPair};
use crate::risk::{PositionInput, PositionScenario, estimate_spot_position};

const DAY_MS: u64 = 86_400_000;
const FEE: Decimal = Decimal::from_parts(1, 0, 0, false, 3);
const SLIPPAGE: Decimal = Decimal::from_parts(5, 0, 0, false, 4);
const MAX_ASSET: Decimal = Decimal::from_parts(30, 0, 0, false, 2);
const MAX_PORTFOLIO: Decimal = Decimal::from_parts(75, 0, 0, false, 2);
const DAILY_FLOOR: Decimal = Decimal::from_parts(99, 0, 0, false, 2);
const WEEKLY_FLOOR: Decimal = Decimal::from_parts(97, 0, 0, false, 2);
const DRAWDOWN_FLOOR: Decimal = Decimal::from_parts(95, 0, 0, false, 2);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PaperError {
    #[error("El capital simulado debe ser mayor que cero.")]
    InvalidCapital,
    #[error("Faltan velas consecutivas sincronizadas para la cartera simulada.")]
    Discontinuous,
    #[error("El conjunto de pares de la cartera es inválido.")]
    InvalidUniverse,
    #[error("El cálculo de cartera excede la precisión disponible.")]
    Arithmetic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperAction {
    Entry,
    Stop,
    Target,
    Emergency,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperEvent {
    pub sequence: u64,
    pub time_ms: u64,
    pub pair: String,
    pub action: PaperAction,
    pub price: Option<Decimal>,
    pub quantity: Option<Decimal>,
    pub fee: Option<Decimal>,
    pub pnl: Option<Decimal>,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperPosition {
    pub quantity: Decimal,
    pub entry: Decimal,
    pub stop: Decimal,
    pub target: Decimal,
    pub cost: Decimal,
    pub mark: Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Pending {
    stop: Decimal,
    target: Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperAccount {
    universe: BTreeSet<String>,
    pub starting_capital: Decimal,
    pub cash: Decimal,
    pub equity: Decimal,
    pub peak_equity: Decimal,
    pub max_drawdown: Decimal,
    pub positions: BTreeMap<String, PaperPosition>,
    pending: BTreeMap<String, Pending>,
    pub last_close_ms: Option<u64>,
    pub sequence: u64,
    pub fees: Decimal,
    pub wins: u64,
    pub losses: u64,
    pub flat_exits: u64,
    pub gross_profit: Decimal,
    pub gross_loss: Decimal,
    pub blocked: u64,
    day: Option<u64>,
    week: Option<u64>,
    day_equity: Decimal,
    week_equity: Decimal,
    day_locked: bool,
    week_locked: bool,
    drawdown_locked: bool,
}

pub struct PaperBar<'a> {
    pub pair: &'a str,
    pub candle: &'a Candle,
    pub signal: Option<&'a PositionScenario>,
}

impl PaperAccount {
    pub fn new(capital: Decimal, pairs: &[&str]) -> Result<Self, PaperError> {
        if capital <= Decimal::ZERO {
            return Err(PaperError::InvalidCapital);
        }
        let universe: BTreeSet<_> = pairs.iter().map(|pair| (*pair).to_owned()).collect();
        if !(1..=3).contains(&universe.len())
            || universe.len() != pairs.len()
            || universe
                .iter()
                .any(|pair| pair.parse::<SpotPair>().is_err())
        {
            return Err(PaperError::InvalidUniverse);
        }
        Ok(Self {
            universe,
            starting_capital: capital,
            cash: capital,
            equity: capital,
            peak_equity: capital,
            max_drawdown: Decimal::ZERO,
            positions: BTreeMap::new(),
            pending: BTreeMap::new(),
            last_close_ms: None,
            sequence: 0,
            fees: Decimal::ZERO,
            wins: 0,
            losses: 0,
            flat_exits: 0,
            gross_profit: Decimal::ZERO,
            gross_loss: Decimal::ZERO,
            blocked: 0,
            day: None,
            week: None,
            day_equity: capital,
            week_equity: capital,
            day_locked: false,
            week_locked: false,
            drawdown_locked: false,
        })
    }

    pub fn advance(
        &mut self,
        bars: &[PaperBar<'_>],
        entries_paused: bool,
    ) -> Result<Vec<PaperEvent>, PaperError> {
        self.advance_controlled(bars, entries_paused, false)
    }

    pub fn advance_controlled(
        &mut self,
        bars: &[PaperBar<'_>],
        entries_paused: bool,
        liquidate_at_close: bool,
    ) -> Result<Vec<PaperEvent>, PaperError> {
        let first = bars.first().ok_or(PaperError::Discontinuous)?.candle;
        let received: BTreeSet<_> = bars.iter().map(|bar| bar.pair).collect();
        if bars.len() != self.universe.len()
            || !received
                .iter()
                .copied()
                .eq(self.universe.iter().map(String::as_str))
            || bars.iter().any(|bar| {
                bar.candle.open_time_ms != first.open_time_ms || bar.candle.validate().is_err()
            })
        {
            return Err(PaperError::Discontinuous);
        }
        if let Some(last) = self.last_close_ms {
            if first.close_time_ms <= last {
                return Ok(Vec::new());
            }
            if last.checked_add(1) != Some(first.open_time_ms) {
                return Err(PaperError::Discontinuous);
            }
        }

        let mut ordered: Vec<_> = bars.iter().collect();
        ordered.sort_unstable_by_key(|item| item.pair);
        let mut next = self.clone();
        let events = next.advance_batch(
            &ordered,
            entries_paused || liquidate_at_close,
            liquidate_at_close,
        )?;
        *self = next;
        Ok(events)
    }

    fn advance_batch(
        &mut self,
        bars: &[&PaperBar<'_>],
        paused: bool,
        liquidate_at_close: bool,
    ) -> Result<Vec<PaperEvent>, PaperError> {
        let first = bars[0].candle;
        self.roll_periods(first.open_time_ms);
        let mut events = Vec::new();

        for bar in bars {
            if let Some(position) = self.positions.get_mut(bar.pair) {
                position.mark = bar.candle.open;
                let opening_exit = if bar.candle.open <= position.stop {
                    Some((PaperAction::Stop, bar.candle.open))
                } else if bar.candle.open >= position.target {
                    Some((PaperAction::Target, position.target))
                } else {
                    None
                };
                if let Some((action, reference)) = opening_exit {
                    events.push(self.exit(bar.pair, reference, first.open_time_ms, action)?);
                }
            }
        }

        self.update_locks()?;
        let pending = std::mem::take(&mut self.pending);
        for bar in bars {
            let Some(order) = pending.get(bar.pair) else {
                continue;
            };
            let blocked = if paused {
                Some("Pausa de nuevas entradas.")
            } else {
                self.circuit()?
            };
            let event = match blocked {
                Some(reason) => self.reject(bar.pair, first.open_time_ms, reason)?,
                None => self.enter(bar.pair, order, bar.candle.open, first.open_time_ms)?,
            };
            events.push(event);
        }

        for bar in bars {
            let Some(position) = self.positions.get_mut(bar.pair) else {
                continue;
            };
            let trigger = if bar.candle.low <= position.stop {
                Some((PaperAction::Stop, position.stop))
            } else if bar.candle.high >= position.target {
                Some((PaperAction::Target, position.target))
            } else {
                None
            };
            if let Some((action, price)) = trigger {
                events.push(self.exit(bar.pair, price, first.close_time_ms, action)?);
            } else if liquidate_at_close {
                events.push(self.exit(
                    bar.pair,
                    bar.candle.close,
                    first.close_time_ms,
                    PaperAction::Emergency,
                )?);
            } else {
                position.mark = bar.candle.close;
            }
        }

        self.equity = self.marked_equity()?;
        self.update_locks()?;
        self.peak_equity = self.peak_equity.max(self.equity);
        let drawdown = exact(self.peak_equity.checked_sub(self.equity))?;
        self.max_drawdown = self
            .max_drawdown
            .max(exact(drawdown.checked_div(self.peak_equity))?);
        self.last_close_ms = Some(first.close_time_ms);

        for bar in bars {
            let Some(signal) = bar.signal else { continue };
            if self.positions.contains_key(bar.pair) {
                continue;
            }
            let blocked = if paused {
                Some("Pausa de nuevas entradas.")
            } else {
                self.circuit()?
            };
            if let Some(reason) = blocked {
                events.push(self.reject(bar.pair, first.close_time_ms, reason)?);
            } else if signal.entry == bar.candle.close
                && Decimal::ZERO < signal.stop
                && signal.stop < signal.entry
                && signal.entry < signal.target
            {
                self.pending.insert(
                    bar.pair.into(),
                    Pending {
                        stop: signal.stop,
                        target: signal.target,
                    },
                );
            } else {
                events.push(self.reject(
                    bar.pair,
                    first.close_time_ms,
                    "La señal no coincide con la vela cerrada.",
                )?);
            }
        }
        Ok(events)
    }

    fn roll_periods(&mut self, at: u64) {
        let day = at / DAY_MS;
        let week = (day + 3) / 7;
        if self.day != Some(day) {
            self.day = Some(day);
            self.day_equity = self.equity;
            self.day_locked = false;
        }
        if self.week != Some(week) {
            self.week = Some(week);
            self.week_equity = self.equity;
            self.week_locked = false;
        }
    }

    fn update_locks(&mut self) -> Result<(), PaperError> {
        let equity = self.marked_equity()?;
        self.drawdown_locked |= equity <= exact(self.peak_equity.checked_mul(DRAWDOWN_FLOOR))?;
        self.day_locked |= equity <= exact(self.day_equity.checked_mul(DAILY_FLOOR))?;
        self.week_locked |= equity <= exact(self.week_equity.checked_mul(WEEKLY_FLOOR))?;
        Ok(())
    }

    pub fn circuit(&self) -> Result<Option<&'static str>, PaperError> {
        let equity = self.marked_equity()?;
        if self.drawdown_locked || equity <= exact(self.peak_equity.checked_mul(DRAWDOWN_FLOOR))? {
            return Ok(Some("Se alcanzó el límite simulado de drawdown del 5%."));
        }
        if self.day_locked || equity <= exact(self.day_equity.checked_mul(DAILY_FLOOR))? {
            return Ok(Some(
                "Se alcanzó el límite simulado de pérdida diaria del 1%.",
            ));
        }
        if self.week_locked || equity <= exact(self.week_equity.checked_mul(WEEKLY_FLOOR))? {
            return Ok(Some(
                "Se alcanzó el límite simulado de pérdida semanal del 3%.",
            ));
        }
        Ok(None)
    }

    fn marked_equity(&self) -> Result<Decimal, PaperError> {
        self.positions
            .values()
            .try_fold(self.cash, |total, position| {
                let proceeds = sell_terms(position.mark, position.quantity)?.2;
                exact(total.checked_add(proceeds))
            })
    }

    fn enter(
        &mut self,
        pair: &str,
        order: &Pending,
        opening: Decimal,
        time: u64,
    ) -> Result<PaperEvent, PaperError> {
        if self.positions.contains_key(pair) || self.positions.len() >= 3 {
            return self.reject(pair, time, "La cartera ya tiene la exposición permitida.");
        }
        let equity = self.marked_equity()?;
        let plan = estimate_spot_position(&PositionInput {
            equity,
            entry: opening,
            stop: order.stop,
            target: order.target,
            risk_bps: 25,
            fee_bps_per_side: 10,
            slippage_bps_per_side: 5,
        });
        let Ok(plan) = plan else {
            return self.reject(
                pair,
                time,
                "La apertura ya no cumple el riesgo o el objetivo después de costos.",
            );
        };

        let existing = self
            .positions
            .values()
            .try_fold(Decimal::ZERO, |total, position| {
                let exposure = exact(position.mark.checked_mul(position.quantity))?;
                exact(total.checked_add(exposure))
            })?;
        let portfolio_room = exact(equity.checked_mul(MAX_PORTFOLIO))?
            .checked_sub(existing)
            .ok_or(PaperError::Arithmetic)?
            .max(Decimal::ZERO);
        let budget = self
            .cash
            .min(exact(equity.checked_mul(MAX_ASSET))?)
            .min(portfolio_room);
        let unit_cost = buy_terms(opening, Decimal::ONE)?.2;
        let quantity = plan
            .quantity
            .min(exact(budget.checked_div(unit_cost))?)
            .round_dp_with_strategy(8, RoundingStrategy::ToZero);
        if quantity <= Decimal::ZERO {
            return self.reject(
                pair,
                time,
                "Saldo insuficiente o límite de exposición alcanzado.",
            );
        }

        let (price, fee, cost) = buy_terms(opening, quantity)?;
        if cost > self.cash || cost > budget || cost <= Decimal::ZERO {
            return self.reject(
                pair,
                time,
                "El tamaño redondeado excede el presupuesto de la cartera.",
            );
        }
        self.cash = exact(self.cash.checked_sub(cost))?;
        self.fees = exact(self.fees.checked_add(fee))?;
        self.positions.insert(
            pair.into(),
            PaperPosition {
                quantity,
                entry: price,
                stop: order.stop,
                target: order.target,
                cost,
                mark: opening,
            },
        );
        let mut event = self.event(
            pair,
            time,
            PaperAction::Entry,
            "Entrada simulada en la apertura siguiente.",
        )?;
        event.price = Some(price);
        event.quantity = Some(quantity);
        event.fee = Some(fee);
        Ok(event)
    }

    fn exit(
        &mut self,
        pair: &str,
        reference: Decimal,
        time: u64,
        action: PaperAction,
    ) -> Result<PaperEvent, PaperError> {
        let position = self
            .positions
            .remove(pair)
            .ok_or(PaperError::Discontinuous)?;
        let (price, fee, proceeds) = sell_terms(reference, position.quantity)?;
        let profit = exact(proceeds.checked_sub(position.cost))?;
        self.cash = exact(self.cash.checked_add(proceeds))?;
        self.fees = exact(self.fees.checked_add(fee))?;
        match profit.cmp(&Decimal::ZERO) {
            std::cmp::Ordering::Greater => {
                self.wins += 1;
                self.gross_profit = exact(self.gross_profit.checked_add(profit))?;
            }
            std::cmp::Ordering::Less => {
                self.losses += 1;
                self.gross_loss = exact(self.gross_loss.checked_add(profit.abs()))?;
            }
            std::cmp::Ordering::Equal => self.flat_exits += 1,
        }
        let detail = match action {
            PaperAction::Stop => "Salida simulada por stop.",
            PaperAction::Target => "Salida simulada por objetivo.",
            _ => "Cierre de emergencia simulado al cierre de la vela.",
        };
        let mut event = self.event(pair, time, action, detail)?;
        event.price = Some(price);
        event.quantity = Some(position.quantity);
        event.fee = Some(fee);
        event.pnl = Some(profit);
        Ok(event)
    }

    fn reject(&mut self, pair: &str, time: u64, detail: &str) -> Result<PaperEvent, PaperError> {
        self.blocked += 1;
        self.event(pair, time, PaperAction::Blocked, detail)
    }

    fn event(
        &mut self,
        pair: &str,
        time_ms: u64,
        action: PaperAction,
        detail: &str,
    ) -> Result<PaperEvent, PaperError> {
        self.sequence = self.sequence.checked_add(1).ok_or(PaperError::Arithmetic)?;
        Ok(PaperEvent {
            sequence: self.sequence,
            time_ms,
            pair: pair.into(),
            action,
            detail: detail.into(),
            price: None,
            quantity: None,
            fee: None,
            pnl: None,
        })
    }
}

pub fn buy_terms(
    reference: Decimal,
    quantity: Decimal,
) -> Result<(Decimal, Decimal, Decimal), PaperError> {
    let price = exact(reference.checked_mul(Decimal::ONE + SLIPPAGE))?;
    let notional = exact(price.checked_mul(quantity))?;
    let fee = exact(notional.checked_mul(FEE))?;
    Ok((price, fee, exact(notional.checked_add(fee))?))
}

pub fn sell_terms(
    reference: Decimal,
    quantity: Decimal,
) -> Result<(Decimal, Decimal, Decimal), PaperError> {
    let price = exact(reference.checked_mul(Decimal::ONE - SLIPPAGE))?;
    let notional = exact(price.checked_mul(quantity))?;
    let fee = exact(notional.checked_mul(FEE))?;
    Ok((price, fee, exact(notional.checked_sub(fee))?))
}

fn exact(value: Option<Decimal>) -> Result<Decimal, PaperError> {
    value.ok_or(PaperError::Arithmetic)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::MINUTE_MS;
    use crate::risk::estimate_automated_spot_position;

    fn bar(minute: u64, open: i64, high: i64, low: i64, close: i64) -> Candle {
        Candle {
            open_time_ms: minute * MINUTE_MS,
            close_time_ms: (minute + 1) * MINUTE_MS - 1,
            open: open.into(),
            high: high.into(),
            low: low.into(),
            close: close.into(),
            base_volume: 10.into(),
        }
    }

    fn step(
        state: &mut PaperAccount,
        candle: &Candle,
        signal: Option<&PositionScenario>,
    ) -> Result<Vec<PaperEvent>, PaperError> {
        state.advance(
            &[PaperBar {
                pair: "BTC/USDT",
                candle,
                signal,
            }],
            false,
        )
    }

    #[test]
    fn fills_on_the_next_open_and_prefers_stop_when_both_levels_are_touched() {
        let capital = Decimal::from(1000);
        let mut account = PaperAccount::new(capital, &["BTC/USDT"]).unwrap();
        let setup = estimate_automated_spot_position(capital, 100.into(), 98.into()).unwrap();
        let first = bar(1, 100, 101, 99, 100);
        assert!(step(&mut account, &first, Some(&setup)).unwrap().is_empty());
        assert_eq!(account.cash, capital);
        let second = bar(2, 100, 110, 96, 101);
        let events = step(&mut account, &second, None).unwrap();
        assert_eq!(
            events.iter().map(|event| event.action).collect::<Vec<_>>(),
            [PaperAction::Entry, PaperAction::Stop]
        );
        assert_eq!(events[0].time_ms, second.open_time_ms);
        assert!(events[1].pnl.unwrap().is_sign_negative());
        assert_eq!(account.cash, capital + events[1].pnl.unwrap());
        assert!(account.positions.is_empty());
        assert!(step(&mut account, &second, None).unwrap().is_empty());
        assert_eq!(account.sequence, 2);
    }

    #[test]
    fn gaps_fail_atomically_and_opening_below_stop_uses_opening_price() {
        let mut account = PaperAccount::new(1000.into(), &["BTC/USDT"]).unwrap();
        let setup = estimate_automated_spot_position(1000.into(), 100.into(), 98.into()).unwrap();
        step(&mut account, &bar(1, 100, 101, 99, 100), Some(&setup)).unwrap();
        step(&mut account, &bar(2, 100, 101, 99, 100), None).unwrap();
        let checkpoint = account.clone();
        assert_eq!(
            step(&mut account, &bar(4, 90, 95, 88, 92), None),
            Err(PaperError::Discontinuous)
        );
        assert_eq!(account, checkpoint);
        let exit = step(&mut account, &bar(3, 90, 95, 88, 92), None)
            .unwrap()
            .remove(0);
        assert_eq!(exit.action, PaperAction::Stop);
        assert!(exit.price.unwrap() < 90.into());
    }

    #[test]
    fn pause_and_daily_limit_block_entries() {
        let capital = Decimal::from(1000);
        let mut account = PaperAccount::new(capital, &["BTC/USDT"]).unwrap();
        let setup = estimate_automated_spot_position(capital, 100.into(), 98.into()).unwrap();
        step(&mut account, &bar(1, 100, 101, 99, 100), Some(&setup)).unwrap();
        let next = bar(2, 100, 101, 99, 100);
        let events = account
            .advance(
                &[PaperBar {
                    pair: "BTC/USDT",
                    candle: &next,
                    signal: None,
                }],
                true,
            )
            .unwrap();
        assert_eq!(events[0].action, PaperAction::Blocked);
        assert_eq!(account.cash, capital);
        account.cash = 989.into();
        assert!(account.circuit().unwrap().unwrap().contains("diaria"));
    }

    #[test]
    fn emergency_liquidation_closes_at_the_observed_close_without_new_entries() {
        let mut account = PaperAccount::new(1000.into(), &["BTC/USDT"]).unwrap();
        let setup = estimate_automated_spot_position(1000.into(), 100.into(), 98.into()).unwrap();
        step(&mut account, &bar(1, 100, 101, 99, 100), Some(&setup)).unwrap();
        step(&mut account, &bar(2, 100, 101, 99, 100), None).unwrap();
        let closing = bar(3, 100, 101, 99, 100);
        let event = account
            .advance_controlled(
                &[PaperBar {
                    pair: "BTC/USDT",
                    candle: &closing,
                    signal: Some(&setup),
                }],
                true,
                true,
            )
            .unwrap();
        assert_eq!(event[0].action, PaperAction::Emergency);
        assert_eq!(event[0].time_ms, closing.close_time_ms);
        assert!(account.positions.is_empty());
        assert!(
            step(&mut account, &bar(4, 100, 101, 99, 100), None)
                .unwrap()
                .is_empty()
        );
    }
}
