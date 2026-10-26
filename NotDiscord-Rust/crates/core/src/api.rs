use crate::{
    api_operations::{self, Operations},
    rate_limits::{RateLimits, Route, Wait},
    safety::{SafetyGate, SafetyState},
};
use futures_util::StreamExt;
pub use reqwest::Method;
use reqwest::{
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
    multipart::{Form, Part},
    redirect::Policy,
};
use serde_json::{Value, json};
use std::{fmt, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio::io::AsyncReadExt;
use url::Url;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Other,
    Network,
    Http,
    PermissionDenied,
    AuthenticationRequired,
    VerificationRequired,
    RateLimited,
    OutcomeUnknown,
    ProtocolUnsupported,
    QueueFull,
    OperationPending,
    OperationCompleted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceError {
    pub message: String,
    pub status: Option<u16>,
    pub kind: ErrorKind,
    pub retry_after: Option<Duration>,
}
impl ServiceError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            kind: ErrorKind::Other,
            retry_after: None,
        }
    }
    pub fn status(message: impl Into<String>, status: u16) -> Self {
        Self {
            message: message.into(),
            status: Some(status),
            kind: match status {
                401 => ErrorKind::AuthenticationRequired,
                403 => ErrorKind::PermissionDenied,
                429 => ErrorKind::RateLimited,
                _ => ErrorKind::Http,
            },
            retry_after: None,
        }
    }
    pub fn network() -> Self {
        Self::typed(
            ErrorKind::Network,
            "No se pudo completar la conexión con Discord.",
        )
    }
    pub fn typed(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            kind,
            retry_after: None,
        }
    }
    pub fn outcome_unknown() -> Self {
        Self::typed(
            ErrorKind::OutcomeUnknown,
            "No se pudo confirmar la operación. Comprueba su resultado en Discord antes de volver a enviarla; el borrador se conserva.",
        )
    }
}
impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ServiceError {}

