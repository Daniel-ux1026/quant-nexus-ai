import { invoke } from "@tauri-apps/api/core";
import { ArrowDownRight, ArrowUpRight, CandlestickChart, Clock3, ExternalLink, Globe2, Landmark, Radio, Sparkles } from "lucide-react";
import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import { formatDecimal } from "./format";
import { MarketChart, type Candle } from "./market-chart";
import { TechnicalTable, type TechnicalFrame } from "./market-intelligence";

const markets = { spot: "Spot USDT", perpetual: "Perpetuo USDT" } as const;
type Market = keyof typeof markets;
type Decision = "long" | "short" | "wait";
type Source = { url: string; domain: string; postedMs: number | null };
type Evidence = { text: string; sources: Source[] };
type Report = {
  pair: string;
  market: Market;
  timeframe: string;
  baseInterval: string;
  candleMs: number;
  checkedMs: number;
  validForMs: number;
  indicators: TechnicalFrame;
  candles: Candle[];
  fearGreed: { score: number; classification: string | null; measuredMs: number; stale: boolean } | null;
  decision: Decision;
  levels: { entry: string; invalidation: string; target: string } | null;
  summary: string;
  technical: string;
  macroMarket: Evidence;
  politics: Evidence;
  social: Evidence;
  risks: string[];
  warning: string | null;
};

const shortcuts = ["5m", "15m", "1h", "4h", "1d", "1w"];
const options = ["1s", "1m", "3m", "5m", "7m", "15m", "30m", "45m", "1h", "2h", "4h", "6h", "8h", "12h", "1d", "3d", "1w", "1M"];
const labels = { long: "Long", short: "Short", wait: "Esperar" };
const dateTime = new Intl.DateTimeFormat("es-PE", {
  dateStyle: "medium", timeStyle: "medium", timeZone: "UTC", hourCycle: "h23",
});
const indicatorsFormat = new Intl.NumberFormat("es-PE", { maximumSignificantDigits: 7 });
const indexUrl = "https://alternative.me/crypto/fear-and-greed-index/";

function utc(value: number) {
  return `${dateTime.format(value)} UTC`;
}

function SourceLink({ url, children, onOpen }: { url: string; children: ReactNode; onOpen: (url: string) => void }) {
  return <a className="consult-source" href={url} onClick={(event) => {
    event.preventDefault();
    onOpen(url);
  }}>{children}<ExternalLink size={13} aria-hidden="true" /></a>;
}

function EvidenceCard({ title, icon, evidence, empty, onOpen }: {
  title: string; icon: ReactNode; evidence: Evidence; empty: string; onOpen: (url: string) => void;
}) {
  return (
    <div className="consult-card">
      <h3>{icon}{title}</h3>
      <p>{evidence.text || empty}</p>
      {evidence.sources.length > 0 && (
        <ul className="consult-sources" aria-label={`Fuentes de ${title}`}>
          {evidence.sources.map((source, index) => <li key={source.url}>
            <SourceLink url={source.url} onOpen={onOpen}>
              {source.postedMs === null ? `${source.domain} · ${index + 1}` : `Publicación de X · ${utc(source.postedMs)}`}
            </SourceLink>
          </li>)}
        </ul>
      )}
    </div>
  );
}

