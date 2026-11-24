# Seguridad

El primer entorno utiliza datos públicos y cálculo de riesgo con capital simulado. La interfaz de lectura de mercado tiene hosts y rutas fijos. Las solicitudes usan TLS, tiempo de espera y límite de respuesta. Los fixtures usados en pruebas y demostraciones conservan la etiqueta de datos sintéticos.

La prueba opcional de OpenAI lee una credencial genérica desde Windows Credential Manager. La clave viaja por una tubería de entrada al cliente HTTPS, fuera de la línea de comandos, del frontend, de SQLite y de los registros. El proceso Rust limpia su copia temporal de la cabecera. El comando usa un destino y una carga fijos y devuelve solo el resultado de la prueba a la interfaz. Cada pulsación inicia como máximo una solicitud, con control de concurrencia y sin reintentos. [CredReadW](https://learn.microsoft.com/en-us/windows/win32/api/wincred/nf-wincred-credreadw) · [Responses API](https://developers.openai.com/api/reference/resources/responses/methods/create)

La explicación bajo demanda envía exclusivamente el par, la marca de tiempo, el cierre, los indicadores públicos y el motivo de la observación. El backend exige que la vela solicitada siga vigente. GPT devuelve texto para la pantalla con un presupuesto de salida acotado. El cálculo y las decisiones de riesgo se ejecutan en el núcleo local.

Consulta IA usa la misma credencial de Windows y envía a OpenAI la instantánea pública del par, los indicadores, el índice de Bitcoin y el enfoque opcional con `store: false`. El motor limita las búsquedas a dominios de fuentes oficiales y X. Los enlaces de las respuestas se validan antes de abrirlos en el navegador. El informe funciona como análisis independiente de la cartera.

Las decisiones del motor deben conservar su procedencia. Ante datos externos inválidos, huecos o retrasos, el flujo devuelve un estado de bloqueo. Los payloads de red se tratan como datos y los mensajes de noticias o modelos se procesarán como texto no confiable.

| Riesgo | Control |
| --- | --- |
| Entrada errónea, decimal inválido o desbordamiento | Validación de dominio y cálculo decimal comprobado. |
| Vela abierta, atraso u orden incoherente de eventos | Validación temporal y rechazo de datos incoherentes. |
| Exposición accidental al exchange | Conector inicial público y de solo lectura. |
| Repetición de órdenes en fases futuras | Intenciones idempotentes y conciliación después de respuestas inciertas. |
| Fuga de credenciales en fases futuras | Windows Credential Manager o DPAPI y permisos mínimos. Exclusión de secretos de logs y proveedores de IA. |

La fase de órdenes privadas requiere reconciliación antes de reanudar, apagado de emergencia, verificaciones de protección y pruebas de desconexión, duplicados, órdenes parciales y reinicio. La activación real requerirá autorización explícita.

La cartera persistente de esta versión procesa órdenes simuladas exclusivamente. Aplica límites de exposición y pérdida. El checkpoint y los eventos se confirman juntos en SQLite. El botón de cierre solicita la liquidación simulada en la próxima vela sincronizada. Los titulares públicos se muestran como texto. El lector RSS rechaza declaraciones de tipo externo.
