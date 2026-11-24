use std::path::PathBuf;
use std::process::Stdio;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

const RESPONSE_LIMIT: usize = 65_536;
const STATUS_LENGTH: usize = 3;

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

pub async fn request(url: &str, post: Option<(&str, &[u8])>) -> Result<Response, String> {
    request_with_limits(url, post, 15, RESPONSE_LIMIT).await
}

pub async fn request_with_limits(
    url: &str,
    post: Option<(&str, &[u8])>,
    seconds: u16,
    response_limit: usize,
) -> Result<Response, String> {
    let system_root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .ok_or("Windows no indicó una ubicación válida para su cliente HTTPS.")?;
    let mut command = Command::new(system_root.join("System32").join("curl.exe"));
    command
        .args([
            "--disable",
            "--silent",
            "--proto",
            "=https",
            "--connect-timeout",
            "5",
            "--max-time",
            &seconds.to_string(),
            "--max-filesize",
            &response_limit.to_string(),
            "--write-out",
            "%{http_code}",
            "--url",
            url,
        ])
        .env_remove("SSLKEYLOGFILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    if let Some((json, _)) = post {
        command
            .args([
                "--header",
                "Content-Type: application/json",
                "--header",
                "@-",
                "--data-binary",
                json,
            ])
            .stdin(Stdio::piped());
    }

    let mut child = command
        .spawn()
        .map_err(|_| "No se pudo iniciar el cliente HTTPS de Windows.")?;
    if let Some((_, secret_header)) = post {
        let mut input = child
            .stdin
            .take()
            .ok_or("No se pudo inicializar el canal de autenticación.")?;
        input
            .write_all(secret_header)
            .await
            .map_err(|_| "El cliente HTTPS no pudo recibir la autenticación.")?;
    }

    let mut body = Vec::new();
    child
        .stdout
        .take()
        .ok_or("No se pudo leer la respuesta HTTPS.")?
        .take((response_limit + STATUS_LENGTH + 1) as u64)
        .read_to_end(&mut body)
        .await
        .map_err(|_| "La respuesta HTTPS se interrumpió.")?;
    if body.len() > response_limit + STATUS_LENGTH {
        return Err("La respuesta HTTPS superó el límite de tamaño.".into());
    }

    let status = child
        .wait()
        .await
        .map_err(|_| "La conexión HTTPS se interrumpió.")?;
    if !status.success() {
        return Err(format!(
            "La conexión HTTPS falló con código {}.",
            status.code().unwrap_or(-1)
        ));
    }
    let body_len = body
        .len()
        .checked_sub(STATUS_LENGTH)
        .ok_or("La respuesta no incluyó estado HTTP.")?;
    let status = std::str::from_utf8(&body[body_len..])
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| (100..600).contains(value))
        .ok_or("La respuesta incluyó un estado HTTP inválido.")?;
    body.truncate(body_len);
    Ok(Response { status, body })
}