function ConsultationResult({ report, onOpen }: { report: Report; onOpen: (url: string) => void }) {
  const { fearGreed: index, indicators } = report;
  const [expired, setExpired] = useState(false);
  useEffect(() => {
    if (!report.levels) return;
    const expiresAt = performance.now() + report.validForMs;
    let timer = 0;
    function schedule() {
      const left = expiresAt - performance.now();
      if (left <= 0) {
        setExpired(true);
      } else {
        timer = window.setTimeout(schedule, Math.min(left, 2_147_483_647));
      }
    }
    schedule();
    return () => window.clearTimeout(timer);
  }, [report]);
  const decision = expired ? "wait" : report.decision;
  const levels = expired ? null : report.levels;
  const score = index === null ? null : Math.max(1, index.score);
  const DecisionIcon = decision === "long" ? ArrowUpRight : decision === "short" ? ArrowDownRight : Clock3;
  return (
    <div className="consult-result">
      {report.warning && <p className="notice" role="alert">{report.warning}</p>}
      <div className="consult-overview">
        <div className={`consult-card consult-decision consult-${decision}`}>
          <div className="consult-title">
            <span className="consult-direction" role="status"><DecisionIcon size={21} aria-hidden="true" />{labels[decision]}</span>
            <small>{report.pair} · {report.timeframe} · {markets[report.market]}</small>
          </div>
          <p>{expired ? "La vigencia de estos niveles terminó. Actualiza la consulta para evaluar una nueva entrada."
            : report.summary || "Espera una nueva confirmación del precio y el contexto."}</p>
          {levels && <>
            <dl className="consult-levels">
              <div><dt>Entrada condicional</dt><dd>{formatDecimal(levels.entry)} <small>USDT</small></dd></div>
              <div><dt>Invalidación</dt><dd>{formatDecimal(levels.invalidation)} <small>USDT</small></dd></div>
              <div><dt>Objetivo</dt><dd>{formatDecimal(levels.target)} <small>USDT</small></dd></div>
            </dl>
            <p className="consult-meta">
              {decision === "long" ? "Requiere un cierre por encima de la entrada." : "Requiere un cierre por debajo de la entrada."} Relación 2:1 antes de costes.
              {report.market === "spot" && decision === "short" && " En Spot, Short expresa un sesgo bajista."}
            </p>
          </>}
          <p className="consult-meta">Último cierre {utc(report.candleMs)} · Consulta {utc(report.checkedMs)}</p>
        </div>
        <div className="consult-card consult-index">
          <h3><Radio size={16} aria-hidden="true" />Miedo y codicia</h3>
          {index && score !== null ? <>
            <div className="consult-score"><strong>{score}<small> / 100</small></strong><span>{index.classification}</span></div>
            <meter className="consult-meter" min={1} max={100} low={25} high={75} optimum={55}
              value={score} aria-label="Índice diario de miedo y codicia de Bitcoin" />
            <p className="consult-meta">Bitcoin · Índice diario · {utc(index.measuredMs)}{index.stale && " · Desactualizado"}</p>
            {index.score === 0 && <p className="consult-meta">Valor publicado de 0, mostrado como 1 en la escala de la app.</p>}
          </> : <p>El proveedor no entregó un valor válido.</p>}
          <SourceLink url={indexUrl} onOpen={onOpen}>Alternative.me</SourceLink>
        </div>
      </div>
      <div className="consult-card consult-chart">
        <div className="consult-chart-header">
          <div>
            <h3><CandlestickChart size={16} aria-hidden="true" />Velas e indicadores</h3>
            <p className="consult-meta">Binance {markets[report.market]} · {indicators.candles} velas cerradas
              {report.baseInterval !== report.timeframe && ` · Agregadas desde ${report.baseInterval}`} · UTC</p>
          </div>
          <div className="consult-stat">
            <span>RSI 14 <strong>{indicators.rsi14 === null ? "—" : indicatorsFormat.format(indicators.rsi14)}</strong></span>
            <span>ATR 14 <strong>{indicators.atr14 === null ? "—" : indicatorsFormat.format(indicators.atr14)}</strong></span>
          </div>
        </div>
        <MarketChart snapshot={{ pair: report.pair, interval: report.timeframe, candles: report.candles }} />
        {report.technical && <p className="consult-technical">{report.technical}</p>}
        <TechnicalTable frames={[indicators]} interval={report.timeframe} />
      </div>
      <div className="consult-evidence">
        <EvidenceCard title="Macro" icon={<Globe2 size={16} aria-hidden="true" />} evidence={report.macroMarket}
          empty="Sin contexto macro respaldado por las fuentes consultadas." onOpen={onOpen} />
        <EvidenceCard title="Política y regulación" icon={<Landmark size={16} aria-hidden="true" />} evidence={report.politics}
          empty="Sin noticias políticas o regulatorias verificadas en esta consulta." onOpen={onOpen} />
        <EvidenceCard title="Pulso de X" icon={<Radio size={16} aria-hidden="true" />} evidence={report.social}
          empty="Sin publicaciones recientes y verificables de X para este análisis." onOpen={onOpen} />
      </div>
      {report.risks.length > 0 && <div className="consult-card">
        <h3>Qué vigilar</h3>
        <ul className="consult-risks">{report.risks.map((risk, index) => <li key={index}>{risk}</li>)}</ul>
      </div>}
    </div>
  );
}

