//! Owner-terminal only. Default is help; nothing starts OAuth implicitly.
use std::io::{self, IsTerminal, Write};
use supa_providers::x_oauth::local::authorization::AuthorizationReceipt;
use supa_providers::x_oauth::local::{CallbackListener, ConnectionApproval, LocalConnection};
use tokio::sync::watch;

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!(
            "STOPPED: X operation unconfirmed. No automatic retry. If consent occurred, review/revoke it on X before another attempt."
        );
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["check-listener"] {
        let listener = CallbackListener::bind().await?;
        println!(
            "{}",
            serde_json::json!({"callback_listener":listener.address()?.to_string(),"loopback_only":true,"consent_started":false,"credential_reads":0,"external_requests":0})
        );
        return Ok(());
    }
    if args == ["status-oauth1"] {
        let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/x-oauth-keyring.py");
        if !std::process::Command::new("/usr/bin/python3")
            .arg("-I")
            .arg(helper)
            .arg("status-oauth1")
            .status()?
            .success()
        {
            return Err("denied".into());
        }
        return Ok(());
    }
    if args.is_empty() {
        println!(
            "Local X only: setup-oauth1; status-oauth1; verify-oauth1 --authorize-one-budgeted-identity-read; setup-local PUBLIC_CLIENT_ID; connect-local PUBLIC_CLIENT_ID --authorize-one-connection-and-one-budgeted-identity-read. No platform role or publishing is granted."
        );
        return Ok(());
    }
    let oauth1_setup = args == ["setup-oauth1"];
    let oauth1_verify = args == ["verify-oauth1", "--authorize-one-budgeted-identity-read"];
    let setup = args.len() == 2 && args[0] == "setup-local";
    let connect = args.len() == 3
        && args[0] == "connect-local"
        && args[2] == "--authorize-one-connection-and-one-budgeted-identity-read";
    if (!setup && !connect && !oauth1_setup && !oauth1_verify)
        || !io::stdin().is_terminal()
        || !io::stdout().is_terminal()
        || !io::stderr().is_terminal()
    {
        return Err("denied".into());
    }
    let limits = std::fs::read_to_string("/proc/self/limits")?;
    let core_disabled = limits
        .lines()
        .find_map(|line| line.strip_prefix("Max core file size"))
        .is_some_and(|line| line.split_whitespace().next() == Some("0"));
    if !core_disabled {
        eprintln!("Disable core dumps in this private shell first: ulimit -c 0");
        return Err("denied".into());
    }
    if oauth1_setup {
        let session =
            AuthorizationReceipt::local_owner_session(supa_providers::x_review::oauth1::NAMESPACE)?;
        session.check()?;
        let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/x-oauth-keyring.py");
        if !std::process::Command::new("/usr/bin/python3")
            .arg("-I")
            .arg(helper)
            .arg("setup-oauth1")
            .status()?
            .success()
        {
            return Err("denied".into());
        }
        return Ok(());
    }
    if oauth1_verify {
        eprintln!(
            "Requires separate approval for one GET /2/users/me, budgeted at $0.01, and verified pricing/credits. No posting or retry."
        );
        eprint!("Type VERIFY @supa_agency ONCE: ");
        io::stderr().flush()?;
        let mut confirmation = String::new();
        io::stdin().read_line(&mut confirmation)?;
        if confirmation.trim() != "VERIFY @supa_agency ONCE" {
            return Err("denied".into());
        }
        let session =
            AuthorizationReceipt::local_owner_session(supa_providers::x_review::oauth1::NAMESPACE)?;
        let receipt = supa_providers::x_review::oauth1::verify_official_account(session).await?;
        println!("{}", serde_json::to_string(&receipt)?);
        return Ok(());
    }
    if setup {
        // Authenticated OS session + unlocked keyring, without reading a secret.
        let session = AuthorizationReceipt::local_owner_session(&args[1])?;
        session.check()?;
        let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/x-oauth-keyring.py");
        let status = std::process::Command::new("/usr/bin/python3")
            .arg("-I")
            .arg(helper)
            .arg("setup-local")
            .arg(&args[1])
            .status()?;
        if !status.success() {
            return Err("denied".into());
        }
        return Ok(());
    }
    eprintln!(
        "Requires separate owner approval and verified X identity-lookup pricing/credits. One exchange and one GET only; no posting, refresh or retry."
    );
    eprint!("Type CONNECT @supa_agency ONCE to confirm the separately approved operation: ");
    io::stderr().flush()?;
    let mut confirmation = String::new();
    io::stdin().read_line(&mut confirmation)?;
    if confirmation.trim() != "CONNECT @supa_agency ONCE" {
        return Err("denied".into());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    // Local OS authentication is scoped only to this one X connection.
    let receipt = AuthorizationReceipt::local_owner_session(&args[1])?;
    let owner = "6fd863e1-745d-4a06-b68e-eea69e854a69".parse()?;
    let approval = ConnectionApproval::owner_approved(owner, args[1].clone(), now + 600, None)?
        .with_receipt(receipt)?;
    let connection = LocalConnection::prepare(approval).await?;
    let (sender, receiver) = watch::channel(false);
    let interrupt = tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = sender.send(true);
    });
    println!(
        "Open this URL privately in your browser and verify @supa_agency on X's consent screen:\n{}",
        connection.private_authorize_url()?
    );
    let result = connection.finish_authorized(receiver).await;
    interrupt.abort();
    println!("{}", serde_json::to_string(&result?)?);
    Ok(())
}
