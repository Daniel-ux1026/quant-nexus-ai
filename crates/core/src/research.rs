use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::Serialize;

use crate::market::{Candle, validate_series};
use crate::paper::{buy_terms, sell_terms};

const LOOKBACK: usize = 20;
const HORIZON: usize = 5;
const FEATURE_COUNT: usize = 4;
const FOLDS: usize = 3;
const MIN_SAMPLES: usize = 300;
const ITERATIONS: usize = 140;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchReport {
    pub samples: usize,
    pub test_samples: usize,
    pub positive_test_samples: usize,
    pub brier: f64,
    pub baseline_brier: f64,
    pub folds: Vec<ResearchFold>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchFold {
    pub from_ms: u64,
    pub through_ms: u64,
    pub train_samples: usize,
    pub test_samples: usize,
    pub brier: f64,
    pub baseline_brier: f64,
}

#[derive(Clone, Copy)]
struct Sample {
    features: [f64; FEATURE_COUNT],
    positive: f64,
    at: u64,
}

pub fn walk_forward(candles: &[Candle]) -> Option<ResearchReport> {
    validate_series(candles).ok()?;
    if candles.len() < MIN_SAMPLES + LOOKBACK + HORIZON {
        return None;
    }
    let mut samples = Vec::with_capacity(candles.len() - LOOKBACK - HORIZON);
    for i in LOOKBACK..candles.len() - HORIZON {
        let bar = &candles[i];
        let recent = &candles[i - LOOKBACK..i];
        let close = bar.close.to_f64()?;
        let average = recent
            .iter()
            .map(|bar| bar.close.to_f64().unwrap_or_default())
            .sum::<f64>()
            / LOOKBACK as f64;
        let average_volume = recent
            .iter()
            .map(|bar| bar.base_volume.to_f64().unwrap_or_default())
            .sum::<f64>()
            / LOOKBACK as f64;
        let volume = bar.base_volume.to_f64()?;
        let features = [
            close / candles[i - HORIZON].close.to_f64()? - 1.0,
            close / average - 1.0,
            (bar.high.to_f64()? - bar.low.to_f64()?) / close,
            if average_volume > 0.0 {
                (1.0 + volume / average_volume).ln()
            } else {
                0.0
            },
        ];
        if features.iter().any(|feature| !feature.is_finite()) {
            return None;
        }
        let entry = buy_terms(bar.close, Decimal::ONE).ok()?.2;
        // La etiqueta pertenece al cierre cinco barras después. La ventana de entrenamiento
        // termina antes de esa etiqueta para cada bloque de evaluación.
        let outcome = sell_terms(candles[i + HORIZON].close, Decimal::ONE).ok()?.2;
        samples.push(Sample {
            features,
            positive: if outcome > entry { 1.0 } else { 0.0 },
            at: bar.close_time_ms,
        });
    }
    evaluate(&samples)
}

fn evaluate(samples: &[Sample]) -> Option<ResearchReport> {
    let initial = samples.len() / 2;
    let remaining = samples.len() - initial;
    let mut folds = Vec::new();
    let mut loss = 0.0;
    let mut baseline_loss = 0.0;
    let mut test_count = 0;
    let mut positives = 0;
    for fold in 0..FOLDS {
        let test_start = initial + fold * remaining / FOLDS;
        let test_end = initial + (fold + 1) * remaining / FOLDS;
        let train = &samples[..test_start - HORIZON];
        let test = &samples[test_start..test_end];
        let prior = train.iter().map(|row| row.positive).sum::<f64>() / train.len() as f64;
        let model = Model::fit(train, prior);
        let mut fold_loss = 0.0;
        let mut fold_baseline = 0.0;
        for row in test {
            fold_loss += (model.predict(row.features) - row.positive).powi(2);
            fold_baseline += (prior - row.positive).powi(2);
            positives += usize::from(row.positive > 0.0);
        }
        folds.push(ResearchFold {
            from_ms: test.first()?.at,
            through_ms: test.last()?.at,
            train_samples: train.len(),
            test_samples: test.len(),
            brier: fold_loss / test.len() as f64,
            baseline_brier: fold_baseline / test.len() as f64,
        });
        test_count += test.len();
        loss += fold_loss;
        baseline_loss += fold_baseline;
    }
    Some(ResearchReport {
        samples: samples.len(),
        test_samples: test_count,
        positive_test_samples: positives,
        brier: loss / test_count as f64,
        baseline_brier: baseline_loss / test_count as f64,
        folds,
    })
}

struct Model {
    means: [f64; FEATURE_COUNT],
    scales: [f64; FEATURE_COUNT],
    weights: [f64; FEATURE_COUNT],
    bias: f64,
}

impl Model {
    fn fit(samples: &[Sample], prior: f64) -> Self {
        let mut means = [0.0; FEATURE_COUNT];
        let mut scales = [0.0; FEATURE_COUNT];
        for i in 0..FEATURE_COUNT {
            means[i] =
                samples.iter().map(|row| row.features[i]).sum::<f64>() / samples.len() as f64;
            let variance = samples
                .iter()
                .map(|row| (row.features[i] - means[i]).powi(2))
                .sum::<f64>()
                / samples.len() as f64;
            scales[i] = variance.sqrt().max(1e-9);
        }
        let prior = prior.clamp(0.001, 0.999);
        let mut model = Self {
            means,
            scales,
            weights: [0.0; FEATURE_COUNT],
            bias: (prior / (1.0 - prior)).ln(),
        };
        let rate = 0.12;
        for _ in 0..ITERATIONS {
            let mut gradient = [0.0; FEATURE_COUNT];
            let mut bias_gradient = 0.0;
            for row in samples {
                let scaled = model.scale(row.features);
                let error = model.logistic(scaled) - row.positive;
                bias_gradient += error;
                for (value, element) in gradient.iter_mut().zip(scaled) {
                    *value += error * element;
                }
            }
            for (weight, delta) in model.weights.iter_mut().zip(gradient) {
                *weight -= rate * (delta / samples.len() as f64 + 0.01 * *weight);
            }
            model.bias -= rate * bias_gradient / samples.len() as f64;
        }
        model
    }

    fn scale(&self, inputs: [f64; FEATURE_COUNT]) -> [f64; FEATURE_COUNT] {
        std::array::from_fn(|i| ((inputs[i] - self.means[i]) / self.scales[i]).clamp(-12.0, 12.0))
    }

    fn logistic(&self, scaled: [f64; FEATURE_COUNT]) -> f64 {
        let value = self.bias
            + self
                .weights
                .iter()
                .zip(scaled)
                .map(|(weight, feature)| weight * feature)
                .sum::<f64>();
        1.0 / (1.0 + (-value.clamp(-36.0, 36.0)).exp())
    }

    fn predict(&self, inputs: [f64; FEATURE_COUNT]) -> f64 {
        self.logistic(self.scale(inputs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::MINUTE_MS;

    #[test]
    fn validation_blocks_are_ordered_with_an_embargo_and_repeatable_scores() {
        let samples: Vec<_> = (0..360)
            .map(|i| Sample {
                features: [f64::from(i % 10), f64::from(i % 3), 0.0, 1.0],
                positive: if i % 10 > 6 { 1.0 } else { 0.0 },
                at: i as u64 * MINUTE_MS,
            })
            .collect();
        let report = evaluate(&samples).unwrap();
        assert_eq!(report.folds.len(), 3);
        assert_eq!(report.test_samples, 180);
        for fold in &report.folds {
            assert!(fold.train_samples as u64 + HORIZON as u64 <= fold.from_ms / MINUTE_MS);
            assert!(fold.through_ms >= fold.from_ms);
        }
        assert!(report.brier < report.baseline_brier);
        assert_eq!(report.brier, evaluate(&samples).unwrap().brier);
    }
}
