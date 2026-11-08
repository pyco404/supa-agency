//! Owner-reviewed Supa drafts. No network, credentials, paid inference or authority.
use crate::*;
use serde_json::{Value, json};

pub const X_HANDLE: &str = "supa_agency";
/// Conservative UTF-8 byte ceiling; URLs/mentions are excluded until weighted X
/// character counting and recipient policies are separately reviewed.
pub fn safe_post(text: &str) -> Result<()> {
    clean_text(text, 280)?;
    let lower = text.to_lowercase();
    if text.len() > 280
        || text.chars().any(char::is_control)
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
