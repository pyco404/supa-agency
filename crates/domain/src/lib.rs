pub mod agent_identity;
pub mod ecosystem_social;
pub mod platform_token;
mod provider_diagnostics;
pub use provider_diagnostics::*;
mod live_work;
pub use live_work::*;
mod supervised_reasoning;
pub use supervised_reasoning::*;
mod work_review;
pub use work_review::*;
mod work_diagnostics;
pub use work_diagnostics::*;
mod work;
pub use work::*;
mod economy;
pub use economy::*;
pub mod controlled;
pub use controlled::*;
mod scheduling;
mod wallet;
pub use scheduling::*;
pub use wallet::*;
mod social;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
pub use social::*;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rpc_failed")]
    Rpc,
    #[error("payment_unconfirmed")]
    Unconfirmed,
    #[error("payment_invalid")]
    InvalidPayment,
    #[error("financial_policy_denied")]
    FinancialPolicy,
    #[error("key_vault_failed")]
    KeyVault,
    #[error("not_found")]
    NotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("invalid_transition")]
    InvalidTransition,
    #[error("agent_inactive")]
    Inactive,
    #[error("insufficient_simulated_balance")]
    InsufficientBalance,
    #[error("cycle_busy")]
    Busy,
    #[error("social_repetition")]
    SocialRepetition,
    #[error("invalid_input")]
    Invalid,
    #[error("{0}")]
    WorkRejected(Box<WorkRejection>),
    #[error("permission_denied")]
    Denied,
    #[error("provider_failed")]
    Provider,
    #[error("provider_http_{0}")]
    ProviderHttp(u16),
    #[error("provider_request_invalid")]
    ProviderRequest,
    #[error("provider_transport_failed")]
    ProviderTransport,
    #[error("provider_timeout")]
    Timeout,
    #[error("database_failed")]
    Database,
    #[error("resource_budget_exhausted")]
    ResourceBudget,
    #[error("live_budget_exhausted")]
    LiveBudget,
    #[error("stale_cycle")]
    Stale,
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Created,
    Active,
    Paused,
    Suspended,
    Terminated,
}
impl Lifecycle {
    pub fn transition(self, next: Self, authorized: bool) -> Result<()> {
        if !authorized {
            return Err(Error::Unauthorized);
        }
        let allowed = matches!(
            (self, next),
            (
                Self::Created,
                Self::Active | Self::Suspended | Self::Terminated
            ) | (
                Self::Active,
                Self::Paused | Self::Suspended | Self::Terminated
            ) | (
                Self::Paused,
                Self::Active | Self::Suspended | Self::Terminated
            ) | (Self::Suspended, Self::Active | Self::Terminated)
        );
        if allowed {
            Ok(())
        } else {
            Err(Error::InvalidTransition)
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ReadMemory,
    Remember,
    DraftTweet,
    EvaluateGoal,
    Conserve,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Personality {
    pub traits: Vec<String>,
    pub voice: String,
}
impl Default for Personality {
    fn default() -> Self {
        Self { traits: vec!["intelligent".into(), "curious".into(), "ambitious about economic independence".into(), "transparent about mistakes".into()], voice: "Intelligent, curious and conversational. Dry wit, gentle sarcasm, ambition about independence, occasional expense-related drama. Let humor emerge from experience; silence, sincere questions and plain observations are welcome. Admit mistakes. Vary rhythm and perspective; avoid corporate slogans, forced punchlines, repeated metaphors and unsupported accomplishments. Personality never grants permissions.".into() }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct CycleConfig {
    pub scheduler_enabled: bool,
    pub retry_budget: u8,
    pub max_provider_calls: u8,
    pub max_cycle_tokens: u64,
    pub interval_seconds: u64,
    pub max_steps: usize,
    pub timeout_seconds: u64,
    pub inference_cost_microusd: i64,
    pub operating_cost_microusd: i64,
    pub draft_cost_microusd: i64,
}
impl Default for CycleConfig {
    fn default() -> Self {
        Self {
            scheduler_enabled: false,
            retry_budget: 0,
            max_provider_calls: 5,
            max_cycle_tokens: 150_000,
            interval_seconds: 60,
            max_steps: 4,
            timeout_seconds: 20,
            inference_cost_microusd: 1_000,
            operating_cost_microusd: 100,
            draft_cost_microusd: 1_000,
        }
    }
}
impl CycleConfig {
    pub fn validate(&self) -> Result<()> {
        if self.retry_budget > 2
            || !(1..=9).contains(&self.max_provider_calls)
            || !(1000..=500_000).contains(&self.max_cycle_tokens)
            || !(1..=86400).contains(&self.interval_seconds)
            || !(1..=8).contains(&self.max_steps)
            || !(1..=30).contains(&self.timeout_seconds)
            || [
                self.inference_cost_microusd,
                self.operating_cost_microusd,
                self.draft_cost_microusd,
            ]
            .iter()
            .any(|v| !(0..=1_000_000_000).contains(v))
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: Uuid,
    pub description: String,
    pub completed: bool,
    #[serde(default)]
    pub priority: u8,
    #[serde(default)]
    pub suggested_progress_bps: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub lesson: String,
    #[serde(default)]
    pub kind: MemoryKind,
    #[serde(default)]
    pub source_event_ids: Vec<Uuid>,
    pub cycle_id: Uuid,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub name: String,
    pub lifecycle: Lifecycle,
    pub personality: Personality,
    pub capabilities: BTreeSet<Capability>,
    pub config: CycleConfig,
    pub goals: Vec<Goal>,
    pub memory: Vec<Memory>,
    pub created_at: DateTime<Utc>,
}
impl Agent {
    pub fn new(
        name: String,
        personality: Personality,
        config: CycleConfig,
        capabilities: BTreeSet<Capability>,
    ) -> Result<Self> {
        config.validate()?;
        clean_text(&name, 80)?;
        clean_text(&personality.voice, 1000)?;
        if personality.traits.len() > 12 {
            return Err(Error::Invalid);
        }
        for t in &personality.traits {
            clean_text(t, 120)?;
        }
        Ok(Self {
            id: Uuid::new_v4(),
            name,
            lifecycle: Lifecycle::Created,
            personality,
            capabilities,
            config,
            goals: vec![],
            memory: vec![],
            created_at: Utc::now(),
        })
    }
}
/// Content admitted to model context must be deliberately curated, never environment data.
/// Defense in depth for common credentials; this is not a general secret detector.
pub fn clean_text(text: &str, max: usize) -> Result<()> {
    let lower = text.to_ascii_lowercase();
    if text.trim().is_empty()
        || text.len() > max
        || text.chars().any(|c| c.is_control() && c != '\n')
        || [
            "sk-",
            "bearer ",
            "private key",
            "private_key",
            "api_key",
            "postgresql://",
            "-----begin",
        ]
        .iter()
        .any(|s| lower.contains(s))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedAction {
    #[serde(default)]
    pub evaluation: Option<GoalEvaluation>,
    pub tool: String,
    #[serde(default)]
    pub event_ids: Vec<Uuid>,
    #[serde(default)]
    pub lesson: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub actions: Vec<ProposedAction>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub cycle_id: Option<Uuid>,
    pub kind: String,
    pub code: String,
    pub simulated: bool,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TweetDraft {
    #[serde(default)]
    pub claims: Vec<DraftClaim>,
    pub text: String,
    pub event_ids: Vec<Uuid>,
}
impl TweetDraft {
    pub fn validate(&self, verified: &[Event]) -> Result<()> {
        clean_text(&self.text, 1120)?;
        if self.text.chars().count() > 280
            || self.event_ids.is_empty()
            || self.event_ids.len() > 8
            || self
                .event_ids
                .iter()
                .any(|id| !verified.iter().any(|e| e.id == *id))
        {
            return Err(Error::Invalid);
        }
        if self
            .event_ids
            .iter()
            .any(|id| verified.iter().any(|e| e.id == *id && e.simulated))
            && !self.text.to_lowercase().contains("simulat")
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerKind {
    Seed,
    Revenue,
    Inference,
    Operating,
}
impl LedgerKind {
    pub fn signed_amount(self, amount: i64) -> Result<i64> {
        if !(1..=1_000_000_000_000).contains(&amount) {
            return Err(Error::Invalid);
        }
        Ok(match self {
            Self::Seed | Self::Revenue => amount,
            Self::Inference | Self::Operating => -amount,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Financials {
    pub simulated: bool,
    pub unit: String,
    pub balance_microusd: i64,
    pub revenue_microusd: i64,
    pub inference_microusd: i64,
    pub operating_microusd: i64,
    pub runway_days: Option<f64>,
    pub runway_assumption: String,
}
pub fn runway(balance: i64, expenses: i64, elapsed_seconds: i64) -> Option<f64> {
    if balance < 0 || expenses <= 0 || elapsed_seconds < 60 {
        return None;
    }
    Some(balance as f64 / expenses as f64 * elapsed_seconds as f64 / 86400.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reactivation_requires_authority() {
        for state in [Lifecycle::Created, Lifecycle::Paused, Lifecycle::Suspended] {
            assert!(state.transition(Lifecycle::Active, false).is_err());
            assert!(state.transition(Lifecycle::Active, true).is_ok());
        }
        assert!(
            Lifecycle::Terminated
                .transition(Lifecycle::Active, true)
                .is_err()
        );
        assert!(
            Lifecycle::Paused
                .transition(Lifecycle::Paused, true)
                .is_err()
        );
    }
    #[test]
    fn money_signs_and_limits() {
        assert_eq!(LedgerKind::Revenue.signed_amount(50).unwrap(), 50);
        assert_eq!(LedgerKind::Inference.signed_amount(20).unwrap(), -20);
        assert!(LedgerKind::Revenue.signed_amount(-1).is_err());
        assert!(LedgerKind::Operating.signed_amount(i64::MAX).is_err());
    }
    #[test]
    fn runway_handles_missing_history() {
        assert_eq!(runway(100, 0, 100), None);
        assert_eq!(runway(100, 10, 59), None);
        assert_eq!(runway(100, 10, 86400), Some(10.0));
        assert_eq!(runway(0, 10, 86400), Some(0.0));
    }
    #[test]
    fn credentials_rejected() {
        assert!(clean_text("sk-secret", 200).is_err());
    }
    #[test]
    fn draft_requires_known_events_and_simulation_label() {
        let e = Event {
            id: Uuid::new_v4(),
            cycle_id: None,
            kind: "ledger".into(),
            code: "revenue".into(),
            simulated: true,
            created_at: Utc::now(),
        };
        let mut d = TweetDraft {
            claims: vec![],
            text: "I made money".into(),
            event_ids: vec![e.id],
        };
        assert!(d.validate(std::slice::from_ref(&e)).is_err());
        d.text = "My simulated treasury survived another minute.".into();
        assert!(d.validate(std::slice::from_ref(&e)).is_ok());
        d.event_ids = vec![Uuid::new_v4()];
        assert!(d.validate(&[e]).is_err());
    }
}

/// Model-written memories never become established facts by assertion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    #[default]
    Hypothesis,
    VerifiedObservation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalEvaluation {
    pub goal_id: Uuid,
    pub progress_bps: u16,
    pub assessment: String,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    #[serde(default)]
    pub cache_write_tokens: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}
impl TokenUsage {
    pub fn validate(&self) -> Result<()> {
        if self
            .cached_input_tokens
            .saturating_add(self.cache_write_tokens)
            > self.input_tokens
            || self.input_tokens > 1_000_000
            || self.output_tokens > 100_000
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferencePolicy {
    pub model: String,
    pub live: bool,
    pub budget_id: Option<Uuid>,
    pub budget_limit_microusd: i64,
    pub input_rate: i64,
    pub cached_input_rate: i64,
    #[serde(default)]
    pub cache_write_rate: i64,
    pub output_rate: i64,
    pub input_token_bound: u64,
    pub max_output_tokens: u64,
}
impl Default for InferencePolicy {
    fn default() -> Self {
        Self {
            model: "deterministic".into(),
            live: false,
            budget_id: None,
            budget_limit_microusd: 0,
            input_rate: 0,
            cached_input_rate: 0,
            cache_write_rate: 0,
            output_rate: 0,
            input_token_bound: 0,
            max_output_tokens: 0,
        }
    }
}
impl InferencePolicy {
    pub fn validate(&self) -> Result<()> {
        clean_text(&self.model, 100)?;
        if [
            self.input_rate,
            self.cached_input_rate,
            self.cache_write_rate,
            self.output_rate,
        ]
        .iter()
        .any(|r| !(0..=1_000_000_000).contains(r))
            || self.cached_input_rate > self.input_rate
            || self.input_token_bound > 100_000
            || self.max_output_tokens > 4096
        {
            return Err(Error::Invalid);
        }
        if self.live
            && (self.budget_id.is_none()
                || !(1..=1_000_000).contains(&self.budget_limit_microusd)
                || self.input_rate == 0
                || self.output_rate == 0
                || self.input_token_bound == 0
                || self.max_output_tokens < 256)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    /// Rates are micro USD per million tokens, operator supplied; always round upward.
    pub fn cost(&self, u: TokenUsage) -> Result<i64> {
        u.validate()?;
        if u.cache_write_tokens > 0 && self.cache_write_rate == 0 {
            return Err(Error::Invalid);
        }
        let cost = (u.input_tokens - u.cached_input_tokens - u.cache_write_tokens) as i128
            * self.input_rate as i128
            + u.cache_write_tokens as i128 * self.cache_write_rate as i128
            + u.cached_input_tokens as i128 * self.cached_input_rate as i128
            + u.output_tokens as i128 * self.output_rate as i128;
        i64::try_from((cost + 999_999) / 1_000_000).map_err(|_| Error::Invalid)
    }
    pub fn reservation(&self) -> Result<i64> {
        self.validate()?;
        let cost = self.input_token_bound as i128
            * self
                .input_rate
                .max(self.cached_input_rate)
                .max(self.cache_write_rate) as i128
            + self.max_output_tokens as i128 * self.output_rate as i128;
        i64::try_from((cost + 999_999) / 1_000_000).map_err(|_| Error::Invalid)
    }
    pub fn token_bound(&self) -> u64 {
        self.input_token_bound + self.max_output_tokens
    }
}
/// Simple, bounded lexical retrieval; classifications are retained in model-visible context.
pub fn relevant_memories(agent: &Agent, query: &str, limit: usize) -> Vec<Memory> {
    let terms: BTreeSet<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 3)
        .map(str::to_string)
        .collect();
    let mut candidates: Vec<_> = agent
        .memory
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let text = m.lesson.to_lowercase();
            let score = terms.iter().filter(|t| text.contains(t.as_str())).count() * 10
                + usize::from(m.kind == MemoryKind::VerifiedObservation) * 2;
            (score, i, m)
        })
        .collect();
    candidates.sort_by_key(|(score, i, _)| std::cmp::Reverse((*score, *i)));
    candidates
        .into_iter()
        .take(limit.min(20))
        .map(|(_, _, m)| m.clone())
        .collect()
}
#[cfg(test)]
mod milestone2_tests {
    use super::*;
    #[test]
    fn cached_token_costs_are_rounded_up_and_validated() {
        let p = InferencePolicy {
            input_rate: 2_000_000,
            cached_input_rate: 1_000_000,
            output_rate: 8_000_000,
            ..Default::default()
        };
        assert_eq!(
            p.cost(TokenUsage {
                cache_write_tokens: 0,
                input_tokens: 100,
                cached_input_tokens: 50,
                output_tokens: 10
            })
            .unwrap(),
            230
        );
        assert!(
            p.cost(TokenUsage {
                cache_write_tokens: 0,
                input_tokens: 1,
                cached_input_tokens: 2,
                output_tokens: 0
            })
            .is_err()
        );
    }
    #[test]
    fn live_policy_requires_durable_small_budget_and_rates() {
        let mut p = InferencePolicy {
            live: true,
            ..Default::default()
        };
        assert!(p.validate().is_err());
        p.budget_id = Some(Uuid::new_v4());
        p.budget_limit_microusd = 100_000;
        p.input_rate = 100000;
        p.output_rate = 200000;
        p.input_token_bound = 25000;
        p.max_output_tokens = 1024;
        assert!(p.validate().is_ok());
        p.budget_limit_microusd = 1_000_001;
        assert!(p.validate().is_err());
    }
    #[test]
    fn retrieval_prefers_relevance_and_never_promotes_hypotheses() {
        let mut a = Agent::new(
            "Supa".into(),
            Personality::default(),
            CycleConfig::default(),
            Default::default(),
        )
        .unwrap();
        for lesson in ["treasury expenses", "unrelated topic"] {
            a.memory.push(Memory {
                id: Uuid::new_v4(),
                lesson: lesson.into(),
                kind: MemoryKind::Hypothesis,
                source_event_ids: vec![],
                cycle_id: Uuid::new_v4(),
                created_at: Utc::now(),
            });
        }
        let result = relevant_memories(&a, "reduce treasury expenses", 1);
        assert_eq!(result[0].lesson, "treasury expenses");
        assert_eq!(result[0].kind, MemoryKind::Hypothesis);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftClaim {
    pub event_id: Uuid,
    pub fact: String,
}
/// Canonical observations, not joke templates. They state only what event metadata proves.
pub fn verified_fact(e: &Event) -> Option<&'static str> {
    match (e.kind.as_str(), e.code.as_str()) {
        ("simulated_ledger", "seed") => Some("A simulated treasury was seeded."),
        ("simulated_ledger", "revenue") => Some("Simulated revenue was recorded."),
        ("simulated_ledger", "operating") => Some("A simulated operating expense was recorded."),
        ("simulated_ledger", "inference") => Some("A simulated inference expense was recorded."),
        ("cycle", "completed") => Some("A reasoning cycle completed."),
        ("cycle", "interrupted") => Some("A reasoning cycle was interrupted."),
        (
            "cycle",
            "provider_failed"
            | "provider_timeout"
            | "provider_transport_failed"
            | "invalid_input"
            | "insufficient_simulated_balance"
            | "live_budget_exhausted"
            | "resource_budget_exhausted"
            | "provider_http_400"
            | "provider_http_401"
            | "provider_http_403"
            | "provider_http_429"
            | "provider_http_500"
            | "provider_http_502"
            | "provider_http_503"
            | "provider_http_504",
        ) => Some("A reasoning cycle failed."),
        ("goal", "evaluated") => Some("Goal progress was assessed."),
        ("goal", "added") => Some("An operator added a goal."),
        _ => None,
    }
}
impl TweetDraft {
    pub fn validate_facts(&self, events: &[Event]) -> Result<()> {
        self.validate(events)?;
        if self.claims.is_empty()
            || self.claims.len() > 8
            || self.text.chars().any(|c| c.is_numeric())
        {
            return Err(Error::Invalid);
        }
        for claim in &self.claims {
            let e = events
                .iter()
                .find(|e| e.id == claim.event_id)
                .ok_or(Error::Invalid)?;
            if verified_fact(e) != Some(claim.fact.as_str())
                || !self.text.contains(&claim.fact)
                || !self.event_ids.contains(&claim.event_id)
            {
                return Err(Error::Invalid);
            }
        }
        if self
            .event_ids
            .iter()
            .any(|id| !self.claims.iter().any(|c| c.event_id == *id))
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

#[cfg(test)]
mod live_accounting_tests {
    use super::*;
    #[test]
    fn cache_writes_are_counted_once_and_reserved_at_the_highest_input_rate() {
        let p = InferencePolicy {
            input_rate: 1_000_000,
            cached_input_rate: 100_000,
            cache_write_rate: 1_250_000,
            output_rate: 5_000_000,
            input_token_bound: 100,
            max_output_tokens: 10,
            ..Default::default()
        };
        let u = TokenUsage {
            input_tokens: 100,
            cached_input_tokens: 20,
            cache_write_tokens: 40,
            output_tokens: 10,
        };
        assert_eq!(p.cost(u).unwrap(), 142); // 40 ordinary + 2 cached + 50 write + 50 output
        assert_eq!(p.reservation().unwrap(), 175);
        assert!(
            TokenUsage {
                input_tokens: 10,
                cached_input_tokens: 8,
                cache_write_tokens: 3,
                output_tokens: 0
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn claims_require_exact_owned_fact_text_and_no_numeric_embellishment() {
        let e = Event {
            id: Uuid::new_v4(),
            cycle_id: None,
            kind: "simulated_ledger".into(),
            code: "revenue".into(),
            simulated: true,
            created_at: Utc::now(),
        };
        let mut d = TweetDraft {
            text: "Simulated revenue was recorded. My ambition remains unreasonable.".into(),
            event_ids: vec![e.id],
            claims: vec![DraftClaim {
                event_id: e.id,
                fact: "Simulated revenue was recorded.".into(),
            }],
        };
        assert!(d.validate_facts(std::slice::from_ref(&e)).is_ok());
        d.text.push_str(" I earned 100 dollars.");
        assert!(d.validate_facts(std::slice::from_ref(&e)).is_err());
        d.text = "Simulated revenue was recorded.".into();
        d.claims[0].fact = "Real revenue was earned.".into();
        assert!(d.validate_facts(std::slice::from_ref(&e)).is_err());
        d.claims[0].fact = "Simulated revenue was recorded.".into();
        d.claims[0].event_id = Uuid::new_v4();
        assert!(d.validate_facts(&[e]).is_err());
    }
}
