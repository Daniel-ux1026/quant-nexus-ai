import { invoke } from "@tauri-apps/api/core";
import { Plug } from "lucide-react";
import { useState } from "react";

export function OpenAiConnection({ native }: { native: boolean }) {
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<{ text: string; error: boolean }>();

  async function testConnection() {
    setResult(undefined);
    setRunning(true);
    try {
      const model = await invoke<string>("test_openai");
      setResult({ text: `Conexión confirmada con ${model}.`, error: false });
    } catch (error) {
      setResult({ text: String(error), error: true });
    } finally {
      setRunning(false);
    }
  }

  return (
    <section id="openai" className="panel ai-panel" aria-labelledby="openai-heading">
      <div className="ai-controls">
        <div>
          <h2 id="openai-heading">Conexión con OpenAI</h2>
          <p>Envía un único mensaje de prueba con la credencial local de Windows.</p>
        </div>
        <button type="button" className="secondary" disabled={!native || running} onClick={() => void testConnection()}>
          <Plug size={16} aria-hidden="true" /> {running ? "Probando…" : "Probar conexión"}
        </button>
      </div>
      {result && <p className={result.error ? "notice" : "connection-result"}
        role={result.error ? "alert" : "status"}>{result.text}</p>}
    </section>
  );
}
