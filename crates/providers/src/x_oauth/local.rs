//! Explicit owner-terminal binding. Nothing is started or connected at module load.
//! No publishing endpoint, refresh worker, credential export or automatic retry.
use super::*;
#[path = "authorization.rs"]
pub mod authorization;
use authorization::AuthorizationReceipt;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
};

const MAX_BODY: usize = 16384;
fn unix_now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| Error::Denied)
}

/// Requires the existing owner's trusted local OS session, not a browser-supplied
/// owner ID. No keyring read occurs on construction. No plaintext-file fallback.
pub struct PrivateKeyring {
    owner: Uuid,
}
impl PrivateKeyring {
    pub fn new(owner: Uuid) -> Result<Self> {
        if owner.is_nil() {
            return Err(Error::Denied);
        }
        Ok(Self { owner })
    }
    fn call(&self, operation: &str, body: &impl Serialize) -> Result<Zeroizing<Vec<u8>>> {
        let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/x-oauth-keyring.py");
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = std::fs::symlink_metadata(&helper).map_err(|_| Error::KeyVault)?;
        if !metadata.is_file()
            || metadata.permissions().mode() & 0o022 != 0
            || metadata.uid()
                != std::fs::metadata("/proc/self")
                    .map_err(|_| Error::KeyVault)?
                    .uid()
        {
            return Err(Error::KeyVault);
        }
        let payload = Zeroizing::new(serde_json::to_vec(body).map_err(|_| Error::KeyVault)?);
        if payload.len() > 7000 {
            return Err(Error::KeyVault);
        }
        let mut command = Command::new("/usr/bin/python3");
        command
            .args(["-I", helper.to_str().ok_or(Error::KeyVault)?, operation])
            .env_clear();
        for name in [
            "HOME",
            "USER",
            "DBUS_SESSION_BUS_ADDRESS",
            "XDG_RUNTIME_DIR",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .env("LANG", "C.UTF-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| Error::KeyVault)?;
        let mut input = child.stdin.take().ok_or(Error::KeyVault)?;
        // Monitor the child while input is written, so a helper that never reads
        // stdin cannot evade the same deadline. Killing it closes the pipe.
        let writer = std::thread::spawn(move || input.write_all(&payload));
        let result = (|| {
            let began = Instant::now();
            loop {
                if let Some(status) = child.try_wait().map_err(|_| Error::KeyVault)? {
                    if !status.success() {
                        return Err(Error::KeyVault);
                    }
                    break;
                }
                if began.elapsed() > Duration::from_millis(5500) {
                    return Err(Error::KeyVault);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let mut output = Zeroizing::new(Vec::new());
            child
                .stdout
                .take()
                .ok_or(Error::KeyVault)?
                .take(7001)
                .read_to_end(&mut output)
                .map_err(|_| Error::KeyVault)?;
            if output.len() > 7000 {
                return Err(Error::KeyVault);
            }
            Ok(output)
        })();
        if result.is_err() {
            let _ = child.kill();
        }
        let _ = child.wait();
        if !matches!(writer.join(), Ok(Ok(()))) {
            return Err(Error::KeyVault);
        }
        result
    }
    /// Private server IPC only; approval is rechecked before reading any token.
    pub(crate) fn posting_connection(
        &self,
        auth: &crate::x_review::live::LiveAuthorization,
    ) -> Result<crate::x_review::live::Connection> {
        auth.check()?;
        if self.owner != auth.owner() {
            return Err(Error::Denied);
        }
        let raw = self.call(
            "backend-read-connection",
            &serde_json::json!({
                "owner":self.owner,"client_id":auth.client_id(),"account_id":auth.account().id
            }),
        )?;
        let connection: crate::x_review::live::Connection =
            serde_json::from_slice(&raw).map_err(|_| Error::KeyVault)?;
        connection.validate(auth)?;
        Ok(connection)
    }
    pub(crate) fn read_oauth1_env(&self) -> Result<Zeroizing<Vec<u8>>> {
        self.call(
            "backend-read-oauth1-env",
            &serde_json::json!({"owner":self.owner,"client_id":crate::x_review::oauth1::NAMESPACE}),
        )
    }
    #[cfg(test)]
    pub(crate) fn read_oauth1(&self) -> Result<Zeroizing<Vec<u8>>> {
        self.call(
            "backend-read-oauth1",
            &serde_json::json!({"owner":self.owner,"client_id":crate::x_review::oauth1::NAMESPACE}),
        )
    }
    fn retain(
        &self,
        client_id: &str,
        token: &TokenBody,
        identity: &IdentityData,
        expires: u64,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct Connection<'a> {
            client_id: &'a str,
            owner: Uuid,
            account_id: &'a str,
            username: &'a str,
            scope: &'static [&'static str],
            expires_unix: u64,
            access_token: &'a str,
        }
        let raw = self.call(
            "backend-store-connection",
            &Connection {
                client_id,
                owner: self.owner,
                account_id: &identity.id,
                username: &identity.username,
                scope: SCOPES,
                expires_unix: expires,
                access_token: &token.access_token,
            },
        )?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Ack {
            stored: bool,
        }
        if !serde_json::from_slice::<Ack>(&raw)
            .map_err(|_| Error::KeyVault)?
            .stored
        {
            return Err(Error::KeyVault);
        }
        Ok(())
    }
}
impl ConfidentialCredentialStore for PrivateKeyring {
    fn client_secret(&self, label: &str, client_id: &str) -> Result<Zeroizing<String>> {
        if label != CREDENTIAL_CUSTODY {
            return Err(Error::KeyVault);
        }
        #[derive(Serialize)]
        struct Query<'a> {
            client_id: &'a str,
            owner: Uuid,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Configuration {
            client_id: String,
            owner: Uuid,
            callback: String,
            #[serde(deserialize_with = "secret_string")]
            client_secret: Zeroizing<String>,
        }
        let raw = self.call(
            "backend-read",
            &Query {
                client_id,
                owner: self.owner,
            },
        )?;
        let config: Configuration = serde_json::from_slice(&raw).map_err(|_| Error::KeyVault)?;
        if config.client_id != client_id
            || config.owner != self.owner
            || config.callback != REGISTERED_CALLBACK
        {
            return Err(Error::KeyVault);
        }
        Ok(config.client_secret)
    }
}

/// Immutable one-connection permission from the local owner. Not an X posting
/// grant. The caller must separately verify/authorize the identity-read tariff.
pub struct ConnectionApproval {
    owner: Uuid,
    client_id: String,
    expires: u64,
    expected_id: Option<String>,
    receipt: Option<AuthorizationReceipt>,
}
impl ConnectionApproval {
    /// The terminal assertion alone never permits a live connection.
    pub fn with_receipt(mut self, receipt: AuthorizationReceipt) -> Result<Self> {
        if !receipt.matches(self.owner, &self.client_id) {
            return Err(Error::Denied);
        }
        receipt.check()?;
        self.expires = self.expires.min(receipt.expires());
        self.receipt = Some(receipt);
        Ok(self)
    }
    fn authorize(&self) -> Result<()> {
        if unix_now()? >= self.expires {
            return Err(Error::Denied);
        }
        self.receipt.as_ref().ok_or(Error::Denied)?.check()
    }

    pub fn owner_approved(
        owner: Uuid,
        client_id: String,
        expires: u64,
        expected_id: Option<String>,
    ) -> Result<Self> {
        let now = unix_now()?;
        OAuthPreparation::public_client(client_id.clone(), REGISTERED_CALLBACK)?;
        if owner.is_nil()
            || expires <= now
            || expires > now.checked_add(600).ok_or(Error::Denied)?
            || expected_id.as_ref().is_some_and(|s| !numeric_id(s))
        {
            return Err(Error::Denied);
        }
        Ok(Self {
            owner,
            client_id,
            expires,
            expected_id,
            receipt: None,
        })
    }
}
fn numeric_id(id: &str) -> bool {
    id.parse::<u64>()
        .is_ok_and(|n| n > 0 && n.to_string() == id)
}

/// Created before loading secrets or presenting a consent URL. Drop closes it.
pub struct CallbackListener {
    listener: TcpListener,
}
impl CallbackListener {
    pub async fn bind() -> Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 37641))
            .await
            .map_err(|_| Error::Busy)?;
        if listener
            .local_addr()
            .map_err(|_| Error::Denied)?
            .to_string()
            != "127.0.0.1:37641"
        {
            return Err(Error::Denied);
        }
        Ok(Self { listener })
    }
    pub fn address(&self) -> Result<std::net::SocketAddr> {
        self.listener.local_addr().map_err(|_| Error::Denied)
    }
    async fn receive(&self) -> Result<Zeroizing<String>> {
        let (mut socket, peer) = self.listener.accept().await.map_err(|_| Error::Denied)?;
        if peer.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
            return Err(Error::Denied);
        }
        let request = tokio::time::timeout(Duration::from_secs(2), read_callback(&mut socket))
            .await
            .map_err(|_| Error::Timeout)?;
        let status = if request.is_ok() {
            "200 OK"
        } else {
            "400 Bad Request"
        };
        // Static no-store completion page: never echo the query/code or account.
        let script = "history.replaceState(null,'','/oauth/x/callback');";
        let hash = base64::engine::general_purpose::STANDARD
            .encode(digest::digest(&digest::SHA256, script.as_bytes()));
        let body = format!(
            "<!doctype html><meta charset=utf-8><title>Supa Agency</title><p>Return to your private terminal. Connection verification is not yet complete.</p><script>{script}</script>"
        );
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'none'; script-src 'sha256-{hash}'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let _ = tokio::time::timeout(
            Duration::from_secs(1),
            socket.write_all(response.as_bytes()),
        )
        .await;
        request
    }
}
async fn read_callback(socket: &mut TcpStream) -> Result<Zeroizing<String>> {
    let mut raw = Zeroizing::new(Vec::new());
    loop {
        let mut chunk = [0u8; 1024];
        let n = socket.read(&mut chunk).await.map_err(|_| Error::Denied)?;
        if n == 0 {
            return Err(Error::Denied);
        }
        raw.extend_from_slice(&chunk[..n]);
        chunk.zeroize();
        if raw.len() > 8192 {
            return Err(Error::Denied);
        }
        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let text = std::str::from_utf8(&raw).map_err(|_| Error::Denied)?;
    let (head, body) = text.split_once("\r\n\r\n").ok_or(Error::Denied)?;
    if !body.is_empty() {
        return Err(Error::Denied);
    }
    let mut lines = head.split("\r\n");
    let request: Vec<_> = lines.next().ok_or(Error::Denied)?.split(' ').collect();
    if request.len() != 3
        || request[0] != "GET"
        || request[2] != "HTTP/1.1"
        || !request[1].starts_with("/oauth/x/callback?")
        || request[1].len() > 4096
        || !request[1].bytes().all(|c| c.is_ascii_graphic())
    {
        return Err(Error::Denied);
    }
    let mut hosts = 0;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(Error::Denied)?;
        if name.eq_ignore_ascii_case("host") {
            hosts += 1;
            if value.trim() != "127.0.0.1:37641" {
                return Err(Error::Denied);
            }
        }
        if name.eq_ignore_ascii_case("origin")
            || name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value.trim() != "0")
        {
            return Err(Error::Denied);
        }
    }
    if hosts != 1 {
        return Err(Error::Denied);
    }
    Ok(Zeroizing::new(format!(
        "http://127.0.0.1:37641{}",
        request[1]
    )))
}

