import { formatDecimal } from "./format";

export const watchedPairs = ["BTC/USDT", "ETH/USDT", "SOL/USDT"] as const;

type Regime = "unknown" | "bullish" | "bearish" | "sideways";
type Reason = "warming_up" | "no_uptrend" | "no_breakout" | "low_volume" |
  "flat_range" | "risk_rejected" | "breakout_with_volume";

type Evidence = {
  smaFast: string;
  smaSlow: string;
  atr: string;
  relativeVolume: string | null;
  priorHigh: string;
};

type Scenario = {
  entry: string;
  stop: string;
  target: string;
  estimate: {
    quantity: string;
    entryCost: string;
    riskBudget: string;
    estimatedLoss: string;
    estimatedReward: string;
    rewardToRisk: string;
  };
};

export type Observation = {
  closeTimeMs: number;
  close: string;
  regime: Regime;
  reason: Reason;
  evidence: Evidence | null;
  scenario: Scenario | null;
  riskRejection: string | null;
};

const regimes: Record<Regime, string> = {
  unknown: "Sin tendencia calculada",
  bullish: "Tendencia alcista",
  bearish: "Tendencia bajista",
  sideways: "Mercado lateral",
};
const reasons: Record<Reason, string> = {
  warming_up: "El análisis comienza al reunir 21 velas consecutivas.",
  no_uptrend: "La media de 9 velas y el cierre todavía no confirman una tendencia alcista.",
  no_breakout: "El cierre sigue por debajo o en el máximo de la vela anterior.",
  low_volume: "La ruptura requiere al menos 1.25 veces el volumen medio de las 20 velas anteriores.",
  flat_range: "La amplitud reciente es insuficiente para estimar el stop.",
  risk_rejected: "El cálculo de riesgo descartó el escenario.",
  breakout_with_volume: "El cierre superó el máximo anterior con tendencia alcista y volumen suficiente.",
};
const clock = new Intl.DateTimeFormat("es-PE", {
  day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit",
  hourCycle: "h23", timeZone: "UTC",
});

export const observeAt = (time: number) => `${clock.format(time)} UTC`;

export function ObservationView({ observation }: { observation: Observation }) {
  const evidence = observation.evidence;
  const scenario = observation.scenario;
  return (
    <>
      <div className="observation-summary">
        <span className={scenario ? "state-label state-positive" : "state-label"}>
          {scenario ? "Escenario simulado" : "En espera"}
        </span>
        <span className="observation-regime">{regimes[observation.regime]}</span>
      </div>
      <p className="observation-reason">
        {reasons[observation.reason]} {observation.riskRejection}
      </p>
      {evidence && (
        <details className="observation-details">
          <summary>{scenario ? "Ver cálculos y evidencia" : "Ver indicadores"}</summary>
          <dl className="observation-grid">
            <div><dt>SMA 9</dt><dd>{formatDecimal(evidence.smaFast, 2, 6)}</dd></div>
            <div><dt>SMA 21</dt><dd>{formatDecimal(evidence.smaSlow, 2, 6)}</dd></div>
            <div><dt>Máximo anterior</dt><dd>{formatDecimal(evidence.priorHigh, 2, 6)}</dd></div>
            <div><dt>Rango medio de 14 velas</dt><dd>{formatDecimal(evidence.atr, 2, 6)}</dd></div>
            <div><dt>Volumen relativo</dt><dd>{evidence.relativeVolume
              ? `${formatDecimal(evidence.relativeVolume, 2, 4)}×`
              : "Sin referencia"}</dd></div>
            {scenario && (
              <>
                <div><dt>Entrada de referencia</dt><dd>{formatDecimal(scenario.entry, 2, 6)} USDT</dd></div>
                <div><dt>Stop</dt><dd>{formatDecimal(scenario.stop, 2, 6)} USDT</dd></div>
                <div><dt>Objetivo</dt><dd>{formatDecimal(scenario.target, 2, 6)} USDT</dd></div>
                <div><dt>Cantidad</dt><dd>{formatDecimal(scenario.estimate.quantity)}</dd></div>
                <div><dt>Costo estimado</dt><dd>{formatDecimal(scenario.estimate.entryCost, 2, 8)} USDT</dd></div>
                <div><dt>Pérdida estimada</dt><dd>{formatDecimal(scenario.estimate.estimatedLoss, 2, 8)} USDT</dd></div>
                <div><dt>Beneficio / riesgo</dt><dd>{formatDecimal(scenario.estimate.rewardToRisk, 2, 4)}:1</dd></div>
              </>
            )}
          </dl>
        </details>
      )}
    </>
  );
}
