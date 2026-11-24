import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { Activity } from "lucide-react";
import { formatDecimal } from "./format";
import { observeAt } from "./scan-view";

export type TechnicalFrame = {
  minutes: number;
  candles: number;
  close: number;
  rsi14: number | null;
  ema9: number | null;
  ema20: number | null;
  ema50: number | null;
  ema100: number | null;
  ema200: number | null;
  sma20: number | null;
  atr14: number | null;
  macd: number | null;
  macdSignal: number | null;
  bollingerLow: number | null;
  bollingerHigh: number | null;
  sampleVwap: number | null;
  obv: number;
  priorSupport: number | null;
  priorResistance: number | null;
  rangePosition: "above" | "below" | "inside" | "unknown";
};

type PublicContext = {
  pair: string;
  book: { bid: string; ask: string; spreadBps: string; imbalance: string; receivedMs: number } | null;
  derivatives: { mark: string; index: string; funding: string; openInterest: string; markTimeMs: number; nextFundingMs: number } | null;
  monetaryNews: { title: string; published: string }[];
  warnings: string[];
};

const metricFormat = new Intl.NumberFormat("es-PE", { maximumFractionDigits: 6 });
const percentage = new Intl.NumberFormat("es-PE", { style: "percent", maximumFractionDigits: 5 });
const scale = (value: number | null) => value === null ? "—" : metricFormat.format(value);
const amount = (value: string) => formatDecimal(value, 0, 6);

export function TechnicalTable({ frames, interval }: { frames: TechnicalFrame[]; interval?: string }) {
  const names: [string, keyof TechnicalFrame][] = [
    ["Velas cerradas", "candles"], ["Cierre", "close"], ["RSI 14", "rsi14"],
    ["ATR 14", "atr14"], ["SMA 20", "sma20"], ["EMA 9", "ema9"], ["EMA 20", "ema20"],
    ["EMA 50", "ema50"], ["EMA 100", "ema100"], ["EMA 200", "ema200"], ["MACD", "macd"],
    ["Señal MACD", "macdSignal"], ["Bollinger inferior", "bollingerLow"], ["Bollinger superior", "bollingerHigh"],
    ["VWAP aproximado de la muestra", "sampleVwap"], ["OBV", "obv"],
    ["Soporte previo", "priorSupport"], ["Resistencia previa", "priorResistance"], ["Posición frente al rango", "rangePosition"],
  ];
  const positions = { above: "Ruptura alcista", below: "Ruptura bajista", inside: "Dentro del rango", unknown: "Sin referencia" };
  if (!frames.length) return null;
  return (
    <details className="observation-details technical-table">
      <summary><Activity size={14} aria-hidden="true" /> Indicadores y estructura</summary>
      <div className="table-scroll">
        <table className="paper-table">
          <thead><tr><th>Indicador</th>{frames.map((frame) => <th key={frame.minutes}>{interval ?? `${frame.minutes} min`}</th>)}</tr></thead>
          <tbody>{names.map(([label, key]) => <tr key={key}>
            <th scope="row">{label}</th>
            {frames.map((frame) => <td key={frame.minutes}>
              {key === "rangePosition" ? positions[frame.rangePosition] : scale(frame[key] as number | null)}
            </td>)}
          </tr>)}</tbody>
        </table>
      </div>
      <p className="replay-meta">El guion indica que el tramo aún tiene pocas velas para ese cálculo.</p>
    </details>
  );
}

export function MarketContextPanel({ native, pair }: { native: boolean; pair: string }) {
  const [report, setReport] = useState<PublicContext>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function load() {
    setBusy(true);
    setError("");
    try {
      setReport(await invoke<PublicContext>("market_context", { pair }));
    } catch (problem) {
      setError(String(problem));
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="panel replay-panel" aria-labelledby="context-heading">
      <div className="panel-top">
        <div><h2 id="context-heading">Contexto de mercado</h2><p>Liquidez Spot, derivados y comunicados monetarios públicos.</p></div>
        <button type="button" className="secondary" disabled={!native || busy} onClick={() => void load()}>
          {busy ? "Consultando…" : `Consultar contexto de ${pair}`}
        </button>
      </div>
      {error && <p className="notice" role="alert">{error}</p>}
      {report && (
        <>
          <p className="replay-meta">{report.pair}</p>
          {report.warnings.map((warning) => <p key={warning} className="notice" role="status">{warning}</p>)}
          <div className="pair-grid">
            {report.book && <div className="pair-card">
              <h3>Libro Spot · Binance</h3>
              <time>{observeAt(report.book.receivedMs)}</time>
              <dl className="observation-grid">
                <div><dt>Mejor compra</dt><dd>{amount(report.book.bid)}</dd></div>
                <div><dt>Mejor venta</dt><dd>{amount(report.book.ask)}</dd></div>
                <div><dt>Spread</dt><dd>{amount(report.book.spreadBps)} pb</dd></div>
                <div><dt>Desequilibrio · 10 niveles</dt><dd>{percentage.format(Number(report.book.imbalance))}</dd></div>
              </dl>
            </div>}
            {report.derivatives && <div className="pair-card">
              <h3>Futuros USDⓈ-M · Binance</h3>
              <time>{observeAt(report.derivatives.markTimeMs)}</time>
              <dl className="observation-grid">
                <div><dt>Precio marca</dt><dd>{amount(report.derivatives.mark)}</dd></div>
                <div><dt>Índice</dt><dd>{amount(report.derivatives.index)}</dd></div>
                <div><dt>Último funding</dt><dd>{percentage.format(Number(report.derivatives.funding))}</dd></div>
                <div><dt>Interés abierto</dt><dd>{amount(report.derivatives.openInterest)}</dd></div>
                <div><dt>Próximo funding</dt><dd>{observeAt(report.derivatives.nextFundingMs)}</dd></div>
              </dl>
            </div>}
            {report.monetaryNews.length > 0 && <div className="pair-card">
              <h3>Reserva Federal · Política monetaria</h3>
              <ul className="bulletin-list">{report.monetaryNews.map((item) => <li key={item.title + item.published}>
                <span>{item.title}</span><small>{item.published}</small>
              </li>)}</ul>
            </div>}
          </div>
        </>
      )}
    </section>
  );
}
