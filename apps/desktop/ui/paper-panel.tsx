import { useEffect, useRef } from "react";
import { formatDecimal } from "./format";
import { observeAt } from "./scan-view";

type PaperPosition = { quantity: string; entry: string; stop: string; target: string; mark: string };
type PaperEvent = {
  sequence: number;
  timeMs: number;
  pair: string;
  action: "entry" | "stop" | "target" | "emergency" | "blocked";
  price: string | null;
  fee: string | null;
  pnl: string | null;
  detail: string;
};
type PaperAccount = {
  startingCapital: string;
  cash: string;
  equity: string;
  positions: Record<string, PaperPosition>;
  maxDrawdown: string;
  fees: string;
  blocked: number;
  wins: number;
  losses: number;
  flatExits: number;
  lastCloseMs: number | null;
};

export type PaperView = { account: PaperAccount; events: PaperEvent[]; detail: string | null };
export type BacktestReport = {
  account: PaperAccount;
  netPnl: string;
  returnRatio: string;
  buyAndHoldReturn: string;
  winRate: string | null;
  profitFactor: string | null;
  expectancy: string | null;
  sharpePerBar: number | null;
  sortinoPerBar: number | null;
  equityCurve: { closeTimeMs: number; equity: string }[];
  events: PaperEvent[];
};

const percentage = new Intl.NumberFormat("es-PE", { style: "percent", maximumFractionDigits: 2 });
const ratio = new Intl.NumberFormat("es-PE", { maximumFractionDigits: 3 });
const money = (value: string | null) => value === null ? "—" : formatDecimal(value, 2, 6);
const percent = (value: string | null) => value === null ? "—" : percentage.format(Number(value));

const actionNames: Record<PaperEvent["action"], string> = {
  entry: "Compra", stop: "Stop", target: "Objetivo", emergency: "Cierre de emergencia", blocked: "Bloqueo",
};

function ActivityTable({ events }: { events: PaperEvent[] }) {
  if (!events.length) {
    return <p className="replay-meta">El registro se actualizará cuando haya una entrada, salida o bloqueo.</p>;
  }
  return (
    <div className="table-scroll">
      <table className="paper-table">
        <thead><tr><th>Hora UTC</th><th>Par</th><th>Evento</th><th>Precio</th><th>PnL neto</th><th>Motivo</th></tr></thead>
        <tbody>{events.map((event) => (
          <tr key={event.sequence}>
            <td>{observeAt(event.timeMs)}</td><td>{event.pair}</td><td>{actionNames[event.action]}</td>
            <td>{money(event.price)}</td><td>{money(event.pnl)}</td><td>{event.detail}</td>
          </tr>
        ))}</tbody>
      </table>
    </div>
  );
}

