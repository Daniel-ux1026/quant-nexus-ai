import { invoke } from "@tauri-apps/api/core";
import { Activity, Play, Radio, ScanLine, ShieldCheck, Sparkles, Square } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";

import { formatDecimal } from "./format";
import { PaperPanel, type PaperView } from "./paper-panel";
import { ObservationView, observeAt, watchedPairs, type Observation } from "./scan-view";

type FeedStatus = "stopped" | "syncing" | "live" | "reconnecting";
type ScannerSnapshot = {
  status: FeedStatus;
  capital: string | null;
  detail: string | null;
  observations: { pair: string; observation: Observation }[];
  paper: PaperView | null;
  paperMode: boolean;
  entriesPaused: boolean;
  emergencyPending: boolean;
};
const stopped: ScannerSnapshot = {
  status: "stopped", capital: null, detail: null, observations: [],
  paper: null, paperMode: false, entriesPaused: false, emergencyPending: false,
};
const statusLabels: Record<FeedStatus, string> = {
  stopped: "Detenido", syncing: "Sincronizando", live: "En directo", reconnecting: "Recuperando",
};

export function ScannerPanel({ native, capital, onCapital }: {
  native: boolean;
  capital: string;
  onCapital: (value: string) => void;
}) {
  const [snapshot, setSnapshot] = useState(stopped);
  const [operation, setOperation] = useState<"start_scanner" | "stop_scanner">();
  const [error, setError] = useState("");
  const [paperMode, setPaperMode] = useState(false);
  const [savedPaper, setSavedPaper] = useState<PaperView>();
  const [aiPair, setAiPair] = useState("");
  const [aiResult, setAiResult] = useState<{ pair: string; closeTimeMs: number; text: string; error: boolean }>();
  const running = snapshot.status !== "stopped";

  useEffect(() => {
    if (!native || running) return;
    let disposed = false;
    invoke<PaperView | null>("saved_paper")
      .then((view) => { if (!disposed) setSavedPaper(view ?? undefined); })
      .catch((problem) => { if (!disposed) setError(String(problem)); });
    return () => { disposed = true; };
  }, [native, running]);

  useEffect(() => {
    if (!native || operation) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const current = await invoke<ScannerSnapshot>("scanner_status");
        if (disposed) return;
        setSnapshot(current);
        if (current.capital) onCapital(current.capital);
      } catch (problem) {
        if (!disposed) {
          setSnapshot(stopped);
          setError(String(problem));
        }
      } finally {
        if (!disposed) timer = setTimeout(() => void poll(), 1500);
      }
    }
    void poll();
    return () => { disposed = true; clearTimeout(timer); };
  }, [native, operation, onCapital]);

  async function toggle(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const command = running ? "stop_scanner" : "start_scanner";
    setOperation(command);
    setError("");
    setAiResult(undefined);
    try {
      const args = command === "start_scanner"
        ? { capital: capital.trim().replace(",", "."), paper: paperMode }
        : {};
      setSnapshot(await invoke<ScannerSnapshot>(command, args));
    } catch (problem) {
      setError(String(problem));
    } finally {
      setOperation(undefined);
    }
  }

  async function togglePaperPause() {
    try {
      setSnapshot(await invoke<ScannerSnapshot>("pause_paper", { paused: !snapshot.entriesPaused }));
    } catch (problem) {
      setError(String(problem));
    }
  }

  async function closePaperPositions() {
    try {
      setSnapshot(await invoke<ScannerSnapshot>("close_paper_positions"));
    } catch (problem) {
      setError(String(problem));
    }
  }

  async function explain(pair: string, closeTimeMs: number) {
    setAiPair(pair);
    setAiResult(undefined);
    try {
      const text = await invoke<string>("explain_scanner", { pair, closeTimeMs });
      setAiResult({ pair, closeTimeMs, text, error: false });
    } catch (problem) {
      setAiResult({ pair, closeTimeMs, text: String(problem), error: true });
    } finally {
      setAiPair("");
    }
  }

  return (
    <section id="automatico" aria-labelledby="automation-heading">
      <div className="panel auto-hero">
        <div className="hero-content">
          <div className={`feed-indicator ${snapshot.status}`} role="status">
            <Radio size={15} aria-hidden="true" /> {statusLabels[snapshot.status]}
          </div>
          <h1 id="automation-heading">Observación automática</h1>
          <p>Indica tu capital simulado. El escáner analiza Binance Spot y calcula el tamaño y el riesgo de cada escenario al cierre de la vela.</p>
        </div>
        <form className="capital-form" onSubmit={(event) => void toggle(event)}>
          <label htmlFor="automatic-mode">Modo</label>
          <select id="automatic-mode" className="glass-select" value={running ? String(snapshot.paperMode) : String(paperMode)}
            disabled={running || Boolean(operation)} onChange={(event) => setPaperMode(event.target.value === "true")}>
            <option value="false">Observación</option><option value="true">Paper trading</option>
          </select>
          <label htmlFor="automatic-capital">Capital simulado</label>
          <div className="capital-entry">
            <input id="automatic-capital" value={capital} inputMode="decimal" autoComplete="off"
              placeholder="1000.00" maxLength={28} pattern="[0-9]+([.,][0-9]+)?"
              required={!running} disabled={running || Boolean(operation)}
              onChange={(event) => setCapitalDraft(event.target.value)} />
            <span>USDT</span>
          </div>
          <button type="submit" disabled={!native || Boolean(operation)}
            className={running ? "secondary stop-button" : "primary"}>
            {running ? <Square size={15} aria-hidden="true" /> : <Play size={15} aria-hidden="true" />}
            {operation === "start_scanner" ? "Iniciando…" : operation === "stop_scanner" ? "Deteniendo…"
              : running ? "Detener observación" : "Iniciar observación"}
          </button>
        </form>
      </div>
      {error && <p className="notice scanner-notice" role="alert">{error}</p>}
      {snapshot.detail && <p className="notice scanner-notice" role="status">{snapshot.detail}</p>}

      <div className="desk-strip" aria-label="Configuración de observación">
        <div><Activity size={17} aria-hidden="true" /><span><strong>Binance Spot</strong><small>Velas cerradas de 1 min</small></span></div>
        <div><ScanLine size={17} aria-hidden="true" /><span><strong>{watchedPairs.length} pares</strong><small>Escaneo continuo</small></span></div>
        <div><ShieldCheck size={17} aria-hidden="true" /><span><strong>0.25% de riesgo</strong><small>Máximo estimado por escenario</small></span></div>
      </div>

      <div className="panel scan-deck">
        <div className="panel-top">
          <div><h2>Escáner de mercado</h2><p>Comisión supuesta de 0.10% y deslizamiento de 0.05% por lado.</p></div>
        </div>
        <div className="pair-grid">
          {watchedPairs.map((pair) => {
            const item = snapshot.observations.find((result) => result.pair === pair);
            return (
              <article className="pair-card" key={pair} aria-label={pair}>
                <header>
                  <span className={`pair-icon ${pair.slice(0, 3).toLowerCase()}`} aria-hidden="true">
                    {pair.slice(0, 1)}
                  </span>
                  <div>
                    <h3>{pair}</h3>
                    <time dateTime={item ? new Date(item.observation.closeTimeMs).toISOString() : undefined}>
                      {item ? observeAt(item.observation.closeTimeMs) : statusLabels[snapshot.status]}
                    </time>
                  </div>
                </header>
                {item && (
                  <>
                    <div className="pair-price">{formatDecimal(item.observation.close, 2)} <small>USDT</small></div>
                    <ObservationView observation={item.observation} />
                    <button className="secondary explain-button" type="button"
                      disabled={Boolean(aiPair) || snapshot.status !== "live"}
                      onClick={() => void explain(pair, item.observation.closeTimeMs)}>
                      <Sparkles size={14} aria-hidden="true" /> {aiPair === pair ? "Consultando…" : "Explicar con GPT"}
                    </button>
                    {aiResult?.pair === pair && (
                      <div className={aiResult.error ? "notice explanation" : "explanation"} role={aiResult.error ? "alert" : "status"}>
                        {!aiResult.error && <small>Vela del {observeAt(aiResult.closeTimeMs)}</small>}
                        <p>{aiResult.text}</p>
                      </div>
                    )}
                  </>
                )}
              </article>
            );
          })}
        </div>
      </div>
      <PaperPanel view={running ? snapshot.paper ?? undefined : savedPaper}
        active={snapshot.paperMode && running} paused={snapshot.entriesPaused}
        emergencyPending={snapshot.emergencyPending} onPause={() => void togglePaperPause()}
        onEmergency={() => void closePaperPositions()} />
    </section>
  );

  function setCapitalDraft(value: string) {
    setError("");
    onCapital(value);
  }
}
