//! Closed mock protocol: no HTTP client, real credentials or live dispatch entrypoint.
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use supa_domain::{Error, Result};
use supa_persistence::{AutonomousSocialGrant, Store};
use supa_providers::x_review::{MockResponse, MockXTransport};
use supa_runtime::social_autonomy::tick_mock;
use uuid::Uuid;

struct Wire {
    uncertain: bool,
}
#[async_trait]
impl MockXTransport for Wire {
    async fn exchange(&self, path: &str, body: Option<Value>, _: &str) -> Result<MockResponse> {
        let value = match path {
            "/2/users/me" => json!({"data":{"id":"1","username":"supa_agency"}}),
            "/2/tweets" if self.uncertain => return Err(Error::ProviderTransport),
            "/2/tweets" => {
                json!({"data":{"id":"123456","text":body.ok_or(Error::Invalid)?["text"]}})
            }
            _ => return Err(Error::Denied),
        };
        Ok(MockResponse {
            status: if path == "/2/tweets" { 201 } else { 200 },
            body: serde_json::to_vec(&value).map_err(|_| Error::Invalid)?,
            rate_limit_reset: None,
        })
    }
}
async fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1
        || !matches!(
            args[0].as_str(),
            "check"
                | "authorize"
                | "resume"
                | "pause"
                | "stop"
                | "revoke"
                | "read"
                | "recover"
                | "tick"
                | "tick-uncertain"
        )
    {
        return Err(Error::Denied);
    }
    let store = Store::connect_first_post_local(
        &std::env::var("DATABASE_URL").map_err(|_| Error::Denied)?,
        false,
    )
    .await?;
    store.verify_first_post_runtime_role().await?;
    store.verify_autonomous_runtime_role().await?;
    let owner = Uuid::from_u128(1);
    let grant = Uuid::from_u128(100);
    let expiry: DateTime<Utc> = std::env::var("SUPA_X_MOCK_EXPIRES_AT")
        .map_err(|_| Error::Denied)?
        .parse()
        .map_err(|_| Error::Denied)?;
    if expiry <= Utc::now()
        && matches!(
            args[0].as_str(),
            "authorize" | "resume" | "tick" | "tick-uncertain"
        )
    {
        return Err(Error::Denied);
    }
    let answer = match args[0].as_str() {
        "check" => {
            // This function denies live policy before any grant or mock protocol.
            let answer = store
                .autonomous_social_command_mock(owner, "read", json!({"grant_id":grant}))
                .await;
            if answer.is_err() {
                return Err(Error::Denied);
            }
            json!({"mode":"mock","state":"checked"})
        }
        "authorize" => {
            store
                .autonomous_social_authorize_mock(
                    owner,
                    &AutonomousSocialGrant {
                        grant_id: grant,
                        account: json!({"id":"1","username":"supa_agency","mode":"mock"}),
                        confirmed: true,
                        scope: "original_text_released_metadata_only".into(),
                        expires_at: expiry,
                        max_posts: 6,
                        interval_seconds: 14400,
                        budget_microusd: 150000,
                    },
                )
                .await?
        }
        "tick" | "tick-uncertain" => {
            tick_mock(
                &store,
                owner,
                grant,
                Uuid::new_v4(),
                Wire {
                    uncertain: args[0] == "tick-uncertain",
                },
            )
            .await?
        }
        op => {
            store
                .autonomous_social_command_mock(owner, op, json!({"grant_id":grant}))
                .await?
        }
    };
    // Only categories/counts escape; never draft text, source data or connection strings.
    let expired = answer["grant"]["expires_at"]
        .as_str()
        .and_then(|s| s.parse::<DateTime<Utc>>().ok())
        .is_some_and(|e| e <= Utc::now());
    println!(
        "{}",
        json!({"mode":"mock","state":answer["state"].as_str().or_else(||answer["grant"]["state"].as_str()).unwrap_or("recorded"),"expired":expired,"attempts":answer["attempts"].as_array().map(|a|a.len())})
    );
    Ok(())
}
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("x_mock_worker_denied");
        std::process::exit(1);
    }
}
