# Paper trading y backtesting

## Cartera simulada en directo

Selecciona **Paper trading** en el panel automático, introduce tu capital simulado y pulsa **Iniciar observación**. El escáner reúne una vela cerrada del mismo minuto para cada uno de los tres pares de Binance Spot. La cartera evalúa el lote en un orden estable y solo considera una señal para la apertura de la siguiente vela.

La cuenta se identifica por su capital inicial normalizado. Si vuelves a iniciar paper trading con el mismo importe, se carga el saldo y registro guardados. Con otro importe se usa otra cuenta simulada. El panel de cartera muestra la última cuenta utilizada cuando el escáner está detenido.

**Pausar nuevas entradas** impide compras nuevas y mantiene activos los stops y objetivos simulados. **Detener observación** detiene el flujo y conserva la cuenta. Al reanudar tras un hueco recuperable se procesan las protecciones pendientes usando el archivo de velas completas. La recuperación histórica no abre nuevas posiciones. La sesión queda bloqueada si le faltan velas esenciales o el hueco supera 24 horas.

**Cerrar posiciones simuladas** pausa las entradas y solicita el cierre de posiciones que sigan abiertas en la próxima vela sincronizada. La app debe seguir recibiendo datos para procesar esa solicitud.

## Controles modelados

| Control | Regla |
| --- | --- |
| Riesgo de entrada | Hasta 0.25% del valor de la cartera, con objetivo de al menos 2 veces el riesgo después de costos. |
| Exposición | Hasta 30% de la cartera por entrada y 75% agregado al admitir nuevas entradas. |
| Costos | Comisión supuesta de 0.10% y deslizamiento de 0.05% por lado. |
| Límites de pérdida | Bloqueo diario al 1%, semanal al 3% y de drawdown al 5%. Día y semana se calculan en UTC. |
| Stops y objetivos | Si una vela toca ambos, se usa el stop. Una apertura que cruza por debajo del stop se simula a la apertura con deslizamiento. |
| Contabilidad | El saldo se reduce al comprar. Las posiciones abiertas se marcan por lo que se obtendría tras costos de salida. |

Los costos y reglas de ejecución corresponden a un modelo de velas. El libro de órdenes y los titulares públicos son información contextual.

## Reproducción y backtest

En **Reproducción histórica**, carga el historial local de un exchange o el ejemplo sintético. El gráfico se recorre vela por vela. Debajo aparece el backtest del tramo completo con curva de capital, actividad, PnL neto, drawdown, porcentaje de ganadoras, factor de beneficio, expectativa y ratios Sharpe y Sortino por vela. Las métricas sin observaciones suficientes muestran un guion.

La referencia Buy & Hold compra en la primera apertura y valora una salida en el último cierre, con costos. Las métricas del backtest corresponden al tramo elegido y a una estrategia de ruptura con volumen.

Con al menos 325 velas seguidas, se muestra un informe de investigación logística en tres tramos cronológicos. La etiqueta evalúa el resultado neto de entrada y salida separados por cinco velas. Cada bloque ajusta la escala solo con el historial anterior y deja cinco muestras entre el ajuste y la prueba. El informe compara el error Brier con la frecuencia observada en el entrenamiento. El modelo es experimental y sus resultados se muestran aparte del motor de señales.
