# Roadmap

| Fase | Entrega | Criterio de aceptación |
| --- | --- | --- |
| Fundamentos | Núcleo Rust, adaptadores públicos Spot USDT, vista de mercado local, persistencia y estado de conectividad. | Datos identificados por fuente, persistencia idempotente, errores visibles y comparación del payload con las API oficiales. |
| Inteligencia de mercado | Escáner, análisis técnico, estructura y volumen. | Resultados reproducibles para barras cerradas. Pruebas de indicadores, huecos de datos y temporalidades. |
| Simulación | Paper Trading, backtesting y gestión de riesgo. | Ledger reproducible con costes y gestión de posiciones. Pruebas de límites, datos vencidos, drawdown y fallos. |
| Coordinación | Agentes especializados, contexto macro, noticias y modelos cuantitativos. | Mensajes trazables, validación temporal fuera de muestra y aislamiento del ejecutor. |
| Integración privada | Adaptadores de pruebas, conciliación y ejecución semiautomática. | Casos de timeout, duplicados, estado incierto, órdenes parciales y reinicio cubiertos en donde exista un entorno de prueba. |
| Producción controlada | Ejecución real opcional y supervisada. | Autorización explícita, controles de riesgo verificados y auditoría de incidentes. |
| Distribución | Ejecutable e instalador de Windows. | Instalación y arranque verificados en Windows 10 y 11, actualizaciones verificables y manuales. |

La primera iteración valida una ruta de datos y simulación pequeña con BTC/USDT. La cobertura de tres exchanges, el instalador y los agentes se completan en sus respectivas fases. Los objetivos de rendimiento se medirán en el hardware real antes de fijar tamaños de cartera o número de activos.

## Validación de la versión 0.1.0

- Ejecutable de prueba compilado con Tauri para Windows x64.
- Consulta pública verificada con BTC/USDT en Binance. La prueba comprueba que la serie es reciente y contiene velas cerradas.
- Ocho pruebas del núcleo y cuatro de escritorio superadas. Cubren validación de datos, cálculo decimal, escritura idempotente y manejo del diagnóstico de OpenAI.
- Interfaz probada en Edge con respuestas locales para el gráfico, el cálculo de riesgo y el botón de OpenAI.
- Conexión autenticada con OpenAI verificada desde el ejecutable de Windows con GPT-5.6 Luna y una credencial local.
- Los adaptadores de Bitget y Bitunix, el registro de operaciones simuladas y el instalador continúan en el roadmap.

## Validación de la versión 0.2.0

- Observación automática de BTC/USDT, ETH/USDT y SOL/USDT en Binance Spot a partir de un capital simulado. El panel muestra los motivos del escáner y permite inspeccionar supuestos, indicadores y escenarios.
- Recuperación REST y WebSocket público verificados con Binance. La prueba de recuperación reconstruyó 120 velas consecutivas y repitió la sincronización sin duplicar filas.
- Reproducción verificada desde el ejecutable de Windows con 60 velas públicas guardadas. Se comprobó el avance vela por vela.
- Once pruebas del núcleo y siete de escritorio superadas, además de tres pruebas de integración pública. La evidencia de GPT se limita a los indicadores del filtro que se activó.
- El siguiente paso es el registro de operaciones de paper trading con saldo, posiciones, costos y métricas de desempeño.

## Cobertura de la versión 0.3.0

| Área | Disponible | Pendiente para la plataforma completa |
| --- | --- | --- |
| Datos de mercado | WebSocket de Binance Spot, REST Spot de Binance, Bitget y Bitunix, archivo SQLite por fuente y descarga histórica por tramo de Binance. | Streaming en los otros exchanges, cobertura de perpetuos y pruebas de escala con cientos de pares. |
| Análisis de mercado | Indicadores a 1, 5 y 15 minutos, soporte y resistencia por rango, profundidad Spot y datos de futuros USDⓈ-M de Binance. | Indicadores y temporalidades restantes, flujo de operaciones histórico y comparación de derivados entre exchanges. |
| Riesgo y simulación | Cartera persistente, pausa, cierre simulado, riesgo diario y semanal, drawdown, exposición, backtesting reproducible y métricas. | Estrategias adicionales, ejecución parcial y libro histórico, stress tests y validación prolongada. |
| Inteligencia | GPT para explicación bajo demanda, titulares oficiales de política monetaria y regresión logística experimental con evaluación temporal. | Calendario macroeconómico, feeds adicionales, sentimiento, modelos avanzados, supervisión y registro de modelos. |
| Ejecución privada | Punto de control operativo en simulación. | Autenticación y conciliación en testnet, órdenes de prueba, modo semiautomático y validación para activar órdenes reales. |
| Distribución | Ejecutable gráfico, instalador NSIS, manual de simulación y flujo de compilación de GitHub Actions. | Firma digital, servidor de actualizaciones verificadas, pruebas de instalación en Windows 10 y 11 y despliegue opcional en VPS. |
