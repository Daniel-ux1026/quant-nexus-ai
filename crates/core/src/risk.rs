use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

const BASIS_POINTS: u32 = 10_000;
const MAX_RISK_BPS: u32 = 25;
const MINIMUM_REWARD_MULTIPLE: u32 = 2;
const QUANTITY_SCALE: u32 = 8;
const SIMULATED_FEE_BPS: u32 = 10;
const SIMULATED_SLIPPAGE_BPS: u32 = 5;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RiskError {
    #[error(
        "Ingresa números positivos. El stop debe quedar debajo de la entrada y el objetivo encima."
    )]
    InvalidPrices,
    #[error("El riesgo simulado debe estar entre 1 y 25 puntos básicos.")]
    RiskLimit,
    #[error("La comisión y el deslizamiento deben estar entre 0 y 100 puntos básicos por lado.")]
    InvalidCosts,
    #[error("El beneficio estimado después de costos es inferior a dos veces el riesgo.")]
    InsufficientReward,
    #[error(
        "El presupuesto es insuficiente para una cantidad positiva a la precisión de la simulación."
    )]
    InsufficientCapital,
    #[error("Los importes exceden la capacidad del cálculo decimal.")]
    Arithmetic,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PositionInput {
    #[serde(deserialize_with = "exact_decimal")]
    pub equity: Decimal,
    #[serde(deserialize_with = "exact_decimal")]
    pub entry: Decimal,
    #[serde(deserialize_with = "exact_decimal")]
    pub stop: Decimal,
    #[serde(deserialize_with = "exact_decimal")]
    pub target: Decimal,
    pub risk_bps: u32,
    pub fee_bps_per_side: u32,
    pub slippage_bps_per_side: u32,
}

