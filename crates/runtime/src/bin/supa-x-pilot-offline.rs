//! NON-DEPLOYABLE offline harness. Same process code; compile-time fake transport.
#[path = "../x_pilot_process.rs"]
mod process;
#[tokio::main]
async fn main() {
    if process::run().await.is_err() {
        eprintln!("OFFLINE VALIDATION: denied or uncertain; no retry");
        std::process::exit(1)
    }
}
