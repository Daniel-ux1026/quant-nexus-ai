import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  Activity, ArrowDownUp, CandlestickChart, Database, FlaskConical,
  Radio, RefreshCw, ShieldCheck,
} from "lucide-react";
import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { ConsultationPanel } from "./consultation-panel";
import { formatDecimal } from "./format";
import { MarketChart, sourceLabel, spotVenues, type MarketSnapshot, type SpotVenue } from "./market-chart";
import { MarketContextPanel, TechnicalTable, type TechnicalFrame } from "./market-intelligence";
import { RiskCalculator } from "./risk-calculator";
import { OpenAiConnection } from "./openai-connection";
import { ReplayPanel } from "./replay-panel";
import { ScannerPanel } from "./scanner-panel";
import "./styles.css";

const percent = new Intl.NumberFormat("es-PE", {
  style: "percent",
  signDisplay: "exceptZero",
  maximumFractionDigits: 2,
  minimumFractionDigits: 2,
});
const clock = new Intl.DateTimeFormat("es-PE", {
  dateStyle: "medium",
  timeStyle: "medium",
  timeZone: "UTC",
  hourCycle: "h23",
});

type MarketInspection = MarketSnapshot & { technical: TechnicalFrame[] };

function App() {
  const [snapshot, setSnapshot] = useState<MarketInspection>();
  const [pair, setPair] = useState("BTC/USDT");
  const [venue, setVenue] = useState<SpotVenue>("binance");
  const [working, setWorking] = useState("");
  const [message, setMessage] = useState("");
  const [capital, setCapital] = useState("");
  const native = isTauri();

  useEffect(() => {
    if (!native) return;
    let mounted = true;
    invoke<MarketInspection>("load_fixture")
      .then((value) => { if (mounted) setSnapshot((current) => current ?? value); })
      .catch((error: unknown) => { if (mounted) setMessage(String(error)); });
    return () => { mounted = false; };
  }, [native]);

  async function loadMarket(command: "load_fixture" | "refresh_market") {
    setMessage("");
    setWorking(command);
    try {
      const value = await invoke<MarketInspection>(command, { pair, venue });
      setSnapshot(value);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setWorking("");
    }
  }

  const last = snapshot?.candles.at(-1);
  const startingPrice = snapshot?.candles[0]?.open;
  const change = last && startingPrice ? Number(last.close) / Number(startingPrice) - 1 : undefined;
  const synthetic = snapshot?.source === "synthetic_fixture";

  return (
    <>
      <header className="topbar">
        <a className="brand" href="#automatico">
          <span className="brand-icon"><Activity size={21} aria-hidden="true" /></span>
          <span>Quant Nexus <b>AI</b></span>
        </a>
        <nav aria-label="Secciones">
          <a href="#automatico">Automático</a>
          <a href="#cartera">Cartera</a>
          <a href="#consulta">Consulta IA</a>
          <a href="#mercado">Mercado</a>
          <a href="#replay">Reproducción</a>
          <a href="#riesgo">Riesgo</a>
          <a href="#openai">OpenAI</a>
        </nav>
        <span className="mode"><span className="status-dot" /> Simulación</span>
      </header>
      <main>
        {!native && <p className="notice" role="status">Abre la aplicación de escritorio para acceder al motor.</p>}
        <ScannerPanel native={native} capital={capital} onCapital={setCapital} />
        <ConsultationPanel native={native} />
        <section id="mercado" aria-labelledby="market-heading">
          <div className="section-heading">
            <div>
              <h2 id="market-heading">Mercado Spot</h2>
              <p>Velas cerradas de un minuto en pares USDT.</p>
            </div>
            <form className="market-form" onSubmit={(event) => {
              event.preventDefault();
              void loadMarket("refresh_market");
            }}>
              <label className="visually-hidden" htmlFor="pair">Par Spot USDT</label>
              <label className="visually-hidden" htmlFor="market-venue">Exchange Spot</label>
              <select id="market-venue" className="glass-select" value={venue}
                onChange={(event) => setVenue(event.target.value as SpotVenue)}>
                {Object.entries(spotVenues).map(([id, label]) => <option key={id} value={id}>{label}</option>)}
              </select>
              <input id="pair" value={pair} onChange={(event) => setPair(event.target.value.toUpperCase())}
                autoComplete="off" maxLength={17} placeholder="BTC/USDT" required />
              <button className="primary" disabled={!native || Boolean(working)}>
                <RefreshCw size={16} aria-hidden="true" />
                {working === "refresh_market" ? "Consultando…" : "Consultar mercado"}
              </button>
            </form>
          </div>
          {message && <p className="notice" role="alert">{message}</p>}
          <div className="metrics">
            <div className="metric">
              <span><CandlestickChart size={16} aria-hidden="true" />Último cierre</span>
              <strong>{last ? formatDecimal(last.close, 2) : "—"} {last && <small>USDT</small>}</strong>
            </div>
            <div className="metric">
              <span><ArrowDownUp size={16} aria-hidden="true" />Variación del tramo</span>
              <strong className={change !== undefined && change >= 0 ? "gain" : change !== undefined ? "loss" : ""}>
                {change !== undefined ? percent.format(change) : "—"}
              </strong>
            </div>
            <div className="metric">
              <span><Database size={16} aria-hidden="true" />Velas cerradas</span>
              <strong>{snapshot?.candles.length ?? "—"}</strong>
            </div>
          </div>
        </section>

        <div className="workspace">
          <section className="panel market-panel" aria-labelledby="chart-heading">
            <div className="panel-top">
              <div>
                <h2 id="chart-heading">{snapshot?.pair ?? "Mercado"}</h2>
                <p>Precio y volumen</p>
              </div>
              {snapshot && <span className={synthetic ? "tag synthetic" : "tag"}>
                {synthetic ? <FlaskConical size={15} aria-hidden="true" /> : <Radio size={15} aria-hidden="true" />}
                {synthetic ? sourceLabel(snapshot.source) : `Instantánea de ${sourceLabel(snapshot.source)}`}
              </span>}
            </div>
            {snapshot
              ? <MarketChart snapshot={snapshot} />
              : <div className="chart-empty">Las velas aparecerán al cargar una fuente.</div>}
            {Boolean(snapshot?.omitted) && <p className="notice scanner-notice" role="status">
              Se omitieron {snapshot?.omitted} velas fuera del tramo reciente y continuo que superó la validación.
            </p>}
            <div className="chart-footer">
              <span>{snapshot ? `${clock.format(snapshot.asOfMs)} UTC` : "Hora del mercado en UTC"}</span>
              <button type="button" className="secondary" disabled={!native || Boolean(working) || synthetic}
                onClick={() => void loadMarket("load_fixture")}>Cargar ejemplo</button>
            </div>
            {snapshot && <TechnicalTable frames={snapshot.technical ?? []} />}
          </section>
          <RiskCalculator native={native} />
        </div>
        <MarketContextPanel native={native} pair={snapshot?.pair ?? pair} />
        <ReplayPanel native={native} capital={capital} />
        <OpenAiConnection native={native} />
      </main>
      <footer><ShieldCheck size={17} aria-hidden="true" /> Capital simulado · Ejecución real deshabilitada</footer>
    </>
  );
}

createRoot(document.getElementById("root")!).render(<StrictMode><App /></StrictMode>);