fn exact_decimal<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
    let value = String::deserialize(deserializer)?;
    Decimal::from_str_exact(&value).map_err(|_| de::Error::custom("Importe decimal inválido"))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionEstimate {
    pub quantity: Decimal,
    pub entry_cost: Decimal,
    pub risk_budget: Decimal,
    pub estimated_loss: Decimal,
    pub estimated_reward: Decimal,
    pub reward_to_risk: Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionScenario {
    pub entry: Decimal,
    pub stop: Decimal,
    pub target: Decimal,
    pub estimate: PositionEstimate,
}

pub fn estimate_automated_spot_position(
    equity: Decimal,
    entry: Decimal,
    stop: Decimal,
) -> Result<PositionScenario, RiskError> {
    let (buy, sell) = cost_multipliers(SIMULATED_FEE_BPS, SIMULATED_SLIPPAGE_BPS)?;
    let entry_cost = entry.checked_mul(buy).ok_or(RiskError::Arithmetic)?;
    let stop_proceeds = stop.checked_mul(sell).ok_or(RiskError::Arithmetic)?;
    let unit_risk = entry_cost
        .checked_sub(stop_proceeds)
        .ok_or(RiskError::Arithmetic)?;
    let desired_reward = unit_risk
        .checked_mul(Decimal::new(225, 2))
        .ok_or(RiskError::Arithmetic)?;
    let target = entry_cost
        .checked_add(desired_reward)
        .and_then(|amount| amount.checked_div(sell))
        .ok_or(RiskError::Arithmetic)?;
    let estimate = estimate_spot_position(&PositionInput {
        equity,
        entry,
        stop,
        target,
        risk_bps: MAX_RISK_BPS,
        fee_bps_per_side: SIMULATED_FEE_BPS,
        slippage_bps_per_side: SIMULATED_SLIPPAGE_BPS,
    })?;
    Ok(PositionScenario {
        entry,
        stop,
        target,
        estimate,
    })
}

fn cost_multipliers(fee_bps: u32, slippage_bps: u32) -> Result<(Decimal, Decimal), RiskError> {
    if fee_bps > 100 || slippage_bps > 100 {
        return Err(RiskError::InvalidCosts);
    }
    let divisor = Decimal::from(BASIS_POINTS);
    let fee = Decimal::from(fee_bps) / divisor;
    let slippage = Decimal::from(slippage_bps) / divisor;
    Ok((
        (Decimal::ONE + slippage) * (Decimal::ONE + fee),
        (Decimal::ONE - slippage) * (Decimal::ONE - fee),
    ))
}

pub fn estimate_spot_position(input: &PositionInput) -> Result<PositionEstimate, RiskError> {
    if !(Decimal::ZERO < input.stop
        && input.stop < input.entry
        && input.entry < input.target
        && input.equity > Decimal::ZERO)
    {
        return Err(RiskError::InvalidPrices);
    }
    if !(1..=MAX_RISK_BPS).contains(&input.risk_bps) {
        return Err(RiskError::RiskLimit);
    }
    let (buy_multiplier, sell_multiplier) =
        cost_multipliers(input.fee_bps_per_side, input.slippage_bps_per_side)?;

    let cost_per_unit = input
        .entry
        .checked_mul(buy_multiplier)
        .ok_or(RiskError::Arithmetic)?;
    let stop_proceeds = input
        .stop
        .checked_mul(sell_multiplier)
        .ok_or(RiskError::Arithmetic)?;
    let target_proceeds = input
        .target
        .checked_mul(sell_multiplier)
        .ok_or(RiskError::Arithmetic)?;
    let risk_per_unit = cost_per_unit
        .checked_sub(stop_proceeds)
        .ok_or(RiskError::Arithmetic)?;
    let reward_per_unit = target_proceeds
        .checked_sub(cost_per_unit)
        .ok_or(RiskError::Arithmetic)?;

    if risk_per_unit <= Decimal::ZERO {
        return Err(RiskError::Arithmetic);
    }
    let minimum_reward = risk_per_unit
        .checked_mul(Decimal::from(MINIMUM_REWARD_MULTIPLE))
        .ok_or(RiskError::Arithmetic)?;
    if reward_per_unit < minimum_reward {
        return Err(RiskError::InsufficientReward);
    }

    let risk_budget = input
        .equity
        .checked_mul(Decimal::from(input.risk_bps) / Decimal::from(BASIS_POINTS))
        .ok_or(RiskError::Arithmetic)?;
    let risk_quantity = risk_budget
        .checked_div(risk_per_unit)
        .ok_or(RiskError::Arithmetic)?;
    let affordable_quantity = input
        .equity
        .checked_div(cost_per_unit)
        .ok_or(RiskError::Arithmetic)?;
    let quantity = risk_quantity
        .min(affordable_quantity)
        .round_dp_with_strategy(QUANTITY_SCALE, RoundingStrategy::ToZero);
    if quantity.is_zero() {
        return Err(RiskError::InsufficientCapital);
    }

    let amount = |per_unit| quantity.checked_mul(per_unit).ok_or(RiskError::Arithmetic);
    let entry_cost = amount(cost_per_unit)?;
    let estimated_loss = amount(risk_per_unit)?;
    if entry_cost > input.equity || estimated_loss > risk_budget {
        return Err(RiskError::Arithmetic);
    }

    Ok(PositionEstimate {
        quantity,
        entry_cost,
        risk_budget,
        estimated_loss,
        estimated_reward: amount(reward_per_unit)?,
        reward_to_risk: reward_per_unit
            .checked_div(risk_per_unit)
            .ok_or(RiskError::Arithmetic)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulation() -> PositionInput {
        PositionInput {
            equity: Decimal::from(1_000),
            entry: Decimal::from(100),
            stop: Decimal::from(98),
            target: Decimal::from(106),
            risk_bps: 25,
            fee_bps_per_side: 10,
            slippage_bps_per_side: 5,
        }
    }

    #[test]
    fn sizes_down_with_costs_and_respects_the_budget() {
        let result = estimate_spot_position(&simulation()).unwrap();
        assert_eq!(result.risk_budget, Decimal::new(250, 2));
        assert!(result.estimated_loss <= result.risk_budget);
        assert!(result.entry_cost <= simulation().equity);
        assert!(result.reward_to_risk > Decimal::from(2));
        assert!(result.quantity < Decimal::new(125, 2));
    }

    #[test]
    fn caps_cash_spend_and_rounds_down() {
        let input = PositionInput {
            stop: Decimal::new(99_999, 3),
            target: Decimal::from(101),
            fee_bps_per_side: 0,
            slippage_bps_per_side: 0,
            ..simulation()
        };
        let result = estimate_spot_position(&input).unwrap();
        assert_eq!(result.quantity, Decimal::from(10));
        assert_eq!(result.entry_cost, input.equity);
    }

    #[test]
    fn blocks_invalid_stop_risk_and_post_cost_reward() {
        let mut input = simulation();
        input.stop = input.entry;
        assert_eq!(
            estimate_spot_position(&input),
            Err(RiskError::InvalidPrices)
        );
        input = simulation();
        input.risk_bps = 26;
        assert_eq!(estimate_spot_position(&input), Err(RiskError::RiskLimit));
        input = simulation();
        input.target = Decimal::from(104);
        assert_eq!(
            estimate_spot_position(&input),
            Err(RiskError::InsufficientReward)
        );
    }

    #[test]
    fn handles_extreme_values_as_errors() {
        let input = PositionInput {
            target: Decimal::MAX,
            ..simulation()
        };
        assert_eq!(estimate_spot_position(&input), Err(RiskError::Arithmetic));
        let input = PositionInput {
            equity: Decimal::new(1, 28),
            ..simulation()
        };
        assert_eq!(
            estimate_spot_position(&input),
            Err(RiskError::InsufficientCapital)
        );
    }
}
