use quant_nexus_core::scanner::{ObservationReason, ScanObservation};
use serde_json::{Value, json};
use windows_sys::Win32::Security::Credentials::{
    CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW,
};
use zeroize::Zeroizing;

use crate::https;

const CREDENTIAL_NAME: &str = "Quant Nexus AI/OpenAI API";
const MAX_KEY_LENGTH: usize = 512;
const API_URL: &str = "https://api.openai.com/v1/responses";
pub const MODEL: &str = "gpt-5.6-luna";

pub async fn test_connection() -> Result<&'static str, String> {
    confirmed_response(&request_response("Reply with exactly OK.", 16).await?)?;
    Ok(MODEL)
}

pub async fn explain(pair: &str, observation: &ScanObservation) -> Result<String, String> {
    let evidence = explanation_evidence(pair, observation);
    let prompt = format!(
        "Explica en español y en dos frases la observación de un escáner de simulación Spot. La causa registrada por el motor está en motivo y debe ser la explicación principal. Usa solo las cifras proporcionadas para aclararla. Evita sugerir operaciones o estimar rentabilidad. Evidencia: {evidence}"
    );
    response_text(&request_response(&prompt, 160).await?)
}

fn explanation_evidence(pair: &str, observation: &ScanObservation) -> Value {
    use ObservationReason::*;
    let reason = match observation.reason {
        WarmingUp => "El escáner espera a reunir 21 velas consecutivas.",
        NoUptrend => "Las SMA de 9 y 21 velas y el cierre no confirman una tendencia alcista.",
        NoBreakout => "El cierre quedó en el máximo de la vela anterior o por debajo.",
        LowVolume => {
            "El volumen relativo quedó por debajo del mínimo de 1.25 o carece de referencia."
        }
        FlatRange => "El rango medio de 14 velas no permite estimar el stop.",
        RiskRejected => "El motor de riesgo descartó el escenario calculado.",
        BreakoutWithVolume => {
            "Una ruptura del máximo previo con tendencia alcista y volumen suficiente generó un escenario simulado."
        }
    };
    let metrics = observation
        .evidence
        .as_ref()
        .and_then(|metrics| match observation.reason {
            WarmingUp | RiskRejected => None,
            NoUptrend => Some(json!({ "sma_9": metrics.sma_fast, "sma_21": metrics.sma_slow })),
            NoBreakout => Some(json!({ "maximo_previo": metrics.prior_high })),
            LowVolume => Some(json!({ "volumen_relativo": metrics.relative_volume })),
            FlatRange => Some(json!({ "rango_medio_14": metrics.atr })),
            BreakoutWithVolume => Some(json!(metrics)),
        });
    json!({
        "pair": pair,
        "close_time_ms": observation.close_time_ms,
        "close": observation.close,
        "motivo": reason,
        "indicadores": metrics,
    })
}

async fn request_response(input: &str, max_output_tokens: u16) -> Result<Vec<u8>, String> {
    let request = json!({
        "model": MODEL,
        "input": input,
        "reasoning": { "effort": "none" },
        "max_output_tokens": max_output_tokens,
        "store": false
    });
    send(&request, 15, 65_536)
        .await
        .map_err(|(status, message)| {
            if status == 0 {
                message
            } else {
                response_error(status, message.as_bytes())
            }
        })
}

pub async fn consult(request: &Value) -> Result<Value, String> {
    let body = send(request, 75, 512 * 1_024)
        .await
        .map_err(|(status, body)| match status {
            0 => body,
            400 => "OpenAI rechazó la consulta o la búsqueda web. HTTP 400.".into(),
            403 => "El proyecto de OpenAI no tiene permiso para esta consulta. HTTP 403.".into(),
            404 => {
                "El modelo de consulta no está disponible para este proyecto de OpenAI. HTTP 404."
                    .into()
            }
            _ => response_error(status, body.as_bytes()).replace("la prueba", "la consulta"),
        })?;
    serde_json::from_slice(&body).map_err(|_| "OpenAI devolvió una respuesta inválida.".into())
}

async fn send(
    request: &Value,
    seconds: u16,
    response_limit: usize,
) -> Result<Vec<u8>, (u16, String)> {
    let header = read_authorization_header().map_err(|reason| (0, reason))?;
    let response = https::request_with_limits(
        API_URL,
        Some((&request.to_string(), &header)),
        seconds,
        response_limit,
    )
    .await
    .map_err(|reason| (0, reason))?;
    drop(header);
    if response.status == 200 {
        return Ok(response.body);
    }
    Err((
        response.status,
        String::from_utf8_lossy(&response.body).into_owned(),
    ))
}

fn response_error(status: u16, body: &[u8]) -> String {
    let details: Option<Value> = serde_json::from_slice(body).ok();
    let code = details
        .as_ref()
        .and_then(|value| value["error"]["code"].as_str());
    let failure = match (status, code) {
        (404, Some("model_not_found")) => {
            "El modelo de prueba no está disponible para el proyecto de OpenAI."
        }
        (429, Some("insufficient_quota")) => {
            "El proyecto de OpenAI no tiene cuota de API disponible."
        }
        (401, _) => "OpenAI rechazó la credencial de Windows.",
        (403, _) => "La credencial no tiene acceso a esta prueba de OpenAI.",
        (429, _) => "OpenAI aplicó un límite de cuota o frecuencia.",
        _ => "OpenAI no pudo completar la prueba.",
    };
    format!("{failure} HTTP {status}.")
}

