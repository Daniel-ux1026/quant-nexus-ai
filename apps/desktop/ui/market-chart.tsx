import { useEffect, useRef } from "react";

export type Candle = {
  openTimeMs: number;
  closeTimeMs: number;
  open: string;
  high: string;
  low: string;
  close: string;
  baseVolume: string;
};

export type MarketSnapshot = {
  pair: string;
  source: "binance_public" | "bitget_public" | "bitunix_public" | "synthetic_fixture";
  interval: "1m";
  asOfMs: number;
  candles: Candle[];
  omitted?: number;
};

export const spotVenues = { binance: "Binance", bitget: "Bitget", bitunix: "Bitunix" } as const;
export type SpotVenue = keyof typeof spotVenues;

export function sourceLabel(source: MarketSnapshot["source"]) {
  if (source === "synthetic_fixture") return "Datos sintéticos";
  return spotVenues[source.replace("_public", "") as SpotVenue];
}

const clock = new Intl.DateTimeFormat("es-PE", {
  hour: "2-digit", minute: "2-digit", timeZone: "UTC", hourCycle: "h23",
});
const seconds = new Intl.DateTimeFormat("es-PE", {
  hour: "2-digit", minute: "2-digit", second: "2-digit", timeZone: "UTC", hourCycle: "h23",
});
const date = new Intl.DateTimeFormat("es-PE", { month: "short", day: "numeric", timeZone: "UTC" });
const price = new Intl.NumberFormat("es-PE", { maximumFractionDigits: 8 });

type ChartSnapshot = Pick<MarketSnapshot, "pair" | "candles"> & { interval: string };

export function MarketChart({ snapshot }: { snapshot: ChartSnapshot }) {
  const canvas = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    const context = element.getContext("2d");
    if (!context) return;

    const candles = snapshot.candles.map((item) => ({
      ...item,
      low: Number(item.low), high: Number(item.high),
      open: Number(item.open), close: Number(item.close), volume: Number(item.baseVolume),
    }));
    const low = Math.min(...candles.map((item) => item.low));
    const high = Math.max(...candles.map((item) => item.high));
    const padding = (high - low) * 0.07 || high * 0.005;
    const minimum = low - padding;
    const range = high + padding - minimum;
    const maxVolume = Math.max(...candles.map((item) => item.volume)) || 1;
    const span = candles[candles.length - 1].closeTimeMs - candles[0].openTimeMs;
    const candleSpan = candles[0].closeTimeMs - candles[0].openTimeMs;
    const tickFormat = span >= 86_400_000 ? date : candleSpan < 59_999 ? seconds : clock;

    function render() {
      if (!element || !context) return;
      const width = element.clientWidth;
      const height = element.clientHeight;
      const scale = window.devicePixelRatio || 1;
      element.width = Math.round(width * scale);
      element.height = Math.round(height * scale);
      context.setTransform(scale, 0, 0, scale, 0, 0);
      context.clearRect(0, 0, width, height);
      context.font = "11px ui-monospace, Consolas, monospace";
      context.textBaseline = "middle";

      const axisWidth = Math.max(
        context.measureText(price.format(minimum)).width,
        context.measureText(price.format(high)).width,
      ) + 14;
      const plotWidth = Math.max(1, width - axisWidth - 10);
      const top = 16;
      const bottom = height - 85;
      const volumeBottom = height - 28;
      const step = plotWidth / candles.length;
      const y = (value: number) => bottom - (value - minimum) / range * (bottom - top);

      for (let index = 0; index <= 4; index++) {
        const value = minimum + range / 4 * index;
        const position = y(value);
        context.strokeStyle = "#243248";
        context.beginPath();
        context.moveTo(5, position);
        context.lineTo(plotWidth + 7, position);
        context.stroke();
        context.fillStyle = "#a9b7cc";
        context.fillText(price.format(value), plotWidth + 13, position);
      }

      candles.forEach((bar, index) => {
        const x = 5 + step * (index + 0.5);
        const body = Math.max(1, Math.min(11, step * 0.59));
        const rising = bar.close >= bar.open;
        context.strokeStyle = rising ? "#58dec4" : "#ff8094";
        context.fillStyle = context.strokeStyle;
        context.beginPath();
        context.moveTo(x, y(bar.high));
        context.lineTo(x, y(bar.low));
        context.stroke();
        context.fillRect(x - body / 2, Math.min(y(bar.open), y(bar.close)), body, Math.max(1, Math.abs(y(bar.open) - y(bar.close))));

        const volumeHeight = bar.volume / maxVolume * 39;
        context.fillStyle = rising ? "#286760" : "#793e53";
        context.fillRect(x - body / 2, volumeBottom - volumeHeight, body, volumeHeight);
      });

      const tickCount = Math.min(candles.length - 1, width > 480 ? 4 : 2);
      context.fillStyle = "#a9b7cc";
      for (let tick = 0; tick <= tickCount; tick++) {
        const index = tickCount === 0 ? 0 : Math.round((candles.length - 1) * tick / tickCount);
        const label = tickFormat.format(candles[index].openTimeMs);
        const x = 5 + step * (index + 0.5) - context.measureText(label).width / 2;
        context.fillText(label, Math.min(plotWidth - 24, Math.max(5, x)), height - 11);
      }
    }

    const observer = new ResizeObserver(render);
    observer.observe(element);
    return () => observer.disconnect();
  }, [snapshot]);

  return <canvas ref={canvas} role="img"
    aria-label={`Gráfico de ${snapshot.candles.length} velas de ${snapshot.interval} de ${snapshot.pair} y su volumen.`} />;
}
