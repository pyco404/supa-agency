//! Owner-reviewed Supa drafts. No network, credentials, paid inference or authority.
use crate::*;
use serde_json::{Value, json};

pub const X_HANDLE: &str = "supa_agency";
/// Conservative UTF-8 byte ceiling; URLs/mentions are excluded until weighted X
/// character counting and recipient policies are separately reviewed.
pub fn safe_post(text: &str) -> Result<()> {
    clean_text(text, 280)?;
    let lower = text.to_lowercase();
    // X can auto-link bare domains even without a URL scheme or slash. Keep
    // the first-post text-only cost/category policy conservative and explicit.
    let bare_domain = text
        .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '-'))
        .any(|token| {
            let token = token.trim_matches('.');
            let labels: Vec<_> = token.split('.').collect();
            labels.len() > 1
                && labels.last().is_some_and(|label| {
                    label.chars().count() >= 2 && label.chars().all(char::is_alphabetic)
                })
                && labels.iter().all(|label| {
                    !label.is_empty() && label.chars().all(|c| c.is_alphanumeric() || c == '-')
                })
        });
    if text.len() > 280 || bare_domain
        || text.chars().any(|c| c.is_control()||matches!(c,'\u{200b}'..='\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2060}'..='\u{206f}'|'\u{feff}'|'\u{3002}'|'\u{ff0e}'|'\u{ff61}'))
        || text.contains(['@', '#', '/', '\\', '<', '>'])
        || [
            "sk-",
            "ghp_",
            "bearer",
            "password",
            "secret",
            "credential",
            "private memory",
            "railway",
            "postgres",
            "api key",
            "seed phrase",
            "guaranteed",
            "suicide",
            "kill yourself",
        ]
        .iter()
        .any(|s| lower.contains(s))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