/// The only authenticated HTTP client. It cannot send credentials to media hosts.
#[derive(Clone)]
pub struct Api {
    client: reqwest::Client,
    _credential: Arc<Zeroizing<String>>,
    gate: Arc<Semaphore>,
    pending: Arc<Semaphore>,
    safety: SafetyGate,
    rates: Arc<RateLimits>,
    operations: Arc<Operations>,
    base: Url,
    #[cfg(test)]
    test_endpoint: Option<Url>,
    #[cfg(test)]
    body_started: Option<Arc<tokio::sync::Notify>>,
}
impl Api {
    pub fn new(credential: String) -> Result<Self, ServiceError> {
        Self::new_with_safety(credential, SafetyGate::default())
    }
    pub fn new_with_safety(credential: String, safety: SafetyGate) -> Result<Self, ServiceError> {
        Self::new_with_secret(Zeroizing::new(credential), safety)
    }
    pub fn new_with_secret(
        credential: Zeroizing<String>,
        safety: SafetyGate,
    ) -> Result<Self, ServiceError> {
        if credential.len() < 20 || credential.len() > 4096 || credential.contains(['\r', '\n']) {
            return Err(ServiceError::new("La credencial no es válida."));
        }
        let credential = Arc::new(credential);
        let mut header = HeaderValue::from_str(credential.as_str())
            .map_err(|_| ServiceError::new("La credencial no es válida."))?;
        header.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, header);
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .user_agent("NotDiscordTerminal/0.2 Rust")
            .redirect(Policy::none())
            .timeout(Duration::from_secs(35))
            .build()
            .map_err(|_| ServiceError::network())?;
        Ok(Self {
            client,
            _credential: credential,
            gate: Arc::new(Semaphore::new(1)),
            pending: Arc::new(Semaphore::new(32)),
            safety,
            rates: Arc::new(RateLimits::default()),
            operations: Arc::new(Operations::default()),
            base: Url::parse("https://discord.com/api/v9/").unwrap(),
            #[cfg(test)]
            test_endpoint: None,
            #[cfg(test)]
            body_started: None,
        })
    }
    pub fn gateway(
        &self,
        initial_status: Option<String>,
    ) -> (
        crate::gateway::GatewayHandle,
        tokio::sync::mpsc::Receiver<crate::gateway::GatewayEvent>,
    ) {
        crate::gateway::spawn_with_safety(
            self._credential.to_string(),
            initial_status,
            self.safety(),
        )
    }
    pub fn safety(&self) -> SafetyGate {
        self.safety.clone()
    }
    fn endpoint(&self, path: &str) -> Result<Url, ServiceError> {
        let endpoint = validate_route(&self.base, path)?;
        #[cfg(test)]
        if let Some(test) = &self.test_endpoint {
            // This override is private and compiled only in unit tests; normal
            // routes are still validated first. The fixture owns its listener.
            return test.join(path).map_err(|_| ServiceError::network());
        }
        Ok(endpoint)
    }
    async fn begin(&self) -> Result<(SemaphorePermit<'_>, SemaphorePermit<'_>), ServiceError> {
        self.safety.check()?;
        let pending = self.pending.try_acquire().map_err(|_| {
            ServiceError::typed(
                ErrorKind::QueueFull,
                "Hay demasiadas operaciones pendientes. Espera antes de volver a intentarlo.",
            )
        })?;
        let permit = tokio::select! {
            biased;
            state = self.safety.wait_blocked() => return Err(state.error().unwrap()),
            permit = self.gate.acquire() => permit.map_err(|_| ServiceError::network())?,
        };
        self.safety.check()?;
        Ok((pending, permit))
    }
    async fn wait_route(&self, route: &Route) -> Result<(), ServiceError> {
        loop {
            self.safety.check()?;
            match self.rates.wait(route) {
                Wait::Ready => return Ok(()),
                Wait::Indefinite => return Err(self.rate_error(route)),
                Wait::Until(until)
                    if until.saturating_duration_since(tokio::time::Instant::now())
                        > Duration::from_secs(30) =>
                {
                    return Err(self.rate_error(route));
                }
                Wait::Until(until) => tokio::select! {
                    biased;
                    state = self.safety.wait_blocked() => return Err(state.error().unwrap()),
                    _ = tokio::time::sleep_until(until) => {},
                },
            }
        }
    }
    fn rate_error(&self, route: &Route) -> ServiceError {
        let mut error = status_error(429);
        error.retry_after = match self.rates.wait(route) {
            Wait::Until(until) => {
                Some(until.saturating_duration_since(tokio::time::Instant::now()))
            }
            Wait::Ready => Some(Duration::ZERO),
            Wait::Indefinite => None,
        };
        error
    }
    async fn receive(
        &self,
        response: reqwest::Response,
        route: &Route,
        success_limit: usize,
        safe_read: bool,
    ) -> Result<Value, ServiceError> {
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        // An authentication rejection is conclusive at the header. Do not let
        // a slow or incomplete body keep this session and its queue alive.
        if status == 401 {
            self.safety.block(SafetyState::AuthenticationRequired);
            self.rates.observe(route, status, &headers, None);
            return Err(status_error(status));
        }
        // A server error can follow a committed write. Without confirmation,
        // replaying it could create a duplicate even though an HTTP status exists.
        if !safe_read && status >= 500 {
            self.rates.observe(route, status, &headers, None);
            let mut error = ServiceError::outcome_unknown();
            error.status = Some(status);
            return Err(error);
        }
        #[cfg(test)]
        if let Some(started) = &self.body_started {
            started.notify_one();
        }
        let bytes = tokio::select! {
            biased;
            state = self.safety.wait_blocked() => return Err(if safe_read {state.error().unwrap()} else {ServiceError::outcome_unknown()}),
            bytes = read_bounded(response, if (200..300).contains(&status) {success_limit} else {64 * 1024}) => bytes,
        };
        let data = bytes
            .as_ref()
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
        self.rates.observe(route, status, &headers, data.as_ref());
        if matches!(status, 400 | 403)
            && (data.as_ref().is_some_and(is_verification)
                || !data.as_ref().is_some_and(Value::is_object))
        {
            // A truncated/oversized/invalid security response cannot safely be
            // classified as an ordinary permission denial. Pause conservatively
            // without claiming a confirmed CAPTCHA or retaining its payload.
            self.safety.block(SafetyState::VerificationRequired);
            let mut error = SafetyState::VerificationRequired.error().unwrap();
            error.status = Some(status);
            return Err(error);
        }
        if status == 429 {
            return Err(self.rate_error(route));
        }
        if status==413 || data.as_ref().is_some_and(|value|value["code"].as_u64()==Some(40005)) {
            return Err(ServiceError::status("Discord rechazó el tamaño del archivo o del envío. No se reintentará automáticamente; reduce el tamaño. El borrador se conserva.",status));
        }
        if !(200..300).contains(&status) {
            return Err(status_error(status));
        }
        let bytes = bytes?;
        if bytes.is_empty() {
            Ok(json!({}))
        } else {
            data.ok_or_else(|| ServiceError::new("Discord devolvió datos no válidos."))
        }
    }
    pub async fn get(&self, path: &str) -> Result<Value, ServiceError> {
        self.request(Method::GET, path, None).await
    }
    pub async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, ServiceError> {
        self.request(method, path, body).await
    }
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, ServiceError> {
        self.request_with_reason(method, path, body, None).await
    }
    /// Adds an optional reason to this request only. All writes still use the
    /// ordinary safety gate, rate limits and operation deduplication; changing
    /// a reason must not permit replaying an unconfirmed administrative action.
    pub async fn send_audited(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        reason: &str,
    ) -> Result<Value, ServiceError> {
        if reason.chars().count() > 512 {
            return Err(ServiceError::new("El motivo admite hasta 512 caracteres."));
        }
        let reason = if reason.is_empty() { None } else {
            let encoded = url::form_urlencoded::byte_serialize(reason.as_bytes())
                .collect::<String>().replace('+', "%20");
            let mut header = HeaderValue::from_str(&encoded)
                .map_err(|_| ServiceError::new("El motivo no es válido."))?;
            header.set_sensitive(true);
            Some(header)
        };
        self.request_with_reason(method, path, body, reason).await
    }
    async fn request_with_reason(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        audit_reason: Option<HeaderValue>,
    ) -> Result<Value, ServiceError> {
        let endpoint = self.endpoint(path)?;
        let route = Route::new(&method, path);
        let safe_read = method == Method::GET || method == Method::HEAD;
        self.safety.check()?;
        let mut operation = if safe_read {
            None
        } else {
            Some(self.operations.begin_for(
                api_operations::key(method.as_str(), endpoint.as_str(), body.as_ref()),
                method.as_str(),
            )?)
        };
        let _permits = self.begin().await?;
        for attempt in 0..3 {
            self.wait_route(&route).await?;
            let mut request = self.client.request(method.clone(), endpoint.clone());
            if let Some(reason) = &audit_reason {
                request = request.header("X-Audit-Log-Reason", reason.clone());
            }
            if let Some(body) = body.as_ref() {
                request = request.json(body)
            }
            if let Some(operation) = &mut operation {
                operation.sending();
            }
            let response = tokio::select! {
                biased;
                state = self.safety.wait_blocked() => return Err(if safe_read {state.error().unwrap()} else {ServiceError::outcome_unknown()}),
                response = request.send() => response.map_err(|_| if safe_read {ServiceError::network()} else {ServiceError::outcome_unknown()})?,
            };
            let result = self
                .receive(response, &route, 16 * 1024 * 1024, safe_read)
                .await;
            // Only a definite 429 on a read can retry automatically. The exact
            // deadline stays shared even when this call returns (e.g. 300s).
            if safe_read
                && attempt < 2
                && result.as_ref().err().is_some_and(|e| {
                    e.kind == ErrorKind::RateLimited
                        && e.retry_after
                            .is_some_and(|delay| delay <= Duration::from_secs(30))
                })
            {
                continue;
            }
            let result = result.map_err(|error| {
                if !safe_read
                    && matches!(error.kind, ErrorKind::Other | ErrorKind::Network)
                    && error.status.is_none()
                {
                    ServiceError::outcome_unknown()
                } else {
                    error
                }
            });
            if let Some(operation) = &mut operation {
                operation.finish(&result);
            }
            return result;
        }
        Err(ServiceError::new("Se agotaron los intentos permitidos."))
    }
    pub async fn upload_many(
        &self,
        channel: &str,
        paths: &[PathBuf],
        content: &str,
        reply_id: &str,
        nonce: &str,
    ) -> Result<Value, ServiceError> {
        self.upload_many_with_everyone(channel, paths, content, reply_id, nonce, false).await
    }
    pub async fn upload_many_with_everyone(
        &self, channel: &str, paths: &[PathBuf], content: &str, reply_id: &str, nonce: &str,
        mention_everyone: bool,
    ) -> Result<Value, ServiceError> {
        self.upload_many_with_limits(channel,paths,content,reply_id,nonce,mention_everyone,crate::uploads::UploadLimit::default()).await
    }
    pub async fn upload_many_with_limits(
        &self, channel:&str,paths:&[PathBuf],content:&str,reply_id:&str,nonce:&str,
        mention_everyone:bool,limit:crate::uploads::UploadLimit,
    )->Result<Value,ServiceError> {
        if !crate::parsing::valid_id(channel)
            || paths.is_empty()
            || paths.len() > 10
            || nonce.is_empty()
            || nonce.len() > 25
            || !reply_id.is_empty() && !crate::parsing::valid_id(reply_id)
        {
            return Err(ServiceError::new("Envío preparado inválido."));
        }
        self.safety.check()?;
        let path = format!("channels/{channel}/messages");
        let endpoint = self.endpoint(&path)?;
        let route = Route::new(&Method::POST, &path);
        let mut total = 0u64;
        let mut prepared=Vec::with_capacity(paths.len());
        for path in paths {
            let file=tokio::fs::File::open(path).await.map_err(|_|ServiceError::new("No se pudo abrir un adjunto. El borrador se conserva."))?;
            let metadata = file.metadata().await.map_err(|_| {
                ServiceError::new(
                    "Uno de los archivos ya no está disponible. El borrador se conserva.",
                )
            })?;
            if !path.is_absolute() || !metadata.is_file() {
                return Err(ServiceError::new(
                    "Uno de los archivos no existe o no es un archivo válido. El borrador se conserva.",
                ));
            }
            limit.validate_file(metadata.len()).map_err(ServiceError::new)?;
            total = total.saturating_add(metadata.len());
            prepared.push((file,metadata.len(),path.file_name().unwrap_or_default().to_string_lossy().into_owned()));
        }
        if total > crate::uploads::LOCAL_BATCH_BYTES {
            return Err(ServiceError::new("Este envío supera el máximo local de 1 GiB. Divide los adjuntos en mensajes separados."));
        }
        let mut body = json!({"content":content,"nonce":nonce,"enforce_nonce":true,"allowed_mentions":{"parse":["users","roles"],"replied_user":false},"attachments":paths.iter().enumerate().map(|(index,path)|json!({"id":index.to_string(),"filename":path.file_name().unwrap_or_default().to_string_lossy()})).collect::<Vec<_>>()});
        body["allowed_mentions"] = crate::mentions::allowed_mentions(content, mention_everyone);
        if !reply_id.is_empty() {
            body["message_reference"] =
                json!({"message_id":reply_id,"channel_id":channel,"fail_if_not_exists":true})
        }
        let mut operation = self.operations.begin_for(
            api_operations::key("POST", endpoint.as_str(), Some(&body)),
            "POST",
        )?;
        let _permits = self.begin().await?;
        self.wait_route(&route).await?;
        let mut form = Form::new().text("payload_json", body.to_string());
        for (index,(file,length,name)) in prepared.into_iter().enumerate() {
            // Keep the validated handle, and bound the actual stream even if
            // the source changes while waiting for a rate-limit permit.
            let stream=tokio_util::io::ReaderStream::new(file.take(length));
            let part=Part::stream_with_length(reqwest::Body::wrap_stream(stream),length).file_name(name);
            form = form.part(format!("files[{index}]"), part)
        }
        // Uploads are never retried: a lost acknowledgement must not duplicate a send.
        self.safety.check()?;
        operation.sending();
        let response = tokio::select! {
            biased;
            _ = self.safety.wait_blocked() => return Err(ServiceError::outcome_unknown()),
            response = self.client.post(endpoint).multipart(form).send() => response.map_err(|_|ServiceError::outcome_unknown())?,
        };
        let result = self
            .receive(response, &route, 1024 * 1024, false)
            .await
            .map_err(|error| {
                if matches!(error.kind, ErrorKind::Other | ErrorKind::Network)
                    && error.status.is_none()
                {
                    ServiceError::outcome_unknown()
                } else {
                    error
                }
            });
        operation.finish(&result);
        result
    }
}
pub fn validate_route(base: &Url, path: &str) -> Result<Url, ServiceError> {
    if path.len() > 8192
        || path.starts_with('/')
        || path.contains([':', '\\', '\r', '\n'])
        || path.split('?').next().unwrap_or("").contains("..")
    {
        return Err(ServiceError::new("Ruta API inválida."));
    }
    let endpoint = base
        .join(path)
        .map_err(|_| ServiceError::new("Ruta API inválida."))?;
    if endpoint.scheme() != "https"
        || endpoint.host_str() != Some("discord.com")
        || endpoint.port_or_known_default() != Some(443)
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || !endpoint.path().starts_with("/api/v9/")
    {
        return Err(ServiceError::new("Ruta API inválida."));
    }
    Ok(endpoint)
}
pub async fn read_bounded(
    response: reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, ServiceError> {
    if response.content_length().is_some_and(|n| n > max as u64) {
        return Err(ServiceError::new(
            "La respuesta supera el límite de memoria configurado.",
        ));
    }
    let mut stream = response.bytes_stream();
    let mut data = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ServiceError::network())?;
        if data.len().saturating_add(chunk.len()) > max {
            return Err(ServiceError::new(
                "La respuesta supera el límite de memoria configurado.",
            ));
        }
        data.extend_from_slice(&chunk)
    }
    Ok(data)
}
fn status_error(status: u16) -> ServiceError {
    ServiceError::status(
        match status {
            401 => "La sesión caducó. Inicia sesión de nuevo.",
            403 => "Discord no permite esta acción con los permisos actuales.",
            404 => "El contenido ya no está disponible o no tienes acceso.",
            400 => {
                "Discord rechazó la operación. Puede requerir datos adicionales o una verificación en el cliente oficial."
            }
            429 => {
                "Discord ha limitado las peticiones. La espera se conserva antes de permitir otra operación."
            }
            _ => "No se pudo completar la operación con Discord.",
        },
        status,
    )
}
fn is_verification(data: &Value) -> bool {
    data.as_object().is_some_and(|object| {
        [
            "captcha_key",
            "captcha_sitekey",
            "captcha_service",
            "captcha_rqdata",
            "captcha_rqtoken",
        ]
        .iter()
        .any(|key| object.contains_key(*key))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::Notify,
    };

    struct FixtureReply {
        status: u16,
        headers: Vec<(&'static str, &'static str)>,
        body: &'static str,
        release: Option<Arc<Notify>>,
        body_release: Option<Arc<Notify>>,
        content_length: Option<usize>,
        disconnect: bool,
    }
    impl FixtureReply {
        fn new(status: u16, body: &'static str) -> Self {
            Self {
                status,
                body,
                headers: Vec::new(),
                release: None,
                body_release: None,
                content_length: None,
                disconnect: false,
            }
        }
    }
    struct FixtureServer {
        base: Url,
        count: Arc<AtomicUsize>,
        seen: Arc<Notify>,
        audit_reasons: Arc<std::sync::Mutex<Vec<Option<String>>>>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for FixtureServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl FixtureServer {
        async fn new(replies: Vec<FixtureReply>) -> Self {
            // Unit tests own this loopback listener; no Discord connection or
            // credential file is consulted. Production has no endpoint override.
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = Url::parse(&format!(
                "http://{}/api/v9/",
                listener.local_addr().unwrap()
            ))
            .unwrap();
            let count = Arc::new(AtomicUsize::new(0));
            let seen = Arc::new(Notify::new());
            let server_count = count.clone();
            let server_seen = seen.clone();
            let audit_reasons = Arc::new(std::sync::Mutex::new(Vec::new()));
            let server_audit_reasons = audit_reasons.clone();
            let task = tokio::spawn(async move {
                for reply in replies {
                    let (mut stream, peer) = listener.accept().await.unwrap();
                    assert!(peer.ip().is_loopback());
                    let mut bytes = Vec::new();
                    let header_end = loop {
                        let mut buffer = [0u8; 2048];
                        let n = stream.read(&mut buffer).await.unwrap();
                        assert!(n > 0 && bytes.len() + n < 1024 * 1024);
                        bytes.extend_from_slice(&buffer[..n]);
                        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            break end + 4;
                        }
                    };
                    let headers =
                        String::from_utf8_lossy(&bytes[..header_end]).to_ascii_lowercase();
                    let audit = String::from_utf8_lossy(&bytes[..header_end]).lines().find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("X-Audit-Log-Reason").then(|| value.trim().to_owned())
                    });
                    server_audit_reasons.lock().unwrap().push(audit);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let chunked = headers.contains("transfer-encoding: chunked");
                    while bytes.len() < header_end + length
                        || chunked && !bytes[header_end..].ends_with(b"0\r\n\r\n")
                    {
                        let mut buffer = [0u8; 2048];
                        let n = stream.read(&mut buffer).await.unwrap();
                        assert!(n > 0 && bytes.len() + n < 1024 * 1024);
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    server_count.fetch_add(1, Ordering::SeqCst);
                    server_seen.notify_one();
                    if let Some(release) = reply.release {
                        release.notified().await;
                    }
                    if reply.disconnect {
                        continue;
                    }
                    let extra = reply
                        .headers
                        .iter()
                        .map(|(key, value)| format!("{key}: {value}\r\n"))
                        .collect::<String>();
                    let response = format!(
                        "HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n",
                        reply.status,
                        reply.content_length.unwrap_or(reply.body.len()),
                        extra
                    );
                    if stream.write_all(response.as_bytes()).await.is_err() {
                        continue;
                    }
                    if let Some(release) = reply.body_release {
                        release.notified().await;
                    }
                    // Closing the response early is expected when the safety
                    // gate cancels a request or rejects its status at headers.
                    let _ = stream.write_all(reply.body.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            });
            Self {
                base,
                count,
                seen,
                audit_reasons,
                task,
            }
        }
        fn api(&self) -> Api {
            let mut api =
                Api::new("synthetic-test-credential-only-not-a-user-token".into()).unwrap();
            api.test_endpoint = Some(self.base.clone());
            api
        }
        async fn wait_seen(&self, target: usize) {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let notified = self.seen.notified();
                    if self.count.load(Ordering::SeqCst) >= target {
                        break;
                    }
                    notified.await;
                }
            })
            .await
            .unwrap();
        }
    }
    #[test]
    fn routes_confined() {
        let base = Url::parse("https://discord.com/api/v9/").unwrap();
        for path in [
            "https://evil.com/",
            "//evil.com/x",
            "../x",
            "channels\\x",
            "channels/1\r\nX",
            "channels/%2e%2e/%2e%2e/%2e%2e/x",
        ] {
            assert!(validate_route(&base, path).is_err(), "{path}")
        }
        assert!(validate_route(&base, "channels/1/messages?content=hello%3Aworld").is_ok())
    }
    #[tokio::test]
    async fn audited_writes_encode_utf8_without_leaking_headers_or_bypassing_deduplication() {
        let server = FixtureServer::new(vec![FixtureReply::new(204, ""), FixtureReply::new(200, "{}")]).await;
        let api = server.api();
        let body = Some(json!({"nick":"Synthetic nickname"}));
        let long_reason = "x".repeat(513);
        assert!(api.send_audited(Method::PATCH,"guilds/123/members/456",body.clone(),&long_reason).await.is_err());
        assert_eq!(server.count.load(Ordering::SeqCst),0);
        api.send_audited(Method::PATCH,"guilds/123/members/456",body.clone(),"Moderación: spam / + café").await.unwrap();
        api.get("guilds/123/members/456").await.unwrap();
        assert_eq!(*server.audit_reasons.lock().unwrap(),vec![
            Some("Moderaci%C3%B3n%3A%20spam%20%2F%20%2B%20caf%C3%A9".into()), None,
        ]);
        assert_eq!(server.count.load(Ordering::SeqCst),2);
        let mut lost = FixtureReply::new(200,"{}");
        lost.disconnect=true;
        let uncertain = FixtureServer::new(vec![lost]).await;
        let api = uncertain.api();
        let error=api.send_audited(Method::PATCH,"guilds/123/members/456",body.clone(),"Original reason").await.unwrap_err();
        assert_eq!(error.kind,ErrorKind::OutcomeUnknown);
        let duplicate=api.send_audited(Method::PATCH,"guilds/123/members/456",body,"Changed reason").await.unwrap_err();
        assert_eq!(duplicate.kind,ErrorKind::OutcomeUnknown);
        assert_eq!(uncertain.count.load(Ordering::SeqCst),1,"Changing an audit reason cannot replay an unconfirmed write");
    }
    #[tokio::test]
    async fn audited_writes_respect_the_shared_safety_gate_without_a_network_request() {
        let server = FixtureServer::new(vec![]).await;
        let api = server.api();
        api.safety().block(SafetyState::VerificationRequired);
        let error = api.send_audited(Method::DELETE,"guilds/123/members/456",None,"Synthetic reason").await.unwrap_err();
        assert_eq!(error.kind,ErrorKind::VerificationRequired);
        assert_eq!(server.count.load(Ordering::SeqCst),0);
    }
    #[test]
    fn service_errors_do_not_include_private_response() {
        assert!(!status_error(401).message.contains("Authorization"));
        assert_eq!(status_error(404).status, Some(404))
    }
    #[tokio::test]
    async fn challenge_stops_an_already_queued_clone_and_future_clicks_without_replay() {
        let release = Arc::new(Notify::new());
        let mut reply = FixtureReply::new(
            400,
            r#"{"captcha_key":["synthetic-private-value"],"captcha_rqtoken":"never-log-this"}"#,
        );
        reply.release = Some(release.clone());
        let server = FixtureServer::new(vec![reply]).await;
        let api = server.api();
        let first_api = api.clone();
        let first = tokio::spawn(async move {
            first_api
                .request(
                    Method::POST,
                    "users/@me/relationships",
                    Some(json!({"username":"fixture"})),
                )
                .await
        });
        server.wait_seen(1).await;
        let queued_api = api.clone();
        let queued = tokio::spawn(async move {
            queued_api
                .request(
                    Method::POST,
                    "channels/123/messages",
                    Some(json!({"content":"fixture"})),
                )
                .await
        });
        tokio::task::yield_now().await;
        release.notify_one();
        let error = first.await.unwrap().unwrap_err();
        assert_eq!(error.kind, ErrorKind::VerificationRequired);
        assert!(!format!("{error:?}").contains("never-log-this"));
        assert!(!error.to_string().contains("synthetic-private-value"));
        assert_eq!(
            queued.await.unwrap().unwrap_err().kind,
            ErrorKind::VerificationRequired
        );
        assert_eq!(
            api.get("users/@me").await.unwrap_err().kind,
            ErrorKind::VerificationRequired
        );
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn upload_uses_the_same_challenge_classifier_and_never_retries() {
        let server =
            FixtureServer::new(vec![FixtureReply::new(403, r#"{"captcha_sitekey":null}"#)]).await;
        let api = server.api();
        let path = std::env::temp_dir().join(format!(
            "notdiscord-upload-p0-{}-{}.txt",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::write(&path, b"synthetic upload fixture").unwrap();
        let result = api
            .upload_many(
                "123",
                std::slice::from_ref(&path),
                "fixture",
                "",
                "123456789012345678",
            )
            .await;
        std::fs::remove_file(&path).unwrap();
        assert_eq!(result.unwrap_err().kind, ErrorKind::VerificationRequired);
        assert_eq!(api.safety().state(), SafetyState::VerificationRequired);
        assert_eq!(
            api.get("users/@me").await.unwrap_err().kind,
            ErrorKind::VerificationRequired
        );
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn ordinary_permission_denial_is_not_a_challenge_but_401_stops_the_session() {
        let server = FixtureServer::new(vec![
            FixtureReply::new(403, r#"{"message":"synthetic-private-permission-details"}"#),
            FixtureReply::new(401, "{}"),
        ])
        .await;
        let api = server.api();
        let error = api.get("channels/123/messages").await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::PermissionDenied);
        assert!(!error.to_string().contains("synthetic-private"));
        assert_eq!(api.safety().state(), SafetyState::Allowed);
        assert_eq!(
            api.get("users/@me").await.unwrap_err().kind,
            ErrorKind::AuthenticationRequired
        );
        assert_eq!(
            api.get("users/@me").await.unwrap_err().kind,
            ErrorKind::AuthenticationRequired
        );
        assert_eq!(server.count.load(Ordering::SeqCst), 2);
    }
    #[tokio::test]
    async fn long_global_429_is_kept_for_the_next_click_without_replaying_a_mutation() {
        let mut reply = FixtureReply::new(429, r#"{"retry_after":1,"global":true}"#);
        reply.headers.push(("Retry-After", "300.25"));
        let server = FixtureServer::new(vec![reply]).await;
        let api = server.api();
        let first = api
            .request(
                Method::POST,
                "channels/123/messages",
                Some(json!({"content":"fixture"})),
            )
            .await
            .unwrap_err();
        assert_eq!(first.kind, ErrorKind::RateLimited);
        assert!(first.retry_after.unwrap() > Duration::from_secs(299));
        let next = api.clone().get("users/@me").await.unwrap_err();
        assert_eq!(next.kind, ErrorKind::RateLimited);
        assert!(next.retry_after.unwrap() > Duration::from_secs(299));
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn safe_read_can_retry_a_short_definite_429_using_the_shared_deadline() {
        let mut reply = FixtureReply::new(429, "{}");
        reply.headers.push(("Retry-After", "0.025"));
        let server =
            FixtureServer::new(vec![reply, FixtureReply::new(200, r#"{"id":"123"}"#)]).await;
        let value = server.api().get("users/@me").await.unwrap();
        assert_eq!(value["id"], "123");
        assert_eq!(server.count.load(Ordering::SeqCst), 2);
    }
    #[tokio::test]
    async fn duplicate_mutation_is_rejected_while_pending_and_after_confirmation_without_second_send()
     {
        let release = Arc::new(Notify::new());
        let mut reply = FixtureReply::new(200, r#"{"id":"123"}"#);
        reply.release = Some(release.clone());
        let server = FixtureServer::new(vec![reply, FixtureReply::new(200, "{}")]).await;
        let api = server.api();
        let first_api = api.clone();
        let first = tokio::spawn(async move {
            first_api
                .request(
                    Method::POST,
                    "users/@me/relationships",
                    Some(json!({"username":"synthetic"})),
                )
                .await
        });
        server.wait_seen(1).await;
        let duplicate = tokio::time::timeout(
            Duration::from_secs(1),
            api.request(
                Method::POST,
                "users/@me/relationships",
                Some(json!({"username":"synthetic"})),
            ),
        )
        .await
        .unwrap();
        assert_eq!(duplicate.unwrap_err().kind, ErrorKind::OperationPending);
        release.notify_one();
        assert_eq!(first.await.unwrap().unwrap()["id"], "123");
        assert_eq!(
            api.request(
                Method::POST,
                "users/@me/relationships",
                Some(json!({"username":"synthetic"}))
            )
            .await
            .unwrap_err()
            .kind,
            ErrorKind::OperationCompleted
        );
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn ambiguous_upload_cannot_be_replayed_by_a_new_click_or_changed_file() {
        let mut lost = FixtureReply::new(200, "{}");
        lost.disconnect = true;
        let server =
            FixtureServer::new(vec![lost, FixtureReply::new(200, r#"{"id":"124"}"#)]).await;
        let api = server.api();
        let path = std::env::temp_dir().join(format!(
            "notdiscord-upload-dedup-{}-{}.txt",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::write(&path, b"synthetic original").unwrap();
        let first = api
            .upload_many("123", std::slice::from_ref(&path), "fixture", "", "555")
            .await;
        assert_eq!(first.unwrap_err().kind, ErrorKind::OutcomeUnknown);
        std::fs::write(&path, b"synthetic replacement").unwrap();
        let repeated = api
            .clone()
            .upload_many(
                "123",
                std::slice::from_ref(&path),
                "changed fixture",
                "",
                "555",
            )
            .await;
        assert_eq!(repeated.unwrap_err().kind, ErrorKind::OutcomeUnknown);
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
        // A deliberately new submission has its own nonce and is independent.
        assert_eq!(
            api.upload_many("123", std::slice::from_ref(&path), "new fixture", "", "556")
                .await
                .unwrap()["id"],
            "124"
        );
        assert_eq!(server.count.load(Ordering::SeqCst), 2);
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn lost_mutation_confirmation_is_unknown_and_has_no_automatic_retry() {
        let mut reply = FixtureReply::new(200, "{}");
        reply.disconnect = true;
        let server = FixtureServer::new(vec![reply, FixtureReply::new(200, "{}")]).await;
        let api = server.api();
        let error = api
            .request(
                Method::POST,
                "channels/123/messages",
                Some(json!({"content":"fixture"})),
            )
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::OutcomeUnknown);
        assert_eq!(
            api.clone()
                .request(
                    Method::POST,
                    "channels/123/messages",
                    Some(json!({"content":"fixture"}))
                )
                .await
                .unwrap_err()
                .kind,
            ErrorKind::OutcomeUnknown
        );
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn queued_requests_are_bounded_and_block_cancels_the_waiting_queue() {
        let release = Arc::new(Notify::new());
        let mut reply = FixtureReply::new(200, "{}");
        reply.release = Some(release.clone());
        let server = FixtureServer::new(vec![reply]).await;
        let api = server.api();
        let first_api = api.clone();
        let first = tokio::spawn(async move { first_api.get("users/@me").await });
        server.wait_seen(1).await;
        let mut queued = Vec::new();
        for _ in 0..31 {
            let api = api.clone();
            queued.push(tokio::spawn(async move { api.get("users/@me").await }));
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            while api.pending.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            api.get("users/@me").await.unwrap_err().kind,
            ErrorKind::QueueFull
        );
        api.safety().block(SafetyState::AuthenticationRequired);
        assert_eq!(
            first.await.unwrap().unwrap_err().kind,
            ErrorKind::AuthenticationRequired
        );
        for pending in queued {
            assert_eq!(
                pending.await.unwrap().unwrap_err().kind,
                ErrorKind::AuthenticationRequired
            );
        }
        release.notify_one();
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn authentication_rejection_stops_at_headers_without_waiting_for_a_body() {
        let mut reply = FixtureReply::new(401, "{}");
        reply.body_release = Some(Arc::new(Notify::new()));
        let server = FixtureServer::new(vec![reply]).await;
        let api = server.api();
        let error = tokio::time::timeout(Duration::from_secs(3), api.get("users/@me"))
            .await
            .expect("401 must not wait for the withheld body")
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::AuthenticationRequired);
        assert_eq!(error.status, Some(401));
        assert_eq!(api.safety().state(), SafetyState::AuthenticationRequired);
        assert!(api.get("users/@me").await.is_err());
        assert_eq!(server.count.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn shared_block_cancels_body_read_and_preserves_write_ambiguity() {
        for (method, expected) in [
            (Method::GET, ErrorKind::ProtocolUnsupported),
            (Method::POST, ErrorKind::OutcomeUnknown),
        ] {
            let mut reply = FixtureReply::new(200, "{}");
            reply.body_release = Some(Arc::new(Notify::new()));
            let server = FixtureServer::new(vec![reply]).await;
            let mut api = server.api();
            let body_started = Arc::new(Notify::new());
            api.body_started = Some(body_started.clone());
            let request_api = api.clone();
            let request = tokio::spawn(async move {
                request_api
                    .request(method, "channels/123/messages", Some(json!({})))
                    .await
            });
            tokio::time::timeout(Duration::from_secs(3), body_started.notified())
                .await
                .unwrap();
            api.safety().block(SafetyState::ProtocolUnsupported);
            let error = tokio::time::timeout(Duration::from_secs(3), request)
                .await
                .expect("blocked gate must cancel a stalled response body")
                .unwrap()
                .unwrap_err();
            assert_eq!(error.kind, expected);
            assert_eq!(api.pending.available_permits(), 32);
            assert_eq!(server.count.load(Ordering::SeqCst), 1);
        }
    }
    #[tokio::test]
    async fn unreadable_security_responses_pause_without_claiming_a_confirmed_captcha() {
        for (status, content_length) in [(400, None), (403, Some(512))] {
            let mut reply = FixtureReply::new(
                status,
                r#"{"captcha_key":"synthetic-private-security-body""#,
            );
            reply.content_length = content_length;
            let server = FixtureServer::new(vec![reply]).await;
            let api = server.api();
            let error = api.get("users/@me").await.unwrap_err();
            assert_eq!(error.kind, ErrorKind::VerificationRequired);
            assert_eq!(error.status, Some(status));
            assert!(error.message.contains("revisión de seguridad"));
            assert!(!error.message.to_lowercase().contains("captcha"));
            assert!(!format!("{error:?}").contains("synthetic-private"));
            assert_eq!(api.safety().state(), SafetyState::VerificationRequired);
            assert!(api.get("users/@me").await.is_err());
            assert_eq!(server.count.load(Ordering::SeqCst), 1);
        }
    }
    #[tokio::test]
    async fn server_error_and_truncated_success_never_confirm_or_replay_a_write() {
        for (status, content_length) in [(503, None), (200, Some(512))] {
            let mut reply = FixtureReply::new(status, "{}");
            reply.content_length = content_length;
            let server = FixtureServer::new(vec![reply]).await;
            let api = server.api();
            let error = api
                .request(
                    Method::POST,
                    "channels/123/messages",
                    Some(json!({"content":"fixture"})),
                )
                .await
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::OutcomeUnknown);
            assert_eq!(api.safety().state(), SafetyState::Allowed);
            assert_eq!(server.count.load(Ordering::SeqCst), 1);
        }
    }
}
