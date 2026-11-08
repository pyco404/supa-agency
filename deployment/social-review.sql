-- Explicit local/disposable extension. Never an automatic migration or live grant.
-- Depends on ecosystem-social.sql. Existing social records remain unchanged.
BEGIN;
CREATE SCHEMA social_review;
REVOKE ALL ON SCHEMA social_review FROM PUBLIC;
-- Provisioned only by a separately authorized migration authority. No credential.
CREATE TABLE social_review.policies (
 agent_id uuid PRIMARY KEY REFERENCES public.agents(id), owner_id uuid NOT NULL UNIQUE,
 account jsonb NOT NULL CHECK(account ?& ARRAY['username','mode','id'] AND jsonb_typeof(account->'id')='string' AND jsonb_typeof(account->'username')='string' AND jsonb_typeof(account->'mode')='string' AND account-ARRAY['username','mode','id']='{}'::jsonb AND account->>'username'='supa_agency'
  AND account->>'mode'='mock' AND account->>'id' ~ '^[0-9]{1,24}$'),
 enabled boolean NOT NULL DEFAULT false,
 daily_cap integer NOT NULL DEFAULT 3 CHECK(daily_cap BETWEEN 1 AND 3)
);
CREATE UNIQUE INDEX one_mock_official_account ON social_review.policies ((account->>'id'));
CREATE TABLE social_review.releases (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, agent_id uuid NOT NULL,
 event_id uuid NOT NULL, actor uuid NOT NULL, snapshot_sha256 text NOT NULL,
 released boolean NOT NULL, created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE social_review.drafts (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), agent_id uuid NOT NULL,
 revision integer NOT NULL DEFAULT 0 CHECK(revision BETWEEN 0 AND 64),
 state text NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','approved','rejected','revision_requested','reserved','posted','failed','uncertain')),
 text text NOT NULL CHECK(octet_length(text) BETWEEN 1 AND 280),
 text_sha256 text NOT NULL, decision jsonb NOT NULL, topics jsonb NOT NULL,
 account jsonb NOT NULL, approval jsonb, receipt jsonb,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK(text_sha256=encode(sha256(convert_to(text,'UTF8')),'hex'))
);
CREATE TABLE social_review.commands (
 request_id uuid PRIMARY KEY, agent_id uuid NOT NULL, actor uuid NOT NULL,
 body jsonb NOT NULL, result jsonb NOT NULL
);
CREATE TABLE social_review.events (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, agent_id uuid NOT NULL, actor uuid NOT NULL,
 body jsonb NOT NULL, previous_sha256 text NOT NULL, sha256 text NOT NULL,
 CHECK(sha256=encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex'))
);
CREATE TABLE social_review.attempts (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), agent_id uuid NOT NULL, draft_id uuid NOT NULL UNIQUE REFERENCES social_review.drafts,
 account_id text NOT NULL, normalized_text text NOT NULL, state text NOT NULL DEFAULT 'reserved',
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), UNIQUE(account_id,normalized_text)
);
CREATE TABLE social_review.pauses (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, agent_id uuid NOT NULL, paused boolean NOT NULL DEFAULT true
);
REVOKE ALL ON ALL TABLES IN SCHEMA social_review FROM PUBLIC;
CREATE FUNCTION social_review.context(a uuid) RETURNS jsonb LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 SELECT jsonb_build_object('publishing_enabled',false,'account',p.account,'paused',coalesce((SELECT paused FROM social_review.pauses WHERE agent_id=a ORDER BY id DESC LIMIT 1),true),
 'sources',coalesce((SELECT jsonb_agg(s ORDER BY s->'event'->>'created_at' DESC) FROM (
  SELECT jsonb_build_object('event',e.snapshot,'snapshot_sha256',encode(sha256(convert_to(e.snapshot::text,'UTF8')),'hex'),
   'released',coalesce((SELECT r.released AND r.snapshot_sha256=encode(sha256(convert_to(e.snapshot::text,'UTF8')),'hex') FROM social_review.releases r WHERE r.agent_id=a AND r.event_id=e.id ORDER BY r.id DESC LIMIT 1),false)) s
  FROM (SELECT id,jsonb_build_object('id',id,'cycle_id',cycle_id,'kind',kind,'code',code,'simulated',simulated,'created_at',created_at) snapshot FROM public.activity
   WHERE agent_id=a AND created_at>clock_timestamp()-interval '7 days' AND created_at<=clock_timestamp()
   AND ((kind='cycle' AND code IN ('completed','interrupted','provider_failed','provider_timeout','provider_transport_failed','invalid_input','insufficient_simulated_balance','live_budget_exhausted','resource_budget_exhausted'))
     OR (kind='goal' AND code IN ('added','evaluated')) OR (kind='simulated_ledger' AND simulated AND code IN ('seed','revenue','inference','operating')))
   ORDER BY created_at DESC,id LIMIT 32) e) q),'[]'),
 'drafts',coalesce((SELECT jsonb_agg(to_jsonb(d) ORDER BY d.updated_at DESC,d.id) FROM (SELECT * FROM social_review.drafts WHERE agent_id=a ORDER BY updated_at DESC,id LIMIT 100) d),'[]'),
 'audit',coalesce((SELECT jsonb_agg(to_jsonb(e) ORDER BY e.id) FROM (SELECT * FROM social_review.events WHERE agent_id=a ORDER BY id DESC LIMIT 500) e),'[]'))
 FROM social_review.policies p WHERE p.agent_id=a
