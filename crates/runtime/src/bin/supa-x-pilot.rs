//! Production one-shot launcher; offline validation cannot be selected here.
#[cfg(feature = "pilot-offline-tests")]
compile_error!("offline validation is forbidden in the production pilot executable");
#[path = "../x_pilot_process.rs"]
mod process;
#[tokio::main]
async fn main() {
    if process::run().await.is_err() {
        eprintln!("STOPPED: pilot unconfirmed; preserve reservation, no retry");
        std::process::exit(1)
    }
}
