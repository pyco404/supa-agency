//! Bounded supervised plans. Real transport requires a separate default-off gate.
use crate::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReasoningPlan {
    pub provider: String,
    pub model: String,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_requests: u8,
    pub max_cost_microusd: i64,
    pub external_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real: Option<RealReasoningScope>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RealReasoningScope {
    pub project: String,
    pub budget_id: Uuid,
    pub expires_at: DateTime<Utc>,
}
impl ReasoningPlan {
    pub fn mock() -> Self {
        Self {
            provider: "mock".into(),
            model: "synthetic-incident-v1".into(),
            max_input_tokens: 2000,
            max_output_tokens: 256,
            max_requests: 1,
            max_cost_microusd: 0,
            external_tools: vec![],
            real: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.real.is_some() {
            let scope = self.real.as_ref().ok_or(Error::Denied)?;
            if self.provider != "openai"
                || self.model != "gpt-6-luna"
                || self.max_input_tokens != 2000
                || self.max_output_tokens != 256
                || self.max_requests != 1
                || self.max_cost_microusd != 378
                || !self.external_tools.is_empty()
                || scope.budget_id.is_nil()
                || !scope.project.starts_with("proj_")
                || !(6..=120).contains(&scope.project.len())
                || !scope
                    .project
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(Error::Denied);
            }
            return Ok(());
        }
        if self != &Self::mock() {
            return Err(Error::Denied);
        }
        Ok(())
    }
    pub fn validate_dispatch(&self) -> Result<()> {
        self.validate()?;
        if let Some(scope) = &self.real {
            let now = Utc::now();
            if scope.expires_at <= now || scope.expires_at > now + chrono::Duration::minutes(10) {
                return Err(Error::Denied);
            }
        }
        Ok(())
    }
    pub fn policy(&self) -> Result<InferencePolicy> {
        self.validate()?;
        if let Some(scope) = &self.real {
            // Verified standard pricing, 2026-11-08. Conservative cache-write reservation.
            return Ok(InferencePolicy {
                model: self.model.clone(),
                live: true,
                budget_id: Some(scope.budget_id),
                budget_limit_microusd: self.max_cost_microusd,
                input_rate: 100_000,
                cached_input_rate: 10_000,
                cache_write_rate: 125_000,
                output_rate: 500_000,
                input_token_bound: 2000,
                max_output_tokens: 256,
            });
        }
        Ok(InferencePolicy {
            model: self.model.clone(),
            input_token_bound: 2000,
            max_output_tokens: 256,
            ..Default::default()
        })
    }
}
/// The mock model defines a token as four UTF-8 bytes (rounded up).
/// This is NOT a tokenizer for a real LLM. A live adapter needs its own verified
/// tokenizer and separately authorized rates, budget and transport.
pub fn mock_reasoning_tokens(value: &impl Serialize) -> Result<u64> {
    Ok(serde_json::to_vec(value)
        .map_err(|_| Error::Invalid)?
        .len()
        .div_ceil(4) as u64)
}
pub fn reasoning_input(objective: &str, desired: &str, material: &str) -> Result<LiveWorkInput> {
    reasoning_packet(&ReasoningPlan::mock(), objective, desired, material)
}
pub fn reasoning_packet(
    plan: &ReasoningPlan,
    objective: &str,
    desired: &str,
    material: &str,
) -> Result<LiveWorkInput> {
    plan.validate()?;
    for s in [objective, desired, material] {
        validate_work_source(s)?;
    }
    let request = WorkRequest {
        objective: objective.into(),
        material: material.into(),
        requirements: vec![desired.into()],
        require_external_research: false,
    };
    let input = LiveWorkInput {
        request,
        kind: ServiceKind::Summary,
        approved_tools: vec![],
        memories: vec![],
        deterministic_findings: serde_json::Value::Null,
    };
    if (plan.real.is_none() && mock_reasoning_tokens(&input)? > 2000)
        || serde_json::to_vec(&input)
            .map_err(|_| Error::Invalid)?
            .len()
            > 16000
    {
        return Err(Error::ResourceBudget);
    }
    Ok(input)
}
pub fn validate_reasoning_result(
    plan: &ReasoningPlan,
    input: &LiveWorkInput,
    output: &WorkProposal,
    usage: TokenUsage,
) -> Result<()> {
    plan.validate()?;
    usage.validate()?;
    if plan.real.is_some() {
        if usage.input_tokens == 0
            || usage.input_tokens > 2000
            || usage.output_tokens == 0
            || usage.output_tokens > 256
            || plan.policy()?.cost(usage)? > plan.max_cost_microusd
            || !matches!(output.status.as_str(), "incomplete" | "abstained")
            || output.missing_information.is_empty()
            || !input.memories.is_empty()
            || !input.approved_tools.is_empty()
        {
            return Err(Error::Denied);
        }
        return output.validate(input);
    }
    let input_tokens = mock_reasoning_tokens(input)?;
    let output_tokens = mock_reasoning_tokens(output)?;
    if usage.input_tokens != input_tokens
        || usage.output_tokens != output_tokens
        || usage.cached_input_tokens != 0
        || usage.cache_write_tokens != 0
        || input_tokens > plan.max_input_tokens
        || output_tokens > plan.max_output_tokens
        || plan.policy()?.cost(usage)? != 0
        || plan.policy()?.reservation()? != 0
        || !matches!(output.status.as_str(), "incomplete" | "abstained")
        || output.missing_information.is_empty()
    {
        return Err(Error::Denied);
    }
    output.validate(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_plan_pins_prices_budget_expiry_and_caps() {
        let mut p = ReasoningPlan::mock();
        p.provider = "openai".into();
        p.model = "gpt-6-luna".into();
        p.max_cost_microusd = 378;
        p.real = Some(RealReasoningScope {
            project: "proj_fixture".into(),
            budget_id: Uuid::new_v4(),
            expires_at: Utc::now() + chrono::Duration::minutes(5),
        });
        p.validate_dispatch().unwrap();
        let policy = p.policy().unwrap();
        // The mock tokenizer is never used as a real-provider token estimate.
        assert!(reasoning_packet(&p, "Review", "recovered", &"\"".repeat(4000)).is_ok());
        assert_eq!(policy.reservation().unwrap(), 378);
        assert_eq!(
            policy
                .cost(TokenUsage {
                    input_tokens: 2000,
                    output_tokens: 256,
                    ..Default::default()
                })
                .unwrap(),
            328
        );
        let mut bad = p.clone();
        bad.max_cost_microusd = 377;
        assert!(bad.validate().is_err());
        let mut bad = p.clone();
        bad.model = "gpt-6-luna-latest".into();
        assert!(bad.validate().is_err());
        let mut bad = p.clone();
        bad.real.as_mut().unwrap().expires_at = Utc::now();
        assert!(bad.validate_dispatch().is_err());
        let mut bad = p.clone();
        bad.real.as_mut().unwrap().expires_at = Utc::now() + chrono::Duration::minutes(11);
        assert!(bad.validate_dispatch().is_err());
        p.real.as_mut().unwrap().budget_id = Uuid::nil();
        assert!(p.validate().is_err());
    }
    #[test]
    fn only_reviewed_zero_cost_mock_plan_is_allowed() {
        let plan = ReasoningPlan::mock();
        assert!(plan.validate().is_ok());
        assert_eq!(plan.policy().unwrap().reservation().unwrap(), 0);
        for change in ["model", "requests", "budget", "tools", "input", "output"] {
            let mut bad = plan.clone();
            match change {
                "model" => bad.model = "unapproved".into(),
                "requests" => bad.max_requests = 2,
                "budget" => bad.max_cost_microusd = 1,
                "tools" => bad.external_tools.push("web".into()),
                "input" => bad.max_input_tokens = 2001,
                _ => bad.max_output_tokens = 257,
            };
            assert!(bad.validate().is_err());
        }
    }
    #[test]
    fn packet_bound_includes_serialization_and_rejects_secrets() {
        assert!(reasoning_input("Review", "recovered", "Synthetic requests recovered.").is_ok());
        assert!(reasoning_input("Review", "recovered", &"a".repeat(8100)).is_err());
        assert!(reasoning_input("Review", "recovered", &"\"".repeat(4000)).is_err());
        assert!(reasoning_input("Review", "recovered", "password=synthetic").is_err());
    }
}