export function ConsultationPanel({ native }: { native: boolean }) {
  const [pair, setPair] = useState("BTC/USDT");
  const [market, setMarket] = useState<Market>("spot");
  const [timeframe, setTimeframe] = useState("1h");
  const [focus, setFocus] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [sourceError, setSourceError] = useState("");
  const [report, setReport] = useState<Report>();

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!native || busy) return;
    setBusy(true);
    setError("");
    setSourceError("");
    setReport(undefined);
    try {
      setReport(await invoke<Report>("consult_market", { request: { pair, market, timeframe, focus } }));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  async function open(url: string) {
    setSourceError("");
    try {
      await invoke("open_consultation_source", { url });
    } catch (reason) {
      setSourceError(String(reason));
    }
  }

  return (
    <section id="consulta" className="panel consult-panel" aria-labelledby="consult-heading">
      <div className="panel-top">
        <div>
          <h2 id="consult-heading">Consulta IA</h2>
          <p>Evalúa un posible Long, Short o Esperar con datos de mercado y fuentes públicas.</p>
        </div>
        <Sparkles size={21} className="consult-icon" aria-hidden="true" />
      </div>
      <form onSubmit={(event) => void submit(event)}>
        <fieldset className="consult-fields" disabled={busy}>
          <label>Par USDT<input value={pair} onChange={(event) => setPair(event.target.value.toUpperCase())}
            placeholder="BTC/USDT" autoComplete="off" maxLength={17} required /></label>
          <label>Mercado<select className="glass-select" value={market} onChange={(event) => setMarket(event.target.value as Market)}>
            {Object.entries(markets).map(([id, label]) => <option key={id} value={id}>{label}</option>)}
          </select></label>
          <label>Temporalidad<input value={timeframe} onChange={(event) => setTimeframe(event.target.value)}
            list="consult-intervals" placeholder="1h" autoComplete="off" maxLength={32} required />
            <datalist id="consult-intervals">{options.filter((item) => market === "spot" || item !== "1s")
              .map((item) => <option key={item} value={item} />)}</datalist>
          </label>
          <label className="consult-focus">Enfoque opcional<input value={focus} onChange={(event) => setFocus(event.target.value)}
            placeholder="Ej. ¿Qué podría invalidar una ruptura?" maxLength={300} autoComplete="off" /></label>
          <button className="primary" type="submit" disabled={!native || busy}><Sparkles size={16} aria-hidden="true" />
            {busy ? "Analizando…" : "Consultar IA"}</button>
        </fieldset>
        <div className="consult-controls">
          <div className="consult-presets" role="group" aria-label="Temporalidades frecuentes">
            {shortcuts.map((item) => <button key={item} type="button" className="secondary"
              aria-pressed={timeframe === item} disabled={busy} onClick={() => setTimeframe(item)}>{item}</button>)}
          </div>
          <p className="consult-meta">Puedes escribir intervalos como 45m, 7 minutos o 1 mes.</p>
        </div>
      </form>
      {busy && <p className="consult-message" role="status">Cargando velas e índice y buscando fuentes para el análisis…</p>}
      {error && <p className="notice consult-message" role="alert">{error}</p>}
      {report && <ConsultationResult report={report} onOpen={(url) => void open(url)} />}
      {sourceError && <p className="notice consult-message" role="alert">{sourceError}</p>}
    </section>
  );
}
