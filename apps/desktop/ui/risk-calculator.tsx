import { invoke } from "@tauri-apps/api/core";
import { Calculator } from "lucide-react";
import { useState, type FormEvent } from "react";
import { formatDecimal } from "./format";

const initial = {
  equity: "1000", entry: "100", stop: "98", target: "106",
  riskBps: "25", feeBpsPerSide: "10", slippageBpsPerSide: "5",
};
const fields = [
  ["equity", "Capital simulado", "USDT"], ["entry", "Entrada", "USDT"],
  ["stop", "Stop", "USDT"], ["target", "Objetivo", "USDT"],
  ["riskBps", "Riesgo", "pb"], ["feeBpsPerSide", "Comisión por lado", "pb"],
  ["slippageBpsPerSide", "Deslizamiento por lado", "pb"],
] as const;
type Result = {
  quantity: string; entryCost: string; riskBudget: string; estimatedLoss: string;
  estimatedReward: string; rewardToRisk: string;
};

export function RiskCalculator({ native }: { native: boolean }) {
  const [values, setValues] = useState(initial);
  const [result, setResult] = useState<Result>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function estimate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setResult(undefined);
    setError("");
    setBusy(true);
    const decimal = (value: string) => value.trim().replace(",", ".");
    try {
      setResult(await invoke<Result>("estimate_position", {
        input: {
          equity: decimal(values.equity),
          entry: decimal(values.entry),
          stop: decimal(values.stop),
          target: decimal(values.target),
          riskBps: Number(values.riskBps),
          feeBpsPerSide: Number(values.feeBpsPerSide),
          slippageBpsPerSide: Number(values.slippageBpsPerSide),
        },
      }));
    } catch (problem) {
      setError(String(problem));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section id="riesgo" className="panel risk-panel" aria-labelledby="risk-heading">
      <div className="panel-top">
        <div>
          <h2 id="risk-heading">Simulador de riesgo</h2>
          <p>Compra Spot sin apalancamiento</p>
        </div>
        <Calculator size={20} aria-hidden="true" className="risk-icon" />
      </div>
      <form onSubmit={(event) => void estimate(event)}>
        <div className="risk-inputs">
          {fields.map(([key, label, unit]) => (
            <label key={key}>
              <span>{label}</span>
              <div className="input-with-unit">
                <input required value={values[key]} disabled={busy}
                  maxLength={unit === "pb" ? 3 : 25}
                  inputMode={unit === "pb" ? "numeric" : "decimal"}
                  pattern={unit === "pb" ? "[0-9]+" : "[0-9]+([.,][0-9]+)?"}
                  onChange={(event) => {
                    setValues({ ...values, [key]: event.target.value });
                    setResult(undefined);
                    setError("");
                  }} />
                <span aria-hidden="true">{unit}</span>
              </div>
            </label>
          ))}
        </div>
        <p className="field-note">100 pb equivalen a 1%. El máximo inicial de riesgo es 25 pb.</p>
        <button className="primary" disabled={!native || busy}>{busy ? "Calculando…" : "Calcular tamaño"}</button>
      </form>
      {error && <p className="notice" role="alert">{error}</p>}
      {result && <dl className="estimate" aria-live="polite">
        <div><dt>Cantidad</dt><dd>{formatDecimal(result.quantity)}</dd></div>
        <div><dt>Costo de la compra</dt><dd>{formatDecimal(result.entryCost)} USDT</dd></div>
        <div><dt>Pérdida estimada al stop</dt><dd>{formatDecimal(result.estimatedLoss)} USDT</dd></div>
        <div><dt>Beneficio estimado</dt><dd>{formatDecimal(result.estimatedReward)} USDT</dd></div>
        <div><dt>Beneficio / riesgo</dt><dd>{formatDecimal(result.rewardToRisk, 2, 4)}:1</dd></div>
      </dl>}
    </section>
  );
}
