-- Explicit Phase 5 extension. No automatic migration, login, grants or activation.
-- Depends on ecosystem-social, agent-factory and agent-supervised (in that order).
BEGIN;
CREATE SCHEMA agent_bounded;
REVOKE ALL ON SCHEMA agent_bounded FROM PUBLIC;
CREATE TABLE agent_bounded.runs (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), creator_id uuid NOT NULL,
 agent_id uuid NOT NULL UNIQUE REFERENCES public.agents(id), request_id uuid NOT NULL,
 scope jsonb NOT NULL, scope_sha256 text NOT NULL,
 state text NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','approved','active','paused','stopped','exhausted','quarantined')),
 revision integer NOT NULL DEFAULT 0, cycles_started integer NOT NULL DEFAULT 0,
 actions_reserved integer NOT NULL DEFAULT 0, approved_until timestamptz,
 next_cycle_at timestamptz NOT NULL DEFAULT clock_timestamp(), lease_id uuid, lease_until timestamptz,
 task_id uuid REFERENCES agent_supervised.tasks, created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(creator_id,request_id),
 CHECK(scope_sha256=encode(sha256(convert_to(scope::text,'UTF8')),'hex')),
 CHECK(cycles_started BETWEEN 0 AND 10 AND actions_reserved BETWEEN 0 AND 30)
);
CREATE TABLE agent_bounded.commands (
 creator_id uuid NOT NULL, request_id uuid NOT NULL, run_id uuid NOT NULL REFERENCES agent_bounded.runs,
 body jsonb NOT NULL, PRIMARY KEY(creator_id,request_id)
);
CREATE TABLE agent_bounded.cycles (
 task_id uuid PRIMARY KEY REFERENCES agent_supervised.tasks, run_id uuid NOT NULL REFERENCES agent_bounded.runs,
 ordinal integer NOT NULL, state text NOT NULL CHECK(state IN ('running','awaiting_review','accepted','rejected','interrupted')),
 result_sha256 text, proposed_memory jsonb, approved_memory jsonb,
 UNIQUE(run_id,ordinal)
);
CREATE TABLE agent_bounded.events (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, run_id uuid NOT NULL REFERENCES agent_bounded.runs,
 body jsonb NOT NULL, previous_sha256 text NOT NULL, sha256 text NOT NULL,
 CHECK(sha256=encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex'))
);
CREATE TRIGGER bounded_events_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON agent_bounded.events
 FOR EACH STATEMENT EXECUTE FUNCTION ecosystem_social.immutable();
CREATE TRIGGER bounded_commands_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON agent_bounded.commands
 FOR EACH STATEMENT EXECUTE FUNCTION ecosystem_social.immutable();
CREATE FUNCTION agent_bounded.event(rid uuid, operation text, details jsonb DEFAULT '{}') RETURNS void
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE b jsonb; previous text;
BEGIN
 SELECT jsonb_build_object('operation',operation,'at',clock_timestamp(),'revision',revision,
 'state',state,'cycles_started',cycles_started,'actions_reserved',actions_reserved,
 'scope_sha256',scope_sha256,'task_id',task_id,'details',details) INTO b FROM agent_bounded.runs WHERE id=rid;
 SELECT sha256 INTO previous FROM agent_bounded.events WHERE run_id=rid ORDER BY id DESC LIMIT 1;
 previous=coalesce(previous,repeat('0',64));
 INSERT INTO agent_bounded.events(run_id,body,previous_sha256,sha256)
 VALUES(rid,b,previous,encode(sha256(convert_to(previous||b::text,'UTF8')),'hex'));
END;
$$;
CREATE FUNCTION agent_bounded.view_run(r agent_bounded.runs) RETURNS jsonb
LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 SELECT (to_jsonb(r)-'lease_id')||jsonb_build_object('mode','deterministic','spending_microusd',0,
 'cycles',coalesce((SELECT jsonb_agg(to_jsonb(c) ORDER BY ordinal) FROM agent_bounded.cycles c WHERE run_id=r.id),'[]'),
 'events',coalesce((SELECT jsonb_agg(to_jsonb(e) ORDER BY id) FROM agent_bounded.events e WHERE run_id=r.id),'[]'))
$$;
-- Application actor comes from the authenticated server principal, never a browser field.
CREATE FUNCTION agent_bounded.command(actor uuid, operation text, b jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
#variable_conflict use_variable
DECLARE r agent_bounded.runs%ROWTYPE; old agent_bounded.commands%ROWTYPE;
 aid uuid; request uuid; s jsonb; c agent_bounded.cycles%ROWTYPE; t agent_supervised.tasks%ROWTYPE;
BEGIN
 IF actor IS NULL OR actor='00000000-0000-0000-0000-000000000000'::uuid
 OR EXISTS(SELECT 1 FROM public.application_epoch) OR jsonb_typeof(b) IS DISTINCT FROM 'object' THEN
 RAISE EXCEPTION 'bounded_scope_denied' USING ERRCODE='42501'; END IF;
 aid=(b->>'agent_id')::uuid;
 -- Same ACL lock order as supervised commands; revocation fences every commit.
 PERFORM 1 FROM public.agent_access WHERE agent_id=aid AND principal_id=actor AND role='owner' FOR SHARE;
 IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM agent_factory.submissions WHERE agent_id=aid AND creator_id=actor) THEN
 RAISE EXCEPTION 'bounded_owner_denied' USING ERRCODE='42501'; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('bounded:'||aid::text,0));
 SELECT * INTO r FROM agent_bounded.runs WHERE agent_id=aid AND creator_id=actor FOR UPDATE;
 IF operation='get' THEN
 RETURN jsonb_build_object('run',CASE WHEN r.id IS NULL THEN NULL ELSE agent_bounded.view_run(r) END,
 'goals',(SELECT data->'goals' FROM public.agents WHERE id=aid)); END IF;
 request=(b->>'request_id')::uuid;
 IF request IS NULL OR request='00000000-0000-0000-0000-000000000000'::uuid THEN
 RAISE EXCEPTION 'bounded_request_required' USING ERRCODE='22023'; END IF;
 SELECT * INTO old FROM agent_bounded.commands WHERE creator_id=actor AND request_id=request;
 IF FOUND THEN
 IF old.body IS DISTINCT FROM b||jsonb_build_object('operation',operation) OR old.run_id IS DISTINCT FROM r.id THEN
 RAISE EXCEPTION 'bounded_request_conflict' USING ERRCODE='40001'; END IF;
 RETURN jsonb_build_object('run',agent_bounded.view_run(r)); END IF;
 IF operation='create' THEN
 s=b->'scope';
 IF r.id IS NOT NULL OR jsonb_typeof(s) IS DISTINCT FROM 'object'
 OR (SELECT count(*) FROM jsonb_object_keys(s))<>9
 OR NOT s ?& ARRAY['goal_id','priority','input','max_cycles','max_actions','max_spending_microusd','interval_seconds','valid_for_seconds','mode']
 OR s->>'mode' IS DISTINCT FROM 'deterministic' OR s->'max_spending_microusd' IS DISTINCT FROM '0'::jsonb
 OR (s->>'max_cycles')::integer NOT BETWEEN 1 AND 10
 OR (s->>'max_actions')::integer NOT BETWEEN 3 AND 30
 OR (s->>'max_cycles')::integer*3>(s->>'max_actions')::integer
 OR (s->>'priority')::integer NOT BETWEEN 0 AND 5
 OR (s->>'interval_seconds')::integer NOT BETWEEN 5 AND 86400
 OR (s->>'valid_for_seconds')::integer NOT BETWEEN 60 AND 3600
 OR EXISTS(SELECT 1 FROM jsonb_each(s) WHERE value='null'::jsonb)
 OR s->'input'->'synthetic_only' IS DISTINCT FROM 'true'::jsonb
 OR s->'input'->'retain_memory' IS DISTINCT FROM 'false'::jsonb
 OR (SELECT count(*) FROM jsonb_object_keys(s->'input'))<>5
 OR s->'input' ? 'reasoning'
 OR NOT EXISTS(SELECT 1 FROM public.agents a,jsonb_array_elements(a.data->'goals') g
 WHERE a.id=aid AND g->>'id'=s->>'goal_id' AND g->'completed'='false'::jsonb
 AND g->>'description'=s->'input'->>'task') THEN
 RAISE EXCEPTION 'bounded_policy_denied' USING ERRCODE='42501'; END IF;
 IF (SELECT count(*) FROM agent_bounded.runs WHERE creator_id=actor)>=8 THEN
 RAISE EXCEPTION 'bounded_quota' USING ERRCODE='54000'; END IF;
 INSERT INTO agent_bounded.runs(creator_id,agent_id,request_id,scope,scope_sha256)
 VALUES(actor,aid,request,s,encode(sha256(convert_to(s::text,'UTF8')),'hex')) RETURNING * INTO r;
 ELSE
 IF r.id IS NULL OR r.revision IS DISTINCT FROM (b->>'expected_revision')::integer
 OR r.scope_sha256 IS DISTINCT FROM b->>'scope_sha256' THEN
 RAISE EXCEPTION 'bounded_stale' USING ERRCODE='40001'; END IF;
 IF r.revision>=128 AND operation NOT IN ('stop','review') THEN RAISE EXCEPTION 'bounded_command_limit' USING ERRCODE='54000'; END IF;
 IF operation='approve' AND r.state='pending' AND b->'confirmed'='true'::jsonb
 AND clock_timestamp()<r.created_at+interval '10 minutes' THEN
 r.state='approved';r.approved_until=clock_timestamp()+make_interval(secs=>(r.scope->>'valid_for_seconds')::integer);
 ELSIF operation IN ('start','resume') AND b->'confirmed'='true'::jsonb
 AND ((operation='start' AND r.state='approved') OR (operation='resume' AND r.state='paused'))
 AND r.approved_until>clock_timestamp() AND r.cycles_started<(r.scope->>'max_cycles')::integer
 AND r.actions_reserved+3<=(r.scope->>'max_actions')::integer THEN r.state='active';
 ELSIF operation IN ('pause','stop') AND r.state NOT IN ('stopped','exhausted') THEN
 r.state=CASE operation WHEN 'stop' THEN 'stopped' ELSE 'paused' END;
 IF r.task_id IS NOT NULL THEN
 SELECT * INTO t FROM agent_supervised.tasks WHERE id=r.task_id;
 IF t.state IN ('approved','running','paused') THEN
 PERFORM agent_supervised.command(actor,'stop',jsonb_build_object('task_id',t.id,'request_id',gen_random_uuid(),
 'expected_revision',t.revision,'input_sha256',t.input_sha256,'confirmed',false)); END IF;
 UPDATE agent_bounded.cycles SET state='interrupted' WHERE task_id=r.task_id AND state='running';
 END IF;
 r.lease_id=NULL;r.lease_until=NULL;r.task_id=NULL;
 ELSIF operation='review' AND b->'confirmed'='true'::jsonb THEN
 SELECT * INTO c FROM agent_bounded.cycles WHERE run_id=r.id AND task_id=(b->>'task_id')::uuid FOR UPDATE;
 IF c.state IS DISTINCT FROM 'awaiting_review' OR c.result_sha256 IS DISTINCT FROM b->>'result_sha256'
 OR jsonb_typeof(b->'accept') IS DISTINCT FROM 'boolean'
 OR jsonb_typeof(b->'retain_memory') IS DISTINCT FROM 'boolean'
 OR (b->'retain_memory'='true'::jsonb AND (b->'accept'<>'true'::jsonb OR NOT EXISTS(
 SELECT 1 FROM agent_factory.submissions WHERE agent_id=aid AND configuration->'requested_capabilities' ? 'remember'))) THEN
 RAISE EXCEPTION 'bounded_review_denied' USING ERRCODE='42501'; END IF;
 UPDATE agent_bounded.cycles SET state=CASE WHEN b->'accept'='true'::jsonb THEN 'accepted' ELSE 'rejected' END,
 approved_memory=CASE WHEN b->'retain_memory'='true'::jsonb THEN proposed_memory ELSE NULL END WHERE task_id=c.task_id;
 ELSE RAISE EXCEPTION 'bounded_transition_denied' USING ERRCODE='42501'; END IF;
 r.revision=r.revision+1;
 UPDATE agent_bounded.runs SET state=r.state,revision=r.revision,approved_until=r.approved_until,
 lease_id=r.lease_id,lease_until=r.lease_until,task_id=r.task_id WHERE id=r.id;
 END IF;
 INSERT INTO agent_bounded.commands VALUES(actor,request,r.id,b||jsonb_build_object('operation',operation));
 PERFORM agent_bounded.event(r.id,operation,jsonb_build_object('actor',actor,'request_id',request,'task_id',b->'task_id','result_sha256',b->'result_sha256','accept',b->'accept','retain_memory',b->'retain_memory'));
 RETURN jsonb_build_object('run',agent_bounded.view_run(r));
