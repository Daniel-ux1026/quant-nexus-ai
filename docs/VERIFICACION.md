# Verificación de Quant Nexus AI 0.3.0

## Pruebas ejecutadas en Windows x64

| Prueba | Resultado |
| --- | --- |
| Núcleo Rust | 19 pruebas aprobadas. Incluye contabilidad decimal, siguiente apertura, recuperación de huecos, cierre simulado, indicador multitemporal y separación temporal del modelo. |
| Backend de escritorio | 9 pruebas aprobadas. Incluye SQLite con control de revisión, aislamiento de fuentes y análisis de payloads públicos. |
| Integraciones públicas | 6 pruebas aprobadas. Incluye REST y WebSocket de Binance, Bitget, Bitunix, contexto de Binance y comunicados monetarios de la Reserva Federal. |
| Interfaz | Compilación TypeScript y Vite aprobada. El ejecutable gráfico abrió en Windows y mostró el backtest sintético, sus métricas y la curva de la cartera. |

Las 6 pruebas de red se ejecutaron aparte. Una recuperó 120 velas consecutivas y volvió a guardarlas para comprobar idempotencia. Otra descargó 360 velas públicas de BTC/USDT y generó un backtest y tres bloques de prueba cronológicos.

## Resultado de una muestra histórica

La ejecución de validación usó seis horas de datos de Binance Spot del 24 de noviembre de 2026, con 1,000 USDT simulados. Produjo un PnL neto de **−4.47 USDT**. El Brier del modelo experimental fue **0.050255** frente a **0.050547** de la referencia en 168 observaciones fuera de muestra. Estas cifras corresponden al tramo y a las reglas probadas.

## Límites de la verificación

La sesión de pruebas usó datos públicos y simulación. La firma digital, la instalación en equipos independientes con Windows 10 y 11 y la integración autenticada de exchanges quedan pendientes. El [roadmap](../ROADMAP.md) indica el alcance por fase.

## Archivos generados

| Archivo | SHA-256 |
| --- | --- |
| `quant-nexus-ai.exe` | `A03ACA0F218C798E1D38726CAFC4D616B578C7EB790BDAF59187EA3EBA23E6C5` |
| `Quant Nexus AI_0.3.0_x64-setup.exe` | `316DAC28FE05166ED401A2D81882F2946070B672F28B3E99305711E2234DA781` |

El instalador se compiló con NSIS y su estado Authenticode es **sin firma**. El ejecutable declara el subsistema gráfico de Windows.
