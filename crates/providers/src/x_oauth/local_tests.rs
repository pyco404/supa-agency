//! Actual loopback sockets/TLS; never reads real custody or contacts X.
use super::*;
use std::io::BufRead;
static CALLBACK_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn pending() -> PendingConnection {
    OAuthPreparation::confidential_client(
        "synthetic_confidential_client".into(),
        REGISTERED_CALLBACK,
        Zeroizing::new("fixture-secret-for-local-test".into()),
    )
    .unwrap()
    .begin(Uuid::from_u128(1), 100)
    .unwrap()
}
fn callback(p: &PendingConnection) -> String {
    let state = p
        .authorize
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    format!("{REGISTERED_CALLBACK}?code=synthetic-oauth-code-fixture&state={state}")
}
async fn send(raw: String) -> String {
    let mut stream = TcpStream::connect("127.0.0.1:37641").await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8(response).unwrap()
}
#[tokio::test]
async fn callback_fixed_bind_exclusivity_sanitized_page_and_exact_state_consumption() {
    let _guard = CALLBACK_LOCK.lock().await;
    let listener = CallbackListener::bind().await.unwrap();
    assert_eq!(listener.address().unwrap().to_string(), "127.0.0.1:37641");
    assert!(CallbackListener::bind().await.is_err());
    let approval = ConnectionApproval::owner_approved(
        Uuid::from_u128(1),
        "synthetic_confidential_client".into(),
        unix_now().unwrap() + 60,
        None,
    )
    .unwrap();
    assert!(matches!(
        LocalConnection::prepare(approval).await,
        Err(Error::Busy)
    ));
    let mut p = pending();
    let cb = callback(&p);
    let path = cb.strip_prefix("http://127.0.0.1:37641").unwrap();
    let client = tokio::spawn(send(format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:37641\r\n\r\n"
    )));
    let received = listener.receive().await.unwrap();
    assert!(*received == cb);
    let response = client.await.unwrap();
    for required in [
        "200 OK",
        "Cache-Control: no-store",
        "default-src 'none'",
        "Referrer-Policy: no-referrer",
        "history.replaceState",
    ] {
        assert!(response.contains(required));
    }
    assert!(!response.contains("synthetic-oauth-code-fixture"));
    assert!(!response.contains("&state="));
    let request = p.consume(Uuid::from_u128(1), &received, 101).unwrap();
    assert!(request.authorization.unwrap().starts_with("Basic "));
    assert!(p.consume(Uuid::from_u128(1), &received, 102).is_err());
    for request in [
        "POST /oauth/x/callback?code=a&state=b HTTP/1.1\r\nHost: 127.0.0.1:37641\r\n\r\n",
        "GET /oauth/x/callback?code=a&state=b HTTP/1.1\r\nHost: attacker.invalid\r\n\r\n",
        "GET /oauth/x/callback?code=a&state=b HTTP/1.1\r\nHost: 127.0.0.1:37641\r\nHost: 127.0.0.1:37641\r\n\r\n",
        "GET /oauth/x/callback?code=a&state=b HTTP/1.1\r\nHost: 127.0.0.1:37641\r\nOrigin: https://attacker.invalid\r\n\r\n",
        "GET /elsewhere HTTP/1.1\r\nHost: 127.0.0.1:37641\r\n\r\n",
        "GET http://127.0.0.1:37641/oauth/x/callback?code=a&state=b HTTP/1.1\r\nHost: 127.0.0.1:37641\r\n\r\n",
        "GET /oauth/x/callback?code=a&state=b HTTP/1.1\r\nHost: 127.0.0.1:37641\r\nTransfer-Encoding: chunked\r\n\r\n",
        "GET /oauth/x/callback?code=a&state=b HTTP/1.1\r\nHost: 127.0.0.1:37641\r\nContent-Length: 2\r\n\r\nxz",
    ] {
        let client = tokio::spawn(send(request.into()));
        assert!(listener.receive().await.is_err());
        assert!(client.await.unwrap().contains("400 Bad Request"));
    }
    let client = tokio::spawn(async {
        let mut socket = TcpStream::connect("127.0.0.1:37641").await.unwrap();
        socket.write_all(b"GET /oauth/x/callback?").await.unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
    });
    assert!(matches!(listener.receive().await, Err(Error::Timeout)));
    client.abort();
}
#[tokio::test]
async fn cancelled_or_expired_local_connection_has_no_credential_or_network_operations() {
    let _guard = CALLBACK_LOCK.lock().await;
    for expired in [false, true] {
        let approval = ConnectionApproval {
            owner: Uuid::from_u128(1),
            client_id: "synthetic_confidential_client".into(),
            expires: if expired { 1 } else { unix_now().unwrap() + 60 },
            expected_id: None,
        };
        let connection = LocalConnection {
            listener: CallbackListener::bind().await.unwrap(),
            pending: pending(),
            approval,
            keyring: PrivateKeyring::new(Uuid::from_u128(1)).unwrap(),
        };
        let (_sender, receiver) = watch::channel(!expired);
        assert!(connection.finish_authorized(receiver).await.is_err());
    }
    for (owner, client, expires, id) in [
        (Uuid::nil(), "client", unix_now().unwrap() + 60, None),
        (Uuid::from_u128(1), "client", 1, None),
        (
            Uuid::from_u128(1),
            "client",
            unix_now().unwrap() + 601,
            None,
        ),
        (Uuid::from_u128(1), "", unix_now().unwrap() + 60, None),
        (
            Uuid::from_u128(1),
            "client",
            unix_now().unwrap() + 60,
            Some("0123".into()),
        ),
    ] {
        assert!(ConnectionApproval::owner_approved(owner, client.into(), expires, id).is_err());
    }
}
struct TlsFixture {
    child: std::process::Child,
    root: tempfile::TempDir,
    port: u16,
}
impl TlsFixture {
    fn new(mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let cert = root.path().join("certificate.pem");
        let key = root.path().join("key.pem");
        let ca = root.path().join("ca.pem");
        let ca_key = root.path().join("ca-key.pem");
        let csr = root.path().join("server.csr");
        let extensions = root.path().join("server.ext");
        std::fs::write(&extensions,"subjectAltName=IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n").unwrap();
        let mut generate_ca = Command::new("/usr/bin/openssl");
        generate_ca
            .args([
                "req",
                "-x509",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:P-256",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=Synthetic test CA",
                "-addext",
                "basicConstraints=critical,CA:TRUE",
                "-keyout",
            ])
            .arg(&ca_key)
            .arg("-out")
            .arg(&ca);
        let mut generate_server = Command::new("/usr/bin/openssl");
        generate_server
            .args([
                "req",
                "-new",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:P-256",
                "-nodes",
                "-subj",
                "/CN=localhost",
                "-keyout",
            ])
            .arg(&key)
            .arg("-out")
            .arg(&csr);
        let mut sign = Command::new("/usr/bin/openssl");
        sign.args(["x509", "-req", "-days", "1", "-in"])
            .arg(&csr)
            .arg("-CA")
            .arg(&ca)
            .arg("-CAkey")
            .arg(&ca_key)
            .arg("-CAcreateserial")
            .arg("-extfile")
            .arg(&extensions)
            .arg("-out")
            .arg(&cert);
        for command in [&mut generate_ca, &mut generate_server, &mut sign] {
            assert!(
                command
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/fixtures/x-oauth-tls.py");
        let mut child = Command::new("/usr/bin/python3")
            .arg("-I")
            .arg(script)
            .arg(mode)
            .arg(&cert)
            .arg(&key)
            .arg(root.path().join("stats.json"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = serde_json::from_str::<serde_json::Value>(&line).unwrap()["port"]
            .as_u64()
            .unwrap() as u16;
        Self { child, root, port }
    }
    fn transport(&self, trusted: bool) -> HttpsTransport {
        let mut builder = HttpsTransport::client_builder();
        if trusted {
            builder = builder.add_root_certificate(
                reqwest::Certificate::from_pem(
                    &std::fs::read(self.root.path().join("ca.pem")).unwrap(),
                )
                .unwrap(),
            );
        }
        HttpsTransport {
            client: builder.build().unwrap(),
            token_url: Url::parse(&format!("https://127.0.0.1:{}/2/oauth2/token", self.port))
                .unwrap(),
            identity_url: Url::parse(&format!("https://127.0.0.1:{}/2/users/me", self.port))
                .unwrap(),
        }
    }
    fn calls(&self) -> u64 {
        std::fs::read(self.root.path().join("stats.json"))
            .ok()
            .map(|b| {
                let s: serde_json::Value = serde_json::from_slice(&b).unwrap();
                assert_eq!(s["posting_calls"], 0);
                s["calls"].as_u64().unwrap()
            })
            .unwrap_or(0)
    }
}
impl Drop for TlsFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn exchange() -> TokenExchange {
    let mut p = pending();
    let cb = callback(&p);
    p.consume(Uuid::from_u128(1), &cb, 101).unwrap()
}
#[tokio::test]
async fn https_wire_basic_pkce_bearer_and_read_only_identity_are_verified() {
    let fixture = TlsFixture::new("success");
    let transport = fixture.transport(true);
    let (_sender, receiver) = watch::channel(false);
    let (token, identity) = transport.connect(exchange(), &receiver).await.unwrap();
    assert!(*token.access_token == "fixture-access-token-only");
    assert_eq!(identity.username, "supa_agency");
    assert_eq!(identity.id, "123");
    assert_eq!(fixture.calls(), 2);
    let production = HttpsTransport::new().unwrap();
    assert_eq!(production.token_url.as_str(), TOKEN);
    assert_eq!(production.identity_url.as_str(), IDENTITY_ENDPOINT);
}
#[tokio::test]
async fn tls_certificates_hostname_versions_plaintext_and_cancellation_fail_closed() {
    for mode in [
        "untrusted",
        "hostname",
        "plaintext",
        "legacy_tls",
        "cancelled",
    ] {
        let fixture = TlsFixture::new(if mode == "legacy_tls" {
            mode
        } else {
            "success"
        });
        let mut transport = fixture.transport(mode != "untrusted");
        if mode == "hostname" {
            transport.token_url.set_host(Some("localhost")).unwrap();
        }
        if mode == "plaintext" {
            transport.token_url.set_scheme("http").unwrap();
        }
        let (_sender, receiver) = watch::channel(mode == "cancelled");
        assert!(
            transport.connect(exchange(), &receiver).await.is_err(),
            "{mode}"
        );
        assert_eq!(fixture.calls(), 0);
    }
}
#[tokio::test]
async fn https_status_redirects_scopes_partial_errors_limits_and_ambiguous_transport_never_retry() {
    for (mode, calls) in [
        ("redirect", 1),
        ("unauthorized", 1),
        ("rate_limit", 1),
        ("scope", 1),
        ("refresh", 1),
        ("oversize", 1),
        ("malformed", 1),
        ("disconnect", 1),
        ("slow", 1),
        ("account", 2),
        ("partial", 2),
    ] {
        let fixture = TlsFixture::new(mode);
        let transport = fixture.transport(true);
        let (_sender, receiver) = watch::channel(false);
        assert!(
            transport.connect(exchange(), &receiver).await.is_err(),
            "{mode}"
        );
        assert_eq!(fixture.calls(), calls, "{mode}");
    }
}

#[test]
#[ignore = "requires the disposable DBus/keyring created by test-x-oauth-keyring.py; never native custody"]
fn private_keyring_bridge_reads_and_retains_only_in_disposable_session() {
    assert_eq!(
        std::env::var("SUPA_X_KEYRING_ISOLATED_TEST").as_deref(),
        Ok("1")
    );
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    assert!(
        home.parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("supa-x-keyring-")
    );
    assert_eq!(home.file_name().unwrap(), "home");
    assert!(
        std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .unwrap()
            .starts_with("unix:")
    );
    let keyring = PrivateKeyring::new(Uuid::from_u128(1)).unwrap();
    let secret = keyring
        .client_secret(CREDENTIAL_CUSTODY, "synthetic_confidential_client")
        .unwrap();
    assert!(*secret == "synthetic-client-secret-fixture-only");
    assert!(
        keyring
            .client_secret(CREDENTIAL_CUSTODY, "another_client")
            .is_err()
    );
    assert!(
        PrivateKeyring::new(Uuid::from_u128(2))
            .unwrap()
            .client_secret(CREDENTIAL_CUSTODY, "synthetic_confidential_client")
            .is_err()
    );
    let token = TokenBody {
        access_token: Zeroizing::new("fixture-access-token-only".into()),
        token_type: "bearer".into(),
        scope: SCOPES.join(" "),
        expires_in: 600,
    };
    let identity = IdentityData {
        id: "123".into(),
        username: "supa_agency".into(),
    };
    keyring
        .retain(
            "synthetic_confidential_client",
            &token,
            &identity,
            unix_now().unwrap() + 600,
        )
        .unwrap();
    assert!(
        keyring
            .retain(
                "synthetic_confidential_client",
                &token,
                &identity,
                unix_now().unwrap() + 600
            )
            .is_err()
    );
}