END;
$$;
-- Separate execution-only grant; no credentials, provider choice or caller-supplied task identity.
CREATE FUNCTION agent_bounded.claim() RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
DECLARE r agent_bounded.runs%ROWTYPE; t jsonb; token uuid:=gen_random_uuid();
BEGIN
 IF EXISTS(SELECT 1 FROM public.application_epoch) THEN RAISE EXCEPTION 'bounded_epoch_denied' USING ERRCODE='42501'; END IF;
 -- One database-wide coordinator; skip busy owners rather than queue duplicate work.
 IF NOT pg_try_advisory_xact_lock(hashtextextended('bounded-coordinator',0)) THEN RETURN NULL; END IF;
 SELECT * INTO r FROM agent_bounded.runs WHERE state='active'
 AND (next_cycle_at<=clock_timestamp() OR lease_until<=clock_timestamp())
 ORDER BY (scope->>'priority')::integer DESC,next_cycle_at,id LIMIT 1;
 IF NOT FOUND THEN RETURN NULL; END IF;
 PERFORM 1 FROM public.agent_access WHERE agent_id=r.agent_id AND principal_id=r.creator_id AND role='owner' FOR SHARE;
 IF NOT FOUND THEN
 UPDATE agent_bounded.runs SET state='quarantined',revision=revision+1 WHERE id=r.id;
 PERFORM agent_bounded.event(r.id,'owner_revoked'); RETURN NULL; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('bounded:'||r.agent_id::text,0));
 SELECT * INTO r FROM agent_bounded.runs WHERE id=r.id FOR UPDATE;
 IF r.state<>'active' THEN RETURN NULL; END IF;
 IF r.lease_id IS NOT NULL THEN
 IF r.lease_until>clock_timestamp() THEN RETURN NULL; END IF;
 UPDATE agent_bounded.runs SET state='quarantined',revision=revision+1 WHERE id=r.id;
 UPDATE agent_bounded.cycles SET state='interrupted' WHERE task_id=r.task_id AND state='running';
 PERFORM agent_bounded.event(r.id,'lease_expired_no_replay');RETURN NULL; END IF;
 IF r.approved_until<=clock_timestamp() OR r.cycles_started>=(r.scope->>'max_cycles')::integer
 OR r.actions_reserved+3>(r.scope->>'max_actions')::integer THEN
 UPDATE agent_bounded.runs SET state='exhausted',revision=revision+1 WHERE id=r.id;
 PERFORM agent_bounded.event(r.id,'authorization_exhausted');RETURN NULL; END IF;
 -- Existing task engine validates owner, synthetic material, quotas and immutable input.
 t=agent_supervised.command(r.creator_id,'create',jsonb_build_object('agent_id',r.agent_id,'request_id',token,'input',r.scope->'input'));
 t=agent_supervised.command(r.creator_id,'approve',jsonb_build_object('task_id',t->'id','request_id',gen_random_uuid(),
 'expected_revision',t->'revision','input_sha256',t->'input_sha256','confirmed',true));
 t=agent_supervised.command(r.creator_id,'start',jsonb_build_object('task_id',t->'id','request_id',gen_random_uuid(),
 'expected_revision',t->'revision','input_sha256',t->'input_sha256','confirmed',true));
 UPDATE agent_bounded.runs SET cycles_started=cycles_started+1,actions_reserved=actions_reserved+3,
 lease_id=token,lease_until=least(approved_until,clock_timestamp()+interval '30 seconds'),task_id=(t->>'id')::uuid,
 next_cycle_at=clock_timestamp()+make_interval(secs=>(scope->>'interval_seconds')::integer),revision=revision+1 WHERE id=r.id;
 INSERT INTO agent_bounded.cycles VALUES((t->>'id')::uuid,r.id,r.cycles_started+1,'running',NULL,NULL,NULL);
 PERFORM agent_bounded.event(r.id,'cycle_claimed');
 RETURN jsonb_build_object('run_id',r.id,'actor',r.creator_id,'lease_id',token,'task',t,
 'personality',(SELECT configuration->'personality' FROM agent_factory.submissions WHERE agent_id=r.agent_id),
 'memories',CASE WHEN (SELECT configuration->'requested_capabilities' ? 'read_memory' FROM agent_factory.submissions WHERE agent_id=r.agent_id)
 THEN coalesce((SELECT jsonb_agg(approved_memory) FROM agent_bounded.cycles WHERE run_id=r.id AND approved_memory IS NOT NULL),'[]') ELSE '[]'::jsonb END);
