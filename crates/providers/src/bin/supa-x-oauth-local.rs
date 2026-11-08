//! Owner-terminal only. Default is help; nothing starts OAuth implicitly.
use std::io::{self, IsTerminal, Write};
use supa_providers::x_oauth::local::{CallbackListener, ConnectionApproval, LocalConnection};
use tokio::sync::watch;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!(
            "STOPPED: OAuth connection unconfirmed. No automatic retry. If consent occurred, review/revoke it on X before another attempt."
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
    if args.is_empty() {
        println!(
            "Local owner-only OAuth preparation. Use check-listener for a no-credential bind check. Live connect requires separate authorization: connect OWNER_UUID PUBLIC_CLIENT_ID --authorize-one-connection-and-one-budgeted-identity-read"
        );
        return Ok(());
    }
    if args.len() != 4
        || args[0] != "connect"
        || args[3] != "--authorize-one-connection-and-one-budgeted-identity-read"
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
    let approval = ConnectionApproval::owner_approved(
        Uuid::parse_str(&args[1])?,
        args[2].clone(),
        now + 600,
        None,
    )?;
    let connection = LocalConnection::prepare(approval).await?;
    let (sender, receiver) = watch::channel(false);
    let interrupt = tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = sender.send(true);
    });
    println!(
        "Open this URL privately in your browser and verify @supa_agency on X's consent screen:\n{}",
        connection.private_authorize_url()
    );
    let result = connection.finish_authorized(receiver).await;
    interrupt.abort();
    println!("{}", serde_json::to_string(&result?)?);
    Ok(())
}
