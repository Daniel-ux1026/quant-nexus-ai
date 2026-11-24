# Quant Nexus AI

**Laboratorio de escritorio para análisis de criptomonedas y simulación de estrategias en Windows.** Quant Nexus AI combina datos públicos de mercado, una cartera de práctica y consultas con IA que muestran las fuentes detrás del análisis.

[Descargar para Windows x64](https://github.com/Daniel-ux1026/quant-nexus-ai/releases/tag/v0.4.0) · [Arquitectura](ARCHITECTURE.md) · [Seguridad](SECURITY.md)

## Qué puedes hacer

| Función | Qué ofrece |
| --- | --- |
| **Consulta IA** | Escenarios condicionales de Long, Short o Esperar para pares USDT de Binance Spot y Perpetuo. Combina velas cerradas e indicadores con fuentes macroeconómicas, políticas y regulatorias. |
| **Contexto y sentimiento** | Índice diario de miedo y codicia de Bitcoin y una muestra separada de publicaciones recientes de X cuando hay enlaces verificables. [Alternative.me](https://alternative.me/crypto/fear-and-greed-index/) |
| **Observación automática** | Escáner de BTC/USDT, ETH/USDT y SOL/USDT en Binance Spot. Evalúa tendencia, ruptura, volumen y riesgo al cierre de cada vela de un minuto. |
| **Paper trading** | Cartera persistente con capital simulado, posiciones, valor de cartera y registro de operaciones. Incluye supuestos de comisión, deslizamiento y límites de riesgo. |
| **Reproducción y backtesting** | Evalúa las reglas sobre velas históricas, revisa costes y resultados, y usa datos sintéticos para practicar sin conexión. |
| **Datos de mercado** | Visualiza precio, volumen e indicadores con datos públicos de Binance, Bitget y Bitunix. El contexto de Binance incluye profundidad, funding e interés abierto cuando están disponibles. |

Los escenarios de IA son hipótesis de análisis. El motor de cartera opera exclusivamente con capital simulado.

## Instalar en Windows x64

1. Abre la [versión 0.4.0](https://github.com/Daniel-ux1026/quant-nexus-ai/releases/tag/v0.4.0).
2. Descarga **Quant-Nexus-AI-0.4.0-Windows-x64-setup.exe** para instalar desde el menú Inicio. Si prefieres abrir la app directamente, usa **quant-nexus-ai.exe**.
3. Ten disponible [Microsoft Edge WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/). Las notas de la versión incluyen las sumas SHA-256 de ambas descargas y el estado de firma.

## Primer uso

**Observación y simulación.** Introduce un capital simulado en USDT. Elige **Observación** para ver las señales o **Paper trading** para registrar operaciones simuladas. Consulta el [manual de paper trading y backtesting](docs/PAPER-TRADING.md) para conocer las reglas y los supuestos de coste.

**Consulta con IA.** Selecciona el par, el mercado y la temporalidad. Puedes escribir intervalos como `15m`, `45m`, `4 horas` o `1 mes`. Los intervalos personalizados requieren al menos 21 velas cerradas y continuas. Al consultar, revisa la hora del último cierre, los indicadores y los enlaces. Una hipótesis de ruptura incluye entrada, invalidación y objetivo antes de costes. En Spot, **Short** expresa un sesgo bajista.

## Configurar OpenAI

La consulta con IA y la explicación del escáner requieren una clave de OpenAI API. El resto de las funciones puede usarse sin esa integración.

1. Abre **Administrador de credenciales → Credenciales de Windows → Agregar una credencial genérica**.
2. Usa `Quant Nexus AI/OpenAI API` como dirección y `OpenAI` como usuario. Guarda la clave en el campo de contraseña.
3. En Quant Nexus AI, abre **OpenAI → Probar conexión**. [Claves de OpenAI API](https://platform.openai.com/api-keys)

Las consultas de IA consumen cuota del proyecto de OpenAI. El motor nativo usa `store: false` y envía el par, las velas, los indicadores públicos y el enfoque que escribas. Conserva el enfoque para preguntas de mercado. El historial y la cartera se guardan localmente en SQLite. [Controles de seguridad](SECURITY.md)

## Compilar y probar

Se necesita Windows x64 con Node.js 24, npm, Rust estable para MSVC, herramientas de compilación C++ y WebView2. [Prerrequisitos de Tauri](https://v2.tauri.app/start/prerequisites/)

```powershell
git clone https://github.com/Daniel-ux1026/quant-nexus-ai.git
cd quant-nexus-ai
npm.cmd ci --ignore-scripts
npm.cmd run desktop
```

La compilación de distribución con NSIS escribe el ejecutable y el instalador debajo de `apps/desktop/src-tauri/target/release/`.

```powershell
npm.cmd run bundle -- --bundles nsis --ci
```

```powershell
cargo test --locked
cargo test --locked --manifest-path apps/desktop/src-tauri/Cargo.toml
npm.cmd run build
```

Las pruebas de conectividad usan fuentes públicas y se ejecutan por separado con `cargo test --locked --manifest-path apps/desktop/src-tauri/Cargo.toml -- --ignored --test-threads=1`.

## Estructura del proyecto

| Ruta | Responsabilidad |
| --- | --- |
| [crates/core](crates/core) | Motor Rust de mercado, validación de velas, indicadores, riesgo y simulación. |
| [apps/desktop/src-tauri](apps/desktop/src-tauri) | Backend Tauri, adaptadores HTTPS y WebSocket, consultas de IA y persistencia SQLite. |
| [apps/desktop/ui](apps/desktop/ui) | Interfaz React y TypeScript para el escáner, el gráfico, la cartera y Consulta IA. |
| [docs](docs) | Manual de usuario y registro de verificación por versión. |

[Roadmap](ROADMAP.md) · [Verificación de 0.3.0](docs/VERIFICACION.md) · [Compilación de Windows en GitHub Actions](https://github.com/Daniel-ux1026/quant-nexus-ai/actions/workflows/windows.yml)