END;
$$;
CREATE FUNCTION agent_bounded.advance(rid uuid, token uuid, expected integer, payload jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
DECLARE r agent_bounded.runs%ROWTYPE; t agent_supervised.tasks%ROWTYPE; v jsonb; proposal jsonb;
BEGIN
 IF EXISTS(SELECT 1 FROM public.application_epoch) THEN RAISE EXCEPTION 'bounded_epoch_denied' USING ERRCODE='42501'; END IF;
 SELECT * INTO r FROM agent_bounded.runs WHERE id=rid;
 PERFORM 1 FROM public.agent_access WHERE agent_id=r.agent_id AND principal_id=r.creator_id AND role='owner' FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'bounded_owner_revoked' USING ERRCODE='42501'; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('bounded:'||r.agent_id::text,0));
 SELECT * INTO r FROM agent_bounded.runs WHERE id=rid FOR UPDATE;
 IF r.state<>'active' OR r.lease_id IS DISTINCT FROM token OR r.lease_until<=clock_timestamp()
 OR r.approved_until<=clock_timestamp() THEN RAISE EXCEPTION 'bounded_lease_denied' USING ERRCODE='42501'; END IF;
 SELECT * INTO t FROM agent_supervised.tasks WHERE id=r.task_id;
 IF t.revision IS DISTINCT FROM expected OR payload ? 'memory' AND payload->'memory'<>'null'::jsonb THEN
 RAISE EXCEPTION 'bounded_result_denied' USING ERRCODE='40001'; END IF;
 PERFORM set_config('supa.bounded_lease',token::text,true);
 v=agent_supervised.command(r.creator_id,'advance',jsonb_build_object('task_id',t.id,'request_id',gen_random_uuid(),
 'expected_revision',t.revision,'input_sha256',t.input_sha256,'confirmed',false,'payload',payload));
 IF v->>'state'='completed' THEN
 proposal=jsonb_build_object('id',gen_random_uuid(),'created_at',clock_timestamp(),'source_event_id',v->'events'->-1->'id','kind','hypothesis','lesson',coalesce(v->'payloads'->1->'report'->'evidence'->0->>'quote','No extractable supplied statement.'),
 'source_task_id',t.id,'source_input_sha256',t.input_sha256,'owner_review_required',true);
 UPDATE agent_bounded.cycles SET state='awaiting_review',result_sha256=encode(sha256(convert_to((v->'payloads')::text,'UTF8')),'hex'),
 proposed_memory=proposal WHERE task_id=t.id;
 UPDATE agent_bounded.runs SET lease_id=NULL,lease_until=NULL,task_id=NULL,revision=revision+1,
 state=CASE WHEN cycles_started>=(scope->>'max_cycles')::integer OR actions_reserved+3>(scope->>'max_actions')::integer THEN 'exhausted' ELSE state END WHERE id=rid;
 PERFORM agent_bounded.event(rid,'cycle_completed_pending_memory_review');
 END IF;
 RETURN v;