$$;
REVOKE ALL ON FUNCTION social_review.context(uuid) FROM PUBLIC;
CREATE FUNCTION social_review.command(actor uuid, op text, b jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
#variable_conflict use_variable
DECLARE p social_review.policies%ROWTYPE; d social_review.drafts%ROWTYPE;
 c jsonb; client jsonb:=b->'client'; proposal jsonb:=b->'proposal'; old social_review.commands%ROWTYPE;
 request uuid; chosen uuid; s jsonb; previous text; event_body jsonb; answer jsonb; now_at timestamptz:=clock_timestamp();
BEGIN
 SET LOCAL statement_timeout='2000ms'; SET LOCAL lock_timeout='1000ms';
 IF actor IS NULL OR EXISTS(SELECT 1 FROM public.application_epoch) THEN RAISE EXCEPTION 'social_scope_denied' USING ERRCODE='42501'; END IF;
 SELECT * INTO p FROM social_review.policies WHERE owner_id=actor AND enabled FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'social_owner_denied' USING ERRCODE='42501'; END IF;
 PERFORM ecosystem_social.lock_agent(p.agent_id);
 PERFORM 1 FROM public.agent_access WHERE agent_id=p.agent_id AND principal_id=actor AND role='owner' FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'social_owner_revoked' USING ERRCODE='42501'; END IF;
 c=social_review.context(p.agent_id);
 IF op='dispatch_guard' THEN
  SELECT * INTO d FROM social_review.drafts WHERE id=(b->>'draft_id')::uuid AND agent_id=p.agent_id;
  IF NOT FOUND OR d.state<>'reserved' OR (c->>'paused')::boolean IS DISTINCT FROM false
   OR d.text_sha256 IS DISTINCT FROM b->>'text_sha256' OR d.account IS DISTINCT FROM b->'account'
   OR d.approval->'account' IS DISTINCT FROM p.account OR jsonb_typeof(d.approval->'expires_at') IS DISTINCT FROM 'string'
   OR (d.approval->>'expires_at')::timestamptz<=now_at
   OR EXISTS(SELECT 1 FROM jsonb_array_elements_text(d.decision->'event_ids') ref
      WHERE NOT EXISTS(SELECT 1 FROM jsonb_array_elements(c->'sources') src WHERE src->'event'->>'id'=ref AND src->'released'='true'::jsonb))
  THEN RAISE EXCEPTION 'social_dispatch_denied' USING ERRCODE='42501'; END IF;
  RETURN 'true'::jsonb;
 END IF;
 IF op='read' THEN RETURN jsonb_build_object('context',c,'context_sha256',encode(sha256(convert_to(c::text,'UTF8')),'hex')); END IF;
 request=(client->>'request_id')::uuid;
 IF request IS NULL OR request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION 'social_request_required'; END IF;
 SELECT * INTO old FROM social_review.commands WHERE request_id=request;
 IF FOUND THEN
  IF old.actor<>actor OR old.agent_id<>p.agent_id OR old.body<>client THEN RAISE EXCEPTION 'social_request_conflict' USING ERRCODE='40001'; END IF;
  RETURN old.result;
 END IF;
 IF op='replay' THEN RETURN 'null'::jsonb; END IF;
 IF op<>'apply' OR b->>'context_sha256' IS DISTINCT FROM encode(sha256(convert_to(c::text,'UTF8')),'hex') THEN RAISE EXCEPTION 'social_stale_context' USING ERRCODE='40001'; END IF;
 IF jsonb_array_length(c->'audit')>=500 AND coalesce(client->>'operation','') NOT IN ('pause','release') THEN RAISE EXCEPTION 'social_quota' USING ERRCODE='54000'; END IF;
 IF client->>'operation'='release' THEN
  SELECT value INTO s FROM jsonb_array_elements(c->'sources') WHERE value->'event'->>'id'=client->>'event_id';
  IF s IS NULL OR jsonb_typeof(client->'released') IS DISTINCT FROM 'boolean' OR client->'confirmed' IS DISTINCT FROM 'true'::jsonb THEN RAISE EXCEPTION 'social_release_required'; END IF;
  INSERT INTO social_review.releases(agent_id,event_id,actor,snapshot_sha256,released) VALUES(p.agent_id,(client->>'event_id')::uuid,actor,s->>'snapshot_sha256',(client->>'released')::boolean);
  -- Any disclosure change invalidates earlier exact-text approvals.
  UPDATE social_review.drafts SET approval=NULL,state='pending',revision=revision+1,updated_at=now_at WHERE agent_id=p.agent_id AND state='approved';
 ELSIF client->>'operation'='pause' THEN
  IF jsonb_typeof(client->'paused') IS DISTINCT FROM 'boolean' OR client->'confirmed' IS DISTINCT FROM 'true'::jsonb THEN RAISE EXCEPTION 'social_pause_confirmation'; END IF;
  INSERT INTO social_review.pauses(agent_id,paused) VALUES(p.agent_id,(client->>'paused')::boolean);
  UPDATE social_review.drafts SET approval=NULL,state='pending',revision=revision+1,updated_at=now_at WHERE agent_id=p.agent_id AND state='approved';
 ELSIF client->>'operation'='create' THEN
  IF (SELECT count(*) FROM social_review.drafts WHERE agent_id=p.agent_id)>=100 THEN RAISE EXCEPTION 'social_quota'; END IF;
  INSERT INTO social_review.drafts(agent_id,text,text_sha256,decision,topics,account) VALUES(p.agent_id,proposal->>'text',encode(sha256(convert_to(proposal->>'text','UTF8')),'hex'),proposal->'decision',proposal->'topics',p.account) RETURNING * INTO d;
 ELSE
  SELECT * INTO d FROM social_review.drafts WHERE id=(client->>'draft_id')::uuid AND agent_id=p.agent_id FOR UPDATE;
  IF NOT FOUND OR d.revision IS DISTINCT FROM (client->>'expected_revision')::integer OR d.revision>=63 THEN RAISE EXCEPTION 'social_stale_draft' USING ERRCODE='40001'; END IF;
  IF client->>'operation' IN ('edit','revise') AND d.state IN ('pending','approved','revision_requested','rejected') THEN
   d.text=proposal->>'text';d.text_sha256=encode(sha256(convert_to(d.text,'UTF8')),'hex');d.decision=proposal->'decision';d.topics=proposal->'topics';d.approval=NULL;d.state='pending';
  ELSIF client->>'operation'='approve' AND d.state='pending' AND client->'confirmed'='true'::jsonb AND NOT (c->>'paused')::boolean THEN
   IF client->>'text_sha256' IS DISTINCT FROM d.text_sha256 OR client->'account' IS DISTINCT FROM p.account OR d.account IS DISTINCT FROM p.account OR jsonb_typeof(client->'expires_at') IS DISTINCT FROM 'string'
      OR (client->>'expires_at')::timestamptz<=now_at OR (client->>'expires_at')::timestamptz>now_at+interval '10 minutes' THEN RAISE EXCEPTION 'social_exact_approval_required' USING ERRCODE='42501'; END IF;
   d.state='approved';d.approval=jsonb_build_object('actor',actor,'text_sha256',d.text_sha256,'account',p.account,'expires_at',client->>'expires_at','approved_at',now_at);
  ELSIF client->>'operation' IN ('reject','request_revision') AND d.state IN ('pending','approved') THEN
   d.state=CASE WHEN client->>'operation'='reject' THEN 'rejected' ELSE 'revision_requested' END;d.approval=NULL;
  ELSIF client->>'operation'='reserve_mock' AND d.state='approved' THEN
   IF (c->>'paused')::boolean IS DISTINCT FROM false OR d.account IS DISTINCT FROM p.account OR d.approval->'account' IS DISTINCT FROM p.account OR d.approval->>'text_sha256' IS DISTINCT FROM d.text_sha256 OR jsonb_typeof(d.approval->'expires_at') IS DISTINCT FROM 'string'
      OR (d.approval->>'expires_at')::timestamptz<=now_at THEN RAISE EXCEPTION 'social_dispatch_denied' USING ERRCODE='42501'; END IF;
   IF (SELECT count(*) FROM social_review.attempts WHERE account_id=p.account->>'id' AND created_at>now_at-interval '24 hours')>=p.daily_cap
    OR EXISTS(SELECT 1 FROM social_review.attempts WHERE account_id=p.account->>'id' AND created_at>now_at-interval '15 minutes') THEN RAISE EXCEPTION 'social_rate_limited' USING ERRCODE='54000'; END IF;
   INSERT INTO social_review.attempts(agent_id,draft_id,account_id,normalized_text) VALUES(p.agent_id,d.id,p.account->>'id',lower(regexp_replace(d.text,'[^[:alnum:]]','','g')));
   d.state='reserved';
  ELSIF client->>'operation'='settle_mock' AND d.state='reserved' THEN
   IF coalesce(proposal->>'state','') NOT IN ('posted','failed','uncertain') OR proposal->>'mode' IS DISTINCT FROM 'mock' THEN RAISE EXCEPTION 'social_receipt_invalid'; END IF;
   d.state=proposal->>'state';d.receipt=proposal;
   UPDATE social_review.attempts SET state=d.state WHERE draft_id=d.id;
  ELSE RAISE EXCEPTION 'social_transition_denied' USING ERRCODE='42501'; END IF;
  d.revision=d.revision+1;d.updated_at=now_at;
  UPDATE social_review.drafts SET text=d.text,text_sha256=d.text_sha256,decision=d.decision,topics=d.topics,approval=d.approval,state=d.state,receipt=d.receipt,revision=d.revision,updated_at=now_at WHERE id=d.id;
 END IF;
 SELECT sha256 INTO previous FROM social_review.events WHERE agent_id=p.agent_id ORDER BY id DESC LIMIT 1;
 previous=coalesce(previous,repeat('0',64));
 event_body=jsonb_build_object('operation',client->>'operation','request_id',request,'draft_id',d.id,'state',d.state,'text_sha256',d.text_sha256,'text',d.text,'account',d.account,'claims',d.decision->'claims','actor',actor,'recorded_at',now_at,'context_before_sha256',b->>'context_sha256');
 INSERT INTO social_review.events(agent_id,actor,body,previous_sha256,sha256) VALUES(p.agent_id,actor,event_body,previous,encode(sha256(convert_to(previous||event_body::text,'UTF8')),'hex'));
 answer=jsonb_build_object('context',social_review.context(p.agent_id));
 INSERT INTO social_review.commands(request_id,agent_id,actor,body,result) VALUES(request,p.agent_id,actor,client,answer);
 RETURN answer;
END;
$$;
REVOKE ALL ON FUNCTION social_review.command(uuid,text,jsonb) FROM PUBLIC;
-- Append-only evidence and replay receipts, independent of HTTP handlers.
CREATE TRIGGER releases_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON social_review.releases FOR EACH STATEMENT EXECUTE FUNCTION ecosystem_social.immutable();
CREATE TRIGGER commands_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON social_review.commands FOR EACH STATEMENT EXECUTE FUNCTION ecosystem_social.immutable();
CREATE TRIGGER events_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON social_review.events FOR EACH STATEMENT EXECUTE FUNCTION ecosystem_social.immutable();
COMMIT;