/// TLS-only fixed-endpoint transport. No constructor starts a request. No proxy,
/// automatic redirect/retry, debug output, raw response report or posting method.
pub struct HttpsTransport {
    client: reqwest::Client,
    token_url: Url,
    identity_url: Url,
}
impl HttpsTransport {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Self::client_builder()
                .build()
                .map_err(|_| Error::ProviderTransport)?,
            token_url: Url::parse(TOKEN).map_err(|_| Error::Invalid)?,
            identity_url: Url::parse(IDENTITY_ENDPOINT).map_err(|_| Error::Invalid)?,
        })
    }
    fn client_builder() -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .https_only(true)
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
    }
    async fn bytes(&self, request: reqwest::RequestBuilder) -> Result<Zeroizing<Vec<u8>>> {
        let mut response = request.send().await.map_err(|_| Error::ProviderTransport)?;
        if response.status().as_u16() != 200 {
            return Err(Error::ProviderHttp(response.status().as_u16()));
        }
        if !response
            .headers()
            .get("content-type")
            .and_then(|s| s.to_str().ok())
            .is_some_and(|s| s.split(';').next() == Some("application/json"))
            || response
                .content_length()
                .is_some_and(|n| n > MAX_BODY as u64)
        {
            return Err(Error::Provider);
        }
        let mut body = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::ProviderTransport)?
        {
            if body.len() + chunk.len() > MAX_BODY {
                return Err(Error::Provider);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
    async fn connect(
        &self,
        exchange: TokenExchange,
        cancel: &watch::Receiver<bool>,
        receipt: Option<&AuthorizationReceipt>,
    ) -> Result<(TokenBody, IdentityData)> {
        let began = Instant::now();
        if *cancel.borrow() || cancel.has_changed().is_err() {
            return Err(Error::Denied);
        }
        authorize_transport(receipt)?;
        if *cancel.borrow()
            || cancel.has_changed().is_err()
            || began.elapsed() >= Duration::from_secs(6)
        {
            return Err(Error::Denied);
        }
        let mut auth = reqwest::header::HeaderValue::from_str(
            exchange.authorization.as_deref().ok_or(Error::Denied)?,
        )
        .map_err(|_| Error::Denied)?;
        auth.set_sensitive(true);
        let response = self
            .bytes(
                self.client
                    .post(self.token_url.clone())
                    .header(reqwest::header::AUTHORIZATION, auth)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(exchange.form.to_string()),
            )
            .await?;
        let token: TokenBody = serde_json::from_slice(&response).map_err(|_| Error::Provider)?;
        if !valid_token_metadata(&token) || token.access_token.starts_with("synthetic-") {
            return Err(Error::Denied);
        }
        if *cancel.borrow()
            || cancel.has_changed().is_err()
            || began.elapsed() >= Duration::from_secs(token.expires_in)
        {
            return Err(Error::Denied);
        }
        authorize_transport(receipt)?;
        if *cancel.borrow()
            || cancel.has_changed().is_err()
            || began.elapsed() >= Duration::from_secs(token.expires_in)
            || began.elapsed() >= Duration::from_secs(6)
        {
            return Err(Error::Denied);
        }
        let mut auth =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", &*token.access_token))
                .map_err(|_| Error::Denied)?;
        auth.set_sensitive(true);
        let response = self
            .bytes(
                self.client
                    .get(self.identity_url.clone())
                    .header(reqwest::header::AUTHORIZATION, auth),
            )
            .await?;
        let identity: IdentityBody =
            serde_json::from_slice(&response).map_err(|_| Error::Provider)?;
        if identity.data.username != "supa_agency"
            || !numeric_id(&identity.data.id)
            || began.elapsed() >= Duration::from_secs(token.expires_in)
        {
            return Err(Error::Denied);
        }
        Ok((token, identity.data))
    }
}

fn authorize_transport(receipt: Option<&AuthorizationReceipt>) -> Result<()> {
    match receipt {
        Some(receipt) => receipt.check(),
        // Synthetic TLS fixture tests never possess authority or call real X.
        None if cfg!(test) => Ok(()),
        None => Err(Error::Denied),
    }
}

#[derive(Serialize)]
pub struct ConnectionReceipt {
    pub owner_id: Uuid,
    pub client_id: String,
    pub account_id: String,
    pub username: String,
    pub expires_unix: u64,
    pub scope: Vec<String>,
    pub publishing_enabled: bool,
}
/// No real call occurs until this consuming method is invoked under a separately
/// authorized live consent + identity-read budget. Owner terminal only. A failed
/// attempt cannot reuse its pending state; caller must never automatically retry.
pub struct LocalConnection {
    listener: CallbackListener,
    pending: PendingConnection,
    approval: ConnectionApproval,
    keyring: PrivateKeyring,
}
impl LocalConnection {
    pub async fn prepare(approval: ConnectionApproval) -> Result<Self> {
        approval.authorize()?;
        let listener = CallbackListener::bind().await?;
        let keyring = PrivateKeyring::new(approval.owner)?;
        let config = OAuthPreparation::confidential_from_custody(
            approval.client_id.clone(),
            REGISTERED_CALLBACK,
            &keyring,
        )?;
        if unix_now()? >= approval.expires {
            return Err(Error::Denied);
        }
        approval.authorize()?;
        let pending = config.begin(approval.owner, unix_now()?)?;
        Ok(Self {
            listener,
            pending,
            approval,
            keyring,
        })
    }
    pub fn private_authorize_url(&self) -> Result<&str> {
        self.approval.authorize()?;
        Ok(self.pending.authorize_url())
    }
    pub async fn finish_authorized(
        mut self,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<ConnectionReceipt> {
        if *cancel.borrow() || unix_now()? >= self.approval.expires {
            self.pending.cancel();
            return Err(Error::Denied);
        }
        self.approval.authorize()?;
        let remaining = Duration::from_secs(self.approval.expires.saturating_sub(unix_now()?));
        let current_cancel = cancel.clone();
        let result = tokio::select! {
            _=cancel.changed()=>Err(Error::Denied),
            value=tokio::time::timeout(remaining,async {
                let callback=self.listener.receive().await?;
                let exchange=self.pending.consume(self.approval.owner,&callback,unix_now()?)?;
                let began=Instant::now();let now=unix_now()?;
                let transport=HttpsTransport::new()?;
                let (token,identity)=tokio::time::timeout(Duration::from_secs(6),transport.connect(exchange, &current_cancel, self.approval.receipt.as_ref())).await.map_err(|_|Error::Timeout)??;
                if unix_now()?>=self.approval.expires || began.elapsed()>=Duration::from_secs(token.expires_in)
                    || self.approval.expected_id.as_ref().is_some_and(|id|id!=&identity.id){return Err(Error::Denied);}
                let expires=now.checked_add(token.expires_in).ok_or(Error::Denied)?;
                // No token reaches the browser/report. A storage failure is a consumed
                // attempt, not permission to repeat the exchange or identity read.
                if *current_cancel.borrow() { return Err(Error::Denied); }
                self.approval.authorize()?;
                self.keyring.retain(&self.approval.client_id,&token,&identity,expires)?;
                if *current_cancel.borrow() || unix_now()? >= expires || unix_now()? >= self.approval.expires { return Err(Error::Denied); }
                Ok(ConnectionReceipt{owner_id:self.approval.owner,client_id:self.approval.client_id.clone(),account_id:identity.id,username:identity.username,expires_unix:expires,scope:SCOPES.iter().map(|s|s.to_string()).collect(),publishing_enabled:false})
            })=>value.unwrap_or(Err(Error::Timeout)),
        };
        self.pending.cancel();
        result
    }
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