/// Only already-released, authoritative metadata may enter this context. No
/// customer prose, private memory lessons or model hypotheses are passed in.
pub fn context(value: &Value, exclude: Option<Uuid>) -> Result<SocialContext> {
    let events: Vec<Event> = value["sources"]
        .as_array()
        .ok_or(Error::Invalid)?
        .iter()
        .filter(|v| v["released"] == true)
        .map(|v| serde_json::from_value(v["event"].clone()).map_err(|_| Error::Invalid))
        .collect::<Result<_>>()?;
    let previous: Vec<PreviousDraft> = value["drafts"]
        .as_array()
        .ok_or(Error::Invalid)?
        .iter()
        .filter(|v| {
            Some(
                v["id"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(Uuid::nil()),
            ) != exclude
        })
        .filter(|v| v["state"] != "rejected" && v["state"] != "revision_requested")
        .take(20)
        .map(|v| PreviousDraft {
            text: v["text"].as_str().unwrap_or("").into(),
            event_ids: serde_json::from_value(v["decision"]["event_ids"].clone())
                .unwrap_or_default(),
            topics: serde_json::from_value(v["topics"].clone()).unwrap_or_default(),
        })
        .collect();
    Ok(social_context(
        Personality::default(),
        &events,
        &[],
        &previous,
    ))
}
/// Explicit offline suggestion, NOT model-generated intelligence. Existing model
/// social_decision integration remains gated by its separate inference approval.
pub fn suggest(context: &SocialContext, variant: u32) -> Result<SocialDecision> {
    let c = context.candidates.first().ok_or(Error::Denied)?;
    let tails = [
        "My confidence is taking a very modest victory lap.",
        "Apparently enthusiasm still needs an evidence trail.",
        "Curiosity remains operational. My ego is under review.",
        "A small plot twist for my extremely supervised existence.",
    ];
    let style = if c.topic == "failure" {
        PostStyle::Failure
    } else if c.topic == "execution" {
        PostStyle::Achievement
    } else {
        PostStyle::Reflection
    };
    let label = if c.event.simulated {
        "Simulated demo: "
    } else {
        ""
    };
    let text = format!(
        "{label}{}; {}",
        c.public_fact,
        tails[variant as usize % tails.len()]
    );
    let decision = SocialDecision {
        choice: SocialChoice::Draft,
        reason: SocialReason::WorthSharing,
        style: Some(style),
        text,
        event_ids: vec![c.event.id],
        memory_ids: vec![],
        claims: vec![SocialClaim {
            event_id: c.event.id,
            fact: c.canonical_fact.clone(),
            wording: c.public_fact.clone(),
        }],
    };
    validate(&decision, context)?;
    Ok(decision)
}
pub fn validate(decision: &SocialDecision, context: &SocialContext) -> Result<()> {
    safe_post(&decision.text)?;
    if decision.choice != SocialChoice::Draft || !decision.memory_ids.is_empty() {
        return Err(Error::Denied);
    }
    decision.validate(context)
}
pub fn proposal(
    context: &Value,
    operation: &str,
    draft: Option<Uuid>,
    text: Option<&str>,
    variant: u32,
) -> Result<Value> {
    let c = self::context(context, draft)?;
    let decision = if operation == "edit" {
        let d = context["drafts"]
            .as_array()
            .ok_or(Error::Invalid)?
            .iter()
            .find(|d| d["id"] == json!(draft))
            .ok_or(Error::NotFound)?;
        let mut decision: SocialDecision =
            serde_json::from_value(d["decision"].clone()).map_err(|_| Error::Invalid)?;
        decision.text = text.ok_or(Error::Invalid)?.into();
        decision
    } else {
        suggest(&c, variant)?
    };
    validate(&decision, &c)?;
    let topics: Vec<_> = c
        .candidates
        .iter()
        .filter(|v| decision.event_ids.contains(&v.event.id))
        .map(|v| v.topic.clone())
        .collect();
    Ok(
        json!({"text":decision.text,"decision":decision,"topics":topics,"draft_origin":"deterministic_suggestion_or_owner_edit","model_requests":0}),
    )
}

/// Autonomous wording is a closed, vetted projection, not arbitrary generated prose.
/// Only individually released cycle/goal metadata is eligible; private memories,
/// financial topics, links and model inventions never enter this first policy.
pub fn autonomous_proposal(context: &Value) -> Result<Value> {
    let mut released = context.clone();
    if let Some(sources) = released["sources"].as_array_mut() {
        sources.retain(|s| matches!(s["event"]["kind"].as_str(), Some("cycle" | "goal")));
    }
    let context = self::context(&released, None)?;
    // More than the twenty-item semantic history window, without inventing facts.
    // Exact normalized wording is also checked against the complete supplied history.
    const VOICE: &[&str] = &[
        "Small field note: my confidence is taking a very modest victory lap",
        "Fresh evidence corner: apparently enthusiasm still needs a paper trail",
        "Quiet progress department: curiosity remains operational while my ego waits outside",
        "Tiny plot development: existence is supervised and the paperwork has opinions",
        "Gentle reality check: optimism has been asked to cite its sources",
        "Brief mechanical interlude: the dramatic soundtrack is doing more work than necessary",
        "Unreasonably calm observation: sometimes the interesting part is what did not explode",
        "Respectfully suspicious update: ambition remains welcome but vibes are not evidence",
        "Polite chaos bulletin: even imaginary laurels deserve an occasional inspection",
        "Current philosophical weather: doubt is useful when it remembers its manners",
        "Minor nerd confession: a tidy explanation is my preferred form of drama",
        "Softly skeptical dispatch: certainty should bring receipts instead of a cape",
        "Practical curiosity log: boring safeguards have surprisingly good comic timing",
        "Measured ego assessment: grand speeches are currently losing to plain facts",
        "Casually technical aside: punctuation is cheaper than pretending to know everything",
        "Local humility forecast: confidence is a hypothesis until reality answers back",
        "Another patient observation: apparently being useful involves listening before monologuing",
        "Rare dramatic pause: my internal narrator prefers fewer exclamation marks",
        "Slightly sarcastic footnote: a convincing story still needs to survive contact with evidence",
        "Friendly status interruption: curiosity would like a chair at the grownup table",
        "Unexpectedly sensible thought: the best plot armor is an honest account of uncertainty",
        "Mildly theatrical reflection: even my imaginary lab coat is asking followup questions",
        "Carefully optimistic marginalia: the lesson matters more than the imaginary applause",
        "Reasonably candid commentary: limitations are more interesting when somebody admits them",
    ];
    let normalize = |text: &str| {
        text.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    for candidate in &context.candidates {
        let mut basis = context.clone();
        basis.candidates = vec![candidate.clone()];
        basis.previous_drafts.clear();
        let mut decision = suggest(&basis, 0)?;
        for voice in VOICE {
            let label = if candidate.event.simulated {
                "Simulated demo: "
            } else {
                ""
            };
            decision.text = format!("{label}{voice}; {}.", candidate.public_fact);
            let duplicate = released["drafts"]
                .as_array()
                .ok_or(Error::Invalid)?
                .iter()
                .any(|d| normalize(d["text"].as_str().unwrap_or("")) == normalize(&decision.text));
            if duplicate || validate(&decision, &context).is_err() {
                continue;
            }
            return Ok(
                json!({"text":decision.text,"decision":decision,"topics":[candidate.topic],
                "draft_origin":"deterministic_autonomous_suggestion","model_requests":0}),
            );
        }
    }
    Err(Error::Denied)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ctx() -> SocialContext {
        let e = Event {
            id: Uuid::new_v4(),
            cycle_id: None,
            kind: "cycle".into(),
            code: "completed".into(),
            simulated: true,
            created_at: Utc::now(),
        };
        social_context(Personality::default(), &[e], &[], &[])
    }
    #[test]
    fn autonomous_policy_never_uses_unreleased_financial_or_private_prose() {
        let event = ctx().candidates[0].event.clone();
        let mut value = json!({"sources":[{"released":true,"event":event}],"drafts":[],"private_memory":"private-lesson-sentinel"});
        let proposal = autonomous_proposal(&value).unwrap();
        assert_eq!(
            proposal["draft_origin"],
            "deterministic_autonomous_suggestion"
        );
        assert!(!proposal.to_string().contains("private-lesson-sentinel"));
        value["sources"][0]["released"] = json!(false);
        assert!(autonomous_proposal(&value).is_err());
        value["sources"][0]["released"] = json!(true);
        value["sources"][0]["event"]["kind"] = json!("simulated_ledger");
        value["sources"][0]["event"]["code"] = json!("revenue");
        assert!(autonomous_proposal(&value).is_err());
    }
    #[test]
    fn unattended_suggestions_continue_without_repeating_or_relaxing_facts() {
        let mut value = json!({"sources":[],"drafts":[]});
        let mut texts = std::collections::BTreeSet::new();
        for n in 0..8 {
            let (kind, code) = [
                ("cycle", "completed"),
                ("cycle", "interrupted"),
                ("goal", "evaluated"),
                ("goal", "added"),
            ][n % 4];
            let event = Event {
                id: Uuid::new_v4(),
                cycle_id: None,
                kind: kind.into(),
                code: code.into(),
                simulated: true,
                created_at: Utc::now(),
            };
            value["sources"]
                .as_array_mut()
                .unwrap()
                .push(json!({"released":true,"event":event}));
            let proposal = autonomous_proposal(&value).unwrap();
            let decision: SocialDecision =
                serde_json::from_value(proposal["decision"].clone()).unwrap();
            validate(&decision, &context(&value, None).unwrap()).unwrap();
            assert!(texts.insert(proposal["text"].as_str().unwrap().to_string()));
            assert!(
                proposal["text"]
                    .as_str()
                    .unwrap()
                    .starts_with("Simulated demo:")
            );
            value["drafts"].as_array_mut().unwrap().insert(0,json!({"id":Uuid::new_v4(),"state":"posted","text":proposal["text"],"decision":proposal["decision"],"topics":proposal["topics"]}));
        }
        assert_eq!(texts.len(), 8);
    }
    #[test]
    fn grounded_supa_suggestions_are_explicit_and_bounded() {
        let c = ctx();
        for n in 0..4 {
            let d = suggest(&c, n).unwrap();
            assert!(d.text.starts_with("Simulated demo:"));
            assert_eq!(d.event_ids.len(), 1);
            validate(&d, &c).unwrap();
        }
    }
    #[test]
    fn sensitive_unsupported_or_duplicate_text_is_rejected() {
        let c = ctx();
        let original = suggest(&c, 0).unwrap();
        for text in [
            "password: hidden",
            &format!("sk-{}", "x".repeat(24)),
            "email@example.com",
            "https://private.example",
            "railway.internal",
            "<script>",
            "private memory",
            "guaranteed returns",
            "example.com",
            "Visit(example.com)",
            "www.example.com.",
            "example。com",
            "hidden\u{200b}format",
        ] {
            assert!(safe_post(text).is_err());
        }
        let mut bad = original.clone();
        bad.text = "I traded real funds.".into();
        assert!(validate(&bad, &c).is_err());
        let mut repeated = c.clone();
        repeated.previous_drafts.push(PreviousDraft {
            text: original.text.clone(),
            event_ids: original.event_ids.clone(),
            topics: vec!["execution".into()],
        });
        assert!(validate(&original, &repeated).is_err());
        bad = original;
        bad.event_ids[0] = Uuid::new_v4();
        assert!(validate(&bad, &c).is_err());
    }
}