struct Credential(*mut CREDENTIALW);

impl Drop for Credential {
    fn drop(&mut self) {
        // CredReadW devuelve un bloque que se libera con CredFree.
        unsafe { CredFree(self.0.cast()) };
    }
}

fn read_authorization_header() -> Result<Zeroizing<Vec<u8>>, String> {
    let target: Vec<u16> = CREDENTIAL_NAME.encode_utf16().chain([0]).collect();
    let mut raw = std::ptr::null_mut();
    // El puntero de destino es UTF-16 terminado en cero. La API inicializa raw en caso de éxito.
    let found = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) };
    if found == 0 || raw.is_null() {
        return Err(format!(
            "Agrega la credencial genérica «{CREDENTIAL_NAME}» en Windows."
        ));
    }
    let credential = Credential(raw);
    // El bloque está vivo hasta que se libera credential.
    let (data, size) = unsafe {
        (
            (*credential.0).CredentialBlob,
            (*credential.0).CredentialBlobSize as usize,
        )
    };
    if data.is_null() || size == 0 || size > 2 * (MAX_KEY_LENGTH + 1) {
        return Err("La credencial de OpenAI tiene un formato inválido.".into());
    }
    // Windows guarda la contraseña genérica como bytes UTF-16LE.
    let bytes = unsafe { std::slice::from_raw_parts(data, size) };
    authorization_header(bytes)
        .ok_or_else(|| "La credencial de OpenAI tiene un formato inválido.".into())
}

fn authorization_header(blob: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let blob = blob.strip_suffix(&[0, 0]).unwrap_or(blob);
    let mut pairs = blob.chunks_exact(2);
    if !pairs.remainder().is_empty() || pairs.len() > MAX_KEY_LENGTH {
        return None;
    }
    let prefix = b"Authorization: Bearer ";
    let mut header = Zeroizing::new(Vec::with_capacity(prefix.len() + pairs.len() + 1));
    header.extend_from_slice(prefix);
    if !pairs.by_ref().all(|pair| {
        let byte = pair[0];
        if pair[1] != 0 || !(byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') {
            return false;
        }
        header.push(byte);
        true
    }) {
        return None;
    }
    if !header[prefix.len()..].starts_with(b"sk-") || header.len() <= prefix.len() + 3 {
        return None;
    }
    header.push(b'\n');
    Some(header)
}

fn confirmed_response(body: &[u8]) -> Result<(), String> {
    if response_text(body)? == "OK" {
        Ok(())
    } else {
        Err("OpenAI respondió sin la confirmación prevista para la prueba.".into())
    }
}

fn response_text(body: &[u8]) -> Result<String, String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| "OpenAI devolvió una respuesta con formato inválido.")?;
    let output = value["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["content"].as_array())
        .flatten()
        .filter(|item| item["type"] == "output_text")
        .filter_map(|item| item["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if value["status"] == "completed" && !output.trim().is_empty() {
        Ok(output.trim().chars().take(1500).collect())
    } else {
        Err("OpenAI devolvió una respuesta sin texto completo.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_blob(value: &str) -> Vec<u8> {
        value.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn credential_transforms_only_a_single_valid_bearer_header() {
        let mut blob = windows_blob("sk-test_ONLY-000");
        blob.extend_from_slice(&[0, 0]);
        let header = authorization_header(&blob).unwrap();
        assert_eq!(
            header.as_slice(),
            b"Authorization: Bearer sk-test_ONLY-000\n"
        );
        assert!(authorization_header(&windows_blob("sk-bad\r\nInjected: yes")).is_none());
        assert!(authorization_header(&windows_blob("sk-")).is_none());
        assert!(authorization_header(&[b'x']).is_none());
    }

    #[test]
    fn verifies_only_the_expected_completed_response() {
        let good = br#"{"status":"completed","output":[{"content":[{"type":"output_text","text":"OK"}]}]}"#;
        assert!(confirmed_response(good).is_ok());
        let incomplete = br#"{"status":"incomplete","output":[{"content":[{"type":"output_text","text":"OK"}]}]}"#;
        assert!(confirmed_response(incomplete).is_err());
        assert!(confirmed_response(b"unavailable").is_err());
    }

    #[test]
    fn reports_provider_error_codes_without_repeating_provider_messages() {
        assert_eq!(
            response_error(
                429,
                br#"{"error":{"code":"insufficient_quota","message":"internal detail"}}"#
            ),
            "El proyecto de OpenAI no tiene cuota de API disponible. HTTP 429."
        );
        assert_eq!(
            response_error(404, br#"{"error":{"code":"model_not_found"}}"#),
            "El modelo de prueba no está disponible para el proyecto de OpenAI. HTTP 404."
        );
    }

    #[test]
    fn explanation_evidence_limits_metrics_to_the_rule_that_was_triggered() {
        use quant_nexus_core::{Decimal, market::synthetic_snapshot, scanner::evaluate};
        let snapshot = synthetic_snapshot().unwrap();
        let mut observation = evaluate(&snapshot.candles, Decimal::from(1000)).unwrap();
        observation.reason = ObservationReason::NoUptrend;
        let evidence = explanation_evidence(&snapshot.pair, &observation);
        assert!(evidence["indicadores"]["sma_9"].is_string());
        assert!(evidence["indicadores"].get("volumen_relativo").is_none());
        assert!(evidence.get("scenario").is_none());
        assert!(evidence.get("capital").is_none());
    }
}
