import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, ChevronRight, History, Play, Square } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";

import { formatDecimal } from "./format";
import { MarketChart, sourceLabel, spotVenues, type MarketSnapshot, type SpotVenue } from "./market-chart";
import { BacktestPanel, type BacktestReport } from "./paper-panel";
import { ObservationView, observeAt, watchedPairs, type Observation } from "./scan-view";

type ReplayResult = {
  pair: string;
  source: MarketSnapshot["source"];
  candles: MarketSnapshot["candles"];
  observations: Observation[];
  backtest: BacktestReport;
  research: ResearchReport | null;
};

type ResearchReport = {
  samples: number; testSamples: number; positiveTestSamples: number; brier: number; baselineBrier: number;
  folds: { fromMs: number; throughMs: number; trainSamples: number; testSamples: number; brier: number; baselineBrier: number }[];
};

const scoreFormat = new Intl.NumberFormat("es-PE", { maximumFractionDigits: 6 });

function ResearchSummary({ report }: { report: ResearchReport }) {
  return (
    <details className="observation-details backtest">
      <summary>Investigación cuantitativa · Walk-forward</summary>
      <p className="replay-meta">Regresión logística experimental para un resultado neto positivo en cinco velas. Mide las probabilidades fuera de muestra con el Brier. Un valor menor indica menor error.</p>
      <div className="table-scroll">
        <table className="paper-table">
          <thead><tr><th>Tramo de prueba</th><th>Entrenamiento</th><th>Prueba</th><th>Brier del modelo</th><th>Brier de referencia</th></tr></thead>
          <tbody>{report.folds.map((fold) => <tr key={fold.fromMs}>
            <td>{observeAt(fold.fromMs)} → {observeAt(fold.throughMs)}</td>
            <td>{fold.trainSamples}</td><td>{fold.testSamples}</td>
            <td>{scoreFormat.format(fold.brier)}</td><td>{scoreFormat.format(fold.baselineBrier)}</td>
          </tr>)}</tbody>
        </table>
      </div>
      <p className="replay-meta">{report.positiveTestSamples} resultados positivos entre {report.testSamples} muestras de prueba. Brier global {scoreFormat.format(report.brier)}. Referencia {scoreFormat.format(report.baselineBrier)}.</p>
    </details>
  );
}

function lastHourUtc() {
  const end = Math.floor(Date.now() / 60_000) * 60_000;
  const display = (time: number) => new Date(time).toISOString().slice(0, 16);
  return { from: display(end - 3_600_000), before: display(end) };
}