function Metrics({ values }: { values: [string, string | number][] }) {
  return (
    <dl className="paper-metrics">{values.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
  );
}

export function PaperPanel({ view, active, paused, emergencyPending, onPause, onEmergency }: {
  view: PaperView | undefined; active: boolean; paused: boolean; emergencyPending: boolean;
  onPause: () => void; onEmergency: () => void;
}) {
  const positions = Object.entries(view?.account.positions ?? {});
  return (
    <section id="cartera" className="panel paper-panel" aria-labelledby="paper-heading">
      <div className="panel-top">
        <div><h2 id="paper-heading">Cartera simulada</h2><p>Saldo, posiciones y registro conservados al cerrar la app.</p></div>
        {active && <div className="replay-actions">
          <button type="button" className="secondary" disabled={emergencyPending} onClick={onPause}>
            {paused ? "Reanudar entradas" : "Pausar nuevas entradas"}
          </button>
          <button type="button" className="secondary" disabled={emergencyPending || !positions.length} onClick={onEmergency}>
            Cerrar posiciones simuladas
          </button>
        </div>}
      </div>
      {paused && active && <p className="notice" role="status">Las entradas están en pausa. Se siguen evaluando stops y objetivos de posiciones abiertas.</p>}
      {emergencyPending && <p className="notice" role="status">Cierre solicitado para la próxima vela sincronizada.</p>}
      {view?.detail && <p className="notice" role="status">{view.detail}</p>}
      {view ? (
        <>
          <Metrics values={[
            ["Capital inicial", money(view.account.startingCapital) + " USDT"],
            ["Saldo disponible", money(view.account.cash) + " USDT"],
            ["Valor de cartera", money(view.account.equity) + " USDT"],
            ["Drawdown máximo", percent(view.account.maxDrawdown)],
            ["Comisiones", money(view.account.fees) + " USDT"],
            ["Entradas bloqueadas", view.account.blocked],
          ]} />
          {positions.length ? <div className="table-scroll">
            <table className="paper-table">
              <thead><tr><th>Posición</th><th>Cantidad</th><th>Entrada</th><th>Marca</th><th>Stop</th><th>Objetivo</th></tr></thead>
              <tbody>{positions.map(([pair, position]) => <tr key={pair}>
                <td>{pair}</td><td>{formatDecimal(position.quantity)}</td><td>{money(position.entry)}</td>
                <td>{money(position.mark)}</td><td>{money(position.stop)}</td><td>{money(position.target)}</td>
              </tr>)}</tbody>
            </table>
          </div> : <p className="replay-meta">Sin posiciones abiertas.</p>}
          <h3 className="paper-subhead">Actividad reciente</h3>
          <ActivityTable events={view.events} />
        </>
      ) : <p className="replay-meta">Selecciona Paper trading al iniciar para crear o reanudar una cartera.</p>}
    </section>
  );
}

function EquityChart({ points }: { points: BacktestReport["equityCurve"] }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const element = canvas.current;
    const context = element?.getContext("2d");
    if (!element || !context || !points.length) return;
    const surface = element;
    const painter = context;
    const values = points.map((point) => Number(point.equity));
    const low = Math.min(...values);
    const high = Math.max(...values);
    const draw = () => {
      const width = surface.clientWidth;
      const height = surface.clientHeight;
      const scale = window.devicePixelRatio || 1;
      surface.width = width * scale;
      surface.height = height * scale;
      painter.setTransform(scale, 0, 0, scale, 0, 0);
      painter.strokeStyle = "#71ead0";
      painter.lineWidth = 2;
      painter.beginPath();
      values.forEach((value, index) => {
        const x = 8 + (width - 16) * index / Math.max(1, values.length - 1);
        const y = low === high ? height / 2 : height - 8 - (value - low) / (high - low) * (height - 16);
        if (index === 0) painter.moveTo(x, y); else painter.lineTo(x, y);
      });
      painter.stroke();
    };
    const observer = new ResizeObserver(draw);
    observer.observe(surface);
    return () => observer.disconnect();
  }, [points]);
  return <canvas ref={canvas} className="equity-chart" role="img" aria-label="Evolución del valor de la cartera simulada" />;
}

export function BacktestPanel({ report }: { report: BacktestReport }) {
  return (
    <section className="backtest" aria-labelledby="backtest-heading">
      <div className="panel-top">
        <div><h3 id="backtest-heading">Backtesting del tramo completo</h3><p>La señal al cierre se procesa en la apertura siguiente con comisiones y deslizamiento modelados.</p></div>
      </div>
      <Metrics values={[
        ["PnL neto", money(report.netPnl) + " USDT"], ["Retorno", percent(report.returnRatio)],
        ["Buy & Hold", percent(report.buyAndHoldReturn)], ["Drawdown", percent(report.account.maxDrawdown)],
        ["Operaciones ganadoras", percent(report.winRate)], ["Factor de beneficio", report.profitFactor ? formatDecimal(report.profitFactor, 2, 3) : "—"],
        ["Expectativa neta", report.expectancy ? money(report.expectancy) + " USDT" : "—"],
        ["Sharpe por vela", report.sharpePerBar === null ? "—" : ratio.format(report.sharpePerBar)],
        ["Sortino por vela", report.sortinoPerBar === null ? "—" : ratio.format(report.sortinoPerBar)],
      ]} />
      <EquityChart points={report.equityCurve} />
      <details className="observation-details">
        <summary>Actividad del backtest ({report.events.length})</summary>
        <ActivityTable events={report.events} />
      </details>
    </section>
  );
}