END;
$$;
-- A normal task endpoint cannot advance a scheduled task outside its lease.
-- Only the execution-only advance function knows/sets this unexposed lease token.
CREATE FUNCTION agent_bounded.fence_task() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
DECLARE r agent_bounded.runs%ROWTYPE;
BEGIN
 SELECT a.* INTO r FROM agent_bounded.runs a JOIN agent_bounded.cycles c ON c.run_id=a.id WHERE c.task_id=OLD.id;
 IF NOT FOUND THEN RETURN NEW; END IF;
 -- Cancellation through either control plane may only reduce execution authority.
 IF NEW.state='stopped' AND NEW.step=OLD.step AND NEW.payloads=OLD.payloads THEN RETURN NEW; END IF;
 IF r.state<>'active' OR r.task_id IS DISTINCT FROM OLD.id OR r.lease_id IS NULL
 OR current_setting('supa.bounded_lease',true) IS DISTINCT FROM r.lease_id::text
 OR r.lease_until<=clock_timestamp() OR r.approved_until<=clock_timestamp()
 OR NEW.step<>OLD.step+1 OR NEW.input->'retain_memory' IS DISTINCT FROM 'false'::jsonb THEN
 RAISE EXCEPTION 'bounded_task_requires_current_lease' USING ERRCODE='42501'; END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER bounded_task_fence BEFORE UPDATE ON agent_supervised.tasks
 FOR EACH ROW EXECUTE FUNCTION agent_bounded.fence_task();
REVOKE ALL ON ALL TABLES IN SCHEMA agent_bounded FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA agent_bounded FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA agent_bounded FROM PUBLIC;
-- API: USAGE schema + EXECUTE command(uuid,text,jsonb) only.
-- Coordinator: USAGE schema + EXECUTE claim(),advance(uuid,uuid,integer,jsonb) only.
-- No grants on agents, memories, wallet/financial data, or existing scheduler tables.
COMMIT;
