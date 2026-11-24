# Arquitectura de Quant Nexus AI

La especificación principal enumera 14 agentes y se toma como referencia funcional. Se implementarán por fases como módulos especializados. El motor de datos, cálculo financiero y riesgo será determinista. Los proveedores de IA serán opcionales y estarán fuera de la ruta que autoriza órdenes.

| Componente | Diseño inicial |
| --- | --- |
| Motor | Rust. El núcleo define contratos y reglas. Tokio se incorpora con los adaptadores asíncronos. |
| Datos | Interfaz común de lectura pública. Los adaptadores verifican el contrato del exchange y normalizan símbolos, timestamps y decimales. |
| Persistencia | SQLite con WAL para estado local y auditoría. DuckDB y Parquet se incorporan con los datasets históricos que lo requieran. |
| Escritorio | Tauri 2, React y TypeScript. La interfaz consume comandos acotados y muestra el modo y la procedencia de datos. |
| IA | Python para investigación y entrenamiento. Inferencia local opcional con versiones sin permisos de ejecución. |

## Contratos

| Mensaje | Campos mínimos |
| --- | --- |
| Instrument | Exchange, tipo de mercado, activo base, moneda cotizada, precisión y filtros disponibles. |
| MarketCandle | Instrumento, intervalo, apertura UTC en ms, cierre UTC en ms, OHLCV decimal y procedencia. |
| Signal | Instrumento, versión de estrategia, timestamp de datos, vencimiento y evidencia. |
| RiskDecision | Identificador de intención, versión de política, decisión, motivos, límites y snapshot de portafolio. |
| PaperFill | Identificador de orden simulada, timestamp, cantidad, precio, comisión, modelo de costo y procedencia. |
| ExchangeOrderStatus | Identificador local, identificador del exchange, estado y cantidad ejecutada acumulada. Contrato de una fase futura. |

Los importes cruzarán las fronteras de serialización como cadenas decimales. El núcleo rechaza velas inválidas y valida barras cerradas antes de entregarlas al análisis. El tiempo de mercado y el de recepción tienen roles distintos. El replay usa el tiempo de mercado como reloj explícito. Para datos en vivo se verifican vigencia, continuidad y fuente. Cada vela se identifica por instrumento, intervalo y apertura.

En las fases con streaming, cada fuente tendrá capacidad de cola limitada. Un hueco o desbordamiento invalidará la información hasta completar la sincronización. El historial de mercado es reproducible. Los registros de ejecución y balances simulados se guardarán aparte de cualquier entorno real.

## Decisiones iniciales

- Spot USDT sin margen, apalancamiento ni posiciones cortas en el simulador inicial.
- El motor de riesgo recibe propuestas a través de un contrato ajeno a modelos generativos. El ejecutor futuro consume autorizaciones de riesgo, y la simulación se mantiene separada.
- Comenzar por velas públicas de un exchange permite verificar el contrato completo antes de sumar los demás y el streaming.
- Las capacidades suben de nivel tras validación con fixtures y pruebas reales del endpoint correspondiente.

| Fuente Spot | Documentación | Alcance inicial |
| --- | --- | --- |
| Binance | [API de datos de mercado](https://developers.binance.com/en/docs/catalog/core-trading-spot-trading/api/rest-api/market) | Primer adaptador de velas públicas. |
| Bitget | [API de datos de mercado](https://www.bitget.com/docs/catalog/market/market-data) | Velas públicas Spot mediante REST. |
| Bitunix | [API Spot pública](https://www.bitunix.com/api-docs/spots/en_us/public/) | Historial público Spot mediante REST. |

En 0.3 las tres fuentes ofrecen consultas Spot REST. Las barras de Bitunix con OHLC inconsistente quedan fuera del tramo reciente y continuo que se acepta. SQLite usa el exchange como parte de la identidad primaria de las velas. El flujo continuo y la cartera operan con Binance Spot.

## Ruta de observación automática

Una sesión Tokio abre el historial SQLite, sincroniza hasta 100 velas recientes por par con REST y mantiene una conexión WSS combinada para BTC, ETH y SOL contra USDT. El proceso lee únicamente velas cerradas de un minuto para analizarlas y guardar cada nueva vela antes de publicar su observación. Una revisión, un hueco o una interrupción vacía las señales visibles. La sesión reintenta con espera creciente, repara faltantes recientes mediante REST y comprueba la continuidad antes de volver al estado En directo. Para una interrupción superior a 24 horas, se sincroniza una ventana nueva y las fechas faltantes permanecen explícitas en la reproducción.

El mismo analizador procesa el flujo y la reproducción histórica. Una vela solo usa su cierre y hasta 20 velas anteriores. El tamaño se calcula a partir del capital simulado, con cantidades decimales y supuestos fijos de costo. La reproducción exige un tramo completo y empieza sus indicadores en la primera vela seleccionada. En paper trading, la cartera vuelve a calcular la exposición al admitir cada entrada.

## Cartera persistente y backtest

La cartera y el backtest comparten `PaperAccount`. La cartera en directo requiere las tres velas cerradas del mismo minuto y ejecuta un orden estable por símbolo. Una intención va de un cierre a la próxima apertura. Las operaciones y el checkpoint se escriben en una transacción. Una revisión de SQLite impide sobrescribir una cuenta con una copia anterior. Tras reconectar se recuperan stops y objetivos con velas completas y se bloquean las compras durante la recuperación.

El contexto público contiene instantáneas del libro Spot y los futuros USDⓈ-M de Binance, además de comunicados oficiales de la Reserva Federal. La vista organiza el análisis técnico en grupos completos de 1, 5 y 15 minutos. Es una ruta informativa separada de la cartera. [Datos públicos de Binance](https://developers.binance.com/en/docs/catalog/core-trading-derivatives-trading-usd-s-m-futures/api/rest-api/market-data) · [Feeds de la Reserva Federal](https://www.federalreserve.gov/feeds/feeds.htm)
