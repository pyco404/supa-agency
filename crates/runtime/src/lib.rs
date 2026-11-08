pub mod controlled;
pub mod economy;
pub mod evaluation;
pub mod health;
pub mod live_work;
pub mod social_autonomy;
pub mod supervised;
pub mod wallet;
use std::{sync::Arc, time::Duration};
use supa_capabilities::{REGISTRY, audit_name, authorize};
use supa_domain::*;
use supa_persistence::{Store, audit_proposal};
use supa_providers::{Context, Invocation, Reasoner};
use uuid::Uuid;

#[derive(Clone)]
pub struct Runtime {
    pub store: Store,
    reasoner: Arc<dyn Reasoner>,
    worker: Option<Uuid>,
}
impl Runtime {
    pub fn new(store: Store, reasoner: Arc<dyn Reasoner>) -> Self {
        Self {
            store,
            reasoner,
            worker: None,
        }
    }
    pub fn with_worker(mut self, worker: Uuid) -> Self {
        self.worker = Some(worker);
        self
    }
    pub async fn cycle(&self, id: Uuid, scheduled: bool) -> Result<Uuid> {
        if self.store.operational_context().is_some()
            || self.store.schedule_status(id).await?.policy.is_some()
        {
            // A configured live/manual provider is never selected for managed schedules.
            let mut offline = Runtime::new(self.store.clone(), Arc::new(supa_providers::Offline));
            offline.worker = self.worker;
            return offline.cycle_inner(id, scheduled, None).await;
        }
        self.cycle_inner(id, scheduled, None).await
    }
    /// Operator-driven draft evaluation, not an autonomous planning result. Uses the same policy/execution path.
    pub async fn draft_cycle(&self, id: Uuid, events: Vec<Uuid>) -> Result<Uuid> {
        self.cycle_inner(
            id,
            false,
            Some(Plan {
                actions: vec![ProposedAction {
                    tool: "draft_tweet".into(),
                    event_ids: events,
                    lesson: None,
                    evaluation: None,
                }],
            }),
        )
        .await
    }
    /// A social opportunity, not an instruction to manufacture a post.
    pub async fn consider_tweet_cycle(&self, id: Uuid) -> Result<Uuid> {
        self.cycle_inner(
            id,
            false,
            Some(Plan {
                actions: vec![ProposedAction {
                    tool: "consider_tweet".into(),
                    event_ids: vec![],
                    lesson: None,
                    evaluation: None,
                }],
            }),
        )
        .await
    }
    async fn social_context(&self, id: Uuid) -> Result<SocialContext> {
        let a = self.store.get(id).await?;
        let history = self.store.social_history(id).await?;
        let mut events = self.store.events(id).await?;
        // Retrieve older source events for relevant long-term verified memories, with existing ownership checks.
        let mut source_ids = std::collections::BTreeSet::new();
        let query = events
            .iter()
            .filter_map(|e| social_fact(e).map(|(topic, phrase, _)| format!("{topic} {phrase}")))
            .collect::<Vec<_>>()
            .join(" ");
        let relevant = relevant_memories(&a, &query, 12);
        for memory in relevant
            .iter()
            .chain(a.memory.iter().take(4))
            .filter(|m| m.kind == MemoryKind::VerifiedObservation)
        {
            source_ids.extend(memory.source_event_ids.iter().take(8).copied());
        }
        let source_ids: Vec<_> = source_ids
            .into_iter()
            .filter(|id| !events.iter().any(|e| e.id == *id))
            .take(64)
            .collect();
        for chunk in source_ids.chunks(8) {
            events.extend(self.store.verified_events(id, chunk).await?);
        }
        Ok(supa_domain::social_context(
            a.personality,
            &events,
            &a.memory,
            &history,
        ))
    }
    async fn cycle_inner(
        &self,
        id: Uuid,
        scheduled: bool,
        requested: Option<Plan>,
    ) -> Result<Uuid> {
        let cycle = self
            .store
            .begin_cycle_for(id, scheduled, self.worker)
            .await?;
        tracing::info!(agent_id=%id,cycle_id=%cycle,"cycle_started");
        let outcome =
            tokio::time::timeout(Duration::from_secs(180), self.run(id, cycle, requested))
                .await
                .unwrap_or(Err(Error::Timeout));
        if let Err(e) = self.store.finish(id, cycle, outcome.as_ref().err()).await
            && (!matches!(e, Error::Stale) || outcome.is_ok())
        {
            return Err(e);
        }
        match &outcome {
            Ok(()) => tracing::info!(agent_id=%id,cycle_id=%cycle,"cycle_completed"),
            Err(e) => tracing::warn!(agent_id=%id,cycle_id=%cycle,error_code=%e,"cycle_failed"),
        }
        outcome?;
        Ok(cycle)
    }
    async fn context(&self, id: Uuid) -> Result<(Agent, Context)> {
        let mut a = self.store.get(id).await?;
        if let Some(policy) = self.store.schedule_status(id).await?.policy {
            a.goals.retain(|goal| policy.goal_ids.contains(&goal.id));
            a.capabilities.retain(|cap| {
                matches!(
                    cap,
                    Capability::ReadMemory | Capability::Remember | Capability::EvaluateGoal
                )
            });
        }
        let mut goals: Vec<_> = a.goals.iter().filter(|g| !g.completed).cloned().collect();
        goals.sort_by_key(|g| std::cmp::Reverse(g.priority));
        let query = goals
            .iter()
            .map(|g| g.description.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let c = Context {
            name: a.name.clone(),
            personality: a.personality.clone(),
            goals: goals.into_iter().take(10).collect(),
            memory: relevant_memories(&a, &query, 12),
            verified_events: self.store.events(id).await?.into_iter().take(20).collect(),
            permitted_tools: REGISTRY
                .iter()
                .filter(|t| a.capabilities.contains(&t.capability) && t.name != "draft_tweet")
                .map(|t| t.name.into())
                .collect(),
            max_steps: a.config.max_steps,
            financials: self.store.financials(id).await?,
            completed_cycles: self.store.completed_cycle_count(id).await?,
            wallet_treasury: match self.store.treasury(id).await {
                Ok(t) => Some(t),
                Err(Error::NotFound) => None,
                Err(e) => return Err(e),
            },
        };
        Ok((a, c))
    }
    async fn run(&self, id: Uuid, cycle: Uuid, requested: Option<Plan>) -> Result<()> {
        let (a, c) = self.context(id).await?;
        self.store
            .charge(
                id,
                cycle,
                LedgerKind::Operating,
                a.config.operating_cost_microusd,
                Uuid::new_v4(),
            )
            .await?;
        let plan = if let Some(p) = requested {
            p
        } else {
            self.invoke(
                id,
                cycle,
                "plan",
                a.config.inference_cost_microusd,
                &a.config,
                || self.reasoner.plan_metered(&c),
            )
            .await?
        };
        if plan.actions.len() > a.config.max_steps {
            return Err(Error::Invalid);
        }
        let proposals: Vec<_> = plan
            .actions
            .iter()
            .map(|p| audit_proposal(audit_name(&p.tool), p))
            .collect();
        let ids = self.store.propose(id, cycle, &proposals).await?;
        for (p, action_id) in plan.actions.iter().zip(ids) {
            let (current, context) = self.context(id).await?;
            let cap = authorize(&current, p);
            self.store
                .authorize_action(id, cycle, action_id, cap.is_ok())
                .await?;
            let capability = match cap {
                Ok(c) => c,
                Err(_) => continue,
            };
            match capability {
                Capability::ReadMemory => {
                    self.store
                        .execute_action(id, cycle, action_id, None, None)
                        .await?
                }
                Capability::Remember => {
                    self.store
                        .execute_action(id, cycle, action_id, p.lesson.as_deref(), None)
                        .await?
                }
                Capability::EvaluateGoal => {
                    self.store
                        .evaluate_goal(
                            id,
                            cycle,
                            action_id,
                            p.evaluation.as_ref().ok_or(Error::Invalid)?,
                            &p.event_ids,
                        )
                        .await?;
                }
                Capability::Conserve => {
                    self.store.conserve(id, cycle, action_id).await?;
                }
                Capability::DraftTweet if p.tool == "consider_tweet" => {
                    let social = self.social_context(id).await?;
                    let decision = if social.candidates.is_empty() {
                        SocialDecision::abstain(SocialReason::NothingNew)
                    } else {
                        self.invoke(
                            id,
                            cycle,
                            "draft",
                            current.config.draft_cost_microusd,
                            &current.config,
                            || self.reasoner.social_metered(&social),
                        )
                        .await?
                    };
                    self.store
                        .finish_social(id, cycle, action_id, &social, &decision)
                        .await?;
                }
                Capability::DraftTweet => {
                    let events = self.store.verified_events(id, &p.event_ids).await?;
                    if events.len() != p.event_ids.len() {
                        return Err(Error::Invalid);
                    }
                    let draft = self
                        .invoke(
                            id,
                            cycle,
                            "draft",
                            current.config.draft_cost_microusd,
                            &current.config,
                            || self.reasoner.draft_metered(&context, &events),
                        )
                        .await?;
                    draft.validate(&events)?;
                    self.store
                        .execute_action(id, cycle, action_id, None, Some(&draft))
                        .await?;
                }
            }
        }
        Ok(())
    }
    async fn invoke<T, F, Fut>(
        &self,
        id: Uuid,
        cycle: Uuid,
        purpose: &str,
        simulated_cost: i64,
        config: &CycleConfig,
        mut operation: F,
    ) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Invocation<T>>,
    {
        let policy = self.reasoner.policy_for(purpose);
        policy.validate()?;
        for attempt in 0..=config.retry_budget {
            let call = self
                .store
                .reserve_inference(id, cycle, purpose, attempt, &policy, simulated_cost)
                .await?;
            let invocation =
                tokio::time::timeout(Duration::from_secs(config.timeout_seconds), operation())
                    .await
                    .unwrap_or(Invocation {
                        diagnostic: None,
                        result: Err(Error::Timeout),
                        usage: None,
                    });
            self.store
                .finish_inference(id, call, invocation.usage, invocation.result.as_ref().err())
                .await?;
            tracing::info!(agent_id=%id,cycle_id=%cycle,call_id=%call,attempt,has_usage=invocation.usage.is_some(),"inference_accounted");
            // Only explicitly rejected rate-limited requests are retried. Uncertain paid requests are never replayed.
            if matches!(invocation.result, Err(Error::ProviderHttp(429)))
                && attempt < config.retry_budget
            {
                tokio::time::sleep(Duration::from_millis(250 * (1u64 << attempt))).await;
                continue;
            }
            return invocation.result;
        }
        Err(Error::ResourceBudget)
    }
    /// Restarts recover only expired work; persisted lifecycle state is never changed.
    /// Sequential scheduling bounds provider concurrency to one autonomous call per process.
    pub async fn tick(&self) -> Result<()> {
        if let Some(worker) = self.worker {
            self.store.worker_tick(worker, false, None).await?;
        }
        let result = self.tick_inner().await;
        if let Some(worker) = self.worker {
            self.store
                .worker_tick(worker, true, result.as_ref().err())
                .await?;
        }
        result
    }
    async fn tick_inner(&self) -> Result<()> {
        self.store.recover().await?;
        // One cycle per tick bounds progress reporting; due-time ordering preserves fairness.
        for id in self.store.due_agents().await?.into_iter().take(1) {
            if let Err(e) = self.cycle(id, true).await {
                tracing::warn!(agent_id=%id,error_code=%e,"scheduled_cycle_failed");
            }
        }
        Ok(())
    }
}