export function ReplayPanel({ capital, native }: { capital: string; native: boolean }) {
  const [pair, setPair] = useState<string>(watchedPairs[0]);
  const [venue, setVenue] = useState<SpotVenue>("binance");
  const [range, setRange] = useState(lastHourUtc);
  const [series, setSeries] = useState<ReplayResult>();
  const [chosenCapital, setChosenCapital] = useState("");
  const [step, setStep] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState("");
  const length = series?.candles.length ?? 0;
  const lastStep = length - 1;
  const observation = series?.observations[step];

  useEffect(() => {
    if (!playing || step >= lastStep) return;
    const timer = setTimeout(() => setStep((current) => current + 1), 550);
    return () => clearTimeout(timer);
  }, [playing, step, lastStep]);

  async function load(source: "archive" | "synthetic" | "download") {
    setPlaying(false);
    setMessage("");
    if (!capital.trim()) {
      setMessage("Ingresa el capital simulado en Observación automática.");
      return;
    }
    const amount = capital.trim().replace(",", ".");
    const selection = { pair, fromMs: Date.parse(range.from + "Z"), beforeMs: Date.parse(range.before + "Z") };
    const request = source === "synthetic" ? { source } : { source: "archive", venue, ...selection };
    setLoading(true);
    try {
      const result = source === "download"
        ? await invoke<ReplayResult>("import_binance_history", { capital: amount, ...selection })
        : await invoke<ReplayResult>("replay_market", { capital: amount, request });
      setSeries(result);
      setChosenCapital(amount);
      setStep(0);
    } catch (problem) {
      setMessage(String(problem));
    } finally {
      setLoading(false);
    }
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void load("archive");
  }

  function seek(index: number) {
    setPlaying(false);
    setStep(index);
  }

  const activePlayback = playing && step < lastStep;
  const chart: MarketSnapshot | undefined = series && observation ? {
    pair: series.pair, source: series.source, interval: "1m",
    asOfMs: observation.closeTimeMs,
    candles: series.candles.slice(Math.max(0, step - 99), step + 1),
  } : undefined;

  return (
    <section id="replay" className="panel replay-panel" aria-labelledby="replay-heading">
      <div className="panel-top">
        <div><h2 id="replay-heading">Reproducción histórica</h2><p>Revisa las decisiones del escáner vela por vela, con el capital del panel automático.</p></div>
        <History size={21} aria-hidden="true" className="risk-icon" />
      </div>
      <form className="replay-form" onSubmit={submit}>
        <label>Exchange
          <select value={venue} onChange={(event) => setVenue(event.target.value as SpotVenue)}>
            {Object.entries(spotVenues).map(([id, label]) => <option key={id} value={id}>{label}</option>)}
          </select>
        </label>
        <label>Par
          <select value={pair} onChange={(event) => setPair(event.target.value)}>
            {watchedPairs.map((symbol) => <option key={symbol}>{symbol}</option>)}
          </select>
        </label>
        <label>Desde (UTC)
          <input type="datetime-local" value={range.from} required
            onChange={(event) => setRange({ ...range, from: event.target.value })} />
        </label>
        <label>Hasta (UTC)
          <input type="datetime-local" value={range.before} required
            onChange={(event) => setRange({ ...range, before: event.target.value })} />
        </label>
        <div className="replay-actions">
          <button className="primary" type="submit" disabled={!native || loading}>
            {loading ? "Cargando…" : "Cargar historial local"}
          </button>
          <button className="secondary" type="button" disabled={!native || loading}
            onClick={() => void load("synthetic")}>Probar ejemplo sintético</button>
          {venue === "binance" && <button className="secondary" type="button" disabled={!native || loading}
            onClick={() => void load("download")}>Descargar tramo de Binance</button>}
        </div>
      </form>
      {message && <p className="notice scanner-notice" role="alert">{message}</p>}
      {chart && observation && series && (
        <div className="replay-content">
          <div className="replay-plot">
            <div className="replay-source">
              <strong>{series.pair}</strong>
              <span className={series.source === "synthetic_fixture" ? "tag synthetic" : "tag"}>
                {series.source === "synthetic_fixture" ? sourceLabel(series.source) : `Historial de ${sourceLabel(series.source)}`}
              </span>
            </div>
            <MarketChart snapshot={chart} />
            <div className="replay-controls">
              <button type="button" className="secondary icon-button" aria-label="Vela anterior" disabled={step === 0}
                onClick={() => seek(step - 1)}><ChevronLeft size={17} /></button>
              <button type="button" className="secondary icon-button" aria-label={activePlayback ? "Pausar reproducción" : "Reproducir"}
                disabled={length <= 1}
                onClick={() => {
                  if (activePlayback) {
                    setPlaying(false);
                  } else {
                    if (step >= lastStep) setStep(0);
                    setPlaying(true);
                  }
                }}>{activePlayback ? <Square size={15} /> : <Play size={15} />}</button>
              <button type="button" className="secondary icon-button" aria-label="Vela siguiente" disabled={step >= lastStep}
                onClick={() => seek(step + 1)}><ChevronRight size={17} /></button>
              <input aria-label="Vela seleccionada" type="range" min={0} max={lastStep} value={step}
                onChange={(event) => seek(Number(event.target.value))} />
              <span>{step + 1} / {length}</span>
            </div>
          </div>
          <div className="replay-insight">
            <h3>Decisión de la vela</h3>
            <p className="replay-meta">{observeAt(observation.closeTimeMs)} · {formatDecimal(chosenCapital, 2)} USDT sim.</p>
            <ObservationView observation={observation} />
          </div>
        </div>
      )}
      {series && <BacktestPanel report={series.backtest} />}
      {series?.research && <ResearchSummary report={series.research} />}
    </section>
  );
}
