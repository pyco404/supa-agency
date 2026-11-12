//! Deterministic substitute for a model, not a claim of live AI inference.
//! Uses the same structured work-provider interface as the existing live adapter.
use super::*;
pub struct SupervisedMock;
#[async_trait]
impl controlled::ControlledProvider for SupervisedMock {
    fn policy(&self) -> InferencePolicy {
        ReasoningPlan::mock().policy().expect("constant policy")
    }
    fn project(&self) -> &str {
        "local-synthetic"
    }
    fn work_evidence_kind(&self) -> &str {
        "mock"
    }
    async fn verify(&self) -> Result<()> {
        Ok(())
    }
    async fn reason(&self, _: &str, _: &[Memory]) -> Invocation<LiveProposal> {
        Invocation::unmetered(Err(Error::Denied))
    }
    async fn work(&self, input: &LiveWorkInput) -> Invocation<WorkProposal> {
        let findings = input
            .request
            .material
            .lines()
            .enumerate()
            .filter(|(_, s)| !s.trim().is_empty())
            .take(2)
            .map(|(i, s)| {
                let quote: String = s.chars().take(70).collect();
                WorkFinding {
                    observation: quote.clone(),
                    interpretation: "Unverified supplied note.".into(),
                    evidence: vec![WorkCitation {
                        source: "material".into(),
                        line: i + 1,
                        quote,
                    }],
                }
            })
            .collect();
        let recovered = input.request.material.to_lowercase().contains("recover");
        let output = WorkProposal {
            interpretation: if recovered {
                "Recovery is reported; causation remains uncertain."
            } else {
                "Cause and recovery remain uncertain."
            }
            .into(),
            summary: "Synthetic incident review; owner assessment required.".into(),
            status: "incomplete".into(),
            tools: vec![],
            findings,
            totals: vec![],
            proposed_steps: vec!["Compare synthetic traces before and after the incident.".into()],
            missing_information: vec!["No independent causal test or diagnostic evidence.".into()],
            memory_ids: vec![],
        };
        let usage = mock_reasoning_tokens(input).and_then(|input_tokens| {
            Ok(TokenUsage {
                input_tokens,
                output_tokens: mock_reasoning_tokens(&output)?,
                ..Default::default()
            })
        });
        match usage {
            Ok(usage) => Invocation {
                result: Ok(output),
                usage: Some(usage),
                diagnostic: None,
            },
            Err(e) => Invocation::unmetered(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controlled::ControlledProvider;
    #[tokio::test]
    async fn mock_uses_structured_provider_contract_and_reports_bounded_usage() {
        let input = reasoning_input(
            "Review synthetic incident",
            "recovered",
            "12:00 Synthetic requests timed out.\n12:05 Synthetic service recovered after restart.",
        )
        .unwrap();
        let invocation = SupervisedMock.work(&input).await;
        let report = invocation.result.unwrap();
        let usage = invocation.usage.unwrap();
        assert!(usage.output_tokens <= 256);
        validate_reasoning_result(&ReasoningPlan::mock(), &input, &report, usage).unwrap();
        assert_eq!(report.status, "incomplete");
        assert!(!report.missing_information.is_empty());
        let mut wrong = report;
        wrong.findings[0].observation = "Invented achievement".into();
        assert!(wrong.validate(&input).is_err());
    }
}
