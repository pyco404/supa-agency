-- Explicit additive extension; no automatic migration, policy enrollment or grant.
-- Live dispatch additionally requires root-installed owner activation and keyring custody.
BEGIN;
CREATE SCHEMA social_autonomy;
REVOKE ALL ON SCHEMA social_autonomy FROM PUBLIC;
CREATE TABLE social_autonomy.grants (
 id uuid PRIMARY KEY, agent_id uuid NOT NULL REFERENCES public.agents, owner_id uuid NOT NULL,
 account jsonb NOT NULL, grant_body jsonb NOT NULL, authorization_sha256 text NOT NULL,
 state text NOT NULL DEFAULT 'paused' CHECK(state IN ('active','paused','stopped','revoked')),
 expires_at timestamptz NOT NULL, next_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 max_posts integer NOT NULL CHECK(max_posts BETWEEN 1 AND 6),
 interval_seconds integer NOT NULL CHECK(interval_seconds BETWEEN 14400 AND 86400),
 budget bigint NOT NULL CHECK(budget BETWEEN 25000 AND 10000000),
 reserved bigint NOT NULL DEFAULT 0 CHECK(reserved>=0 AND reserved<=budget),
 CHECK(account->>'mode' IN ('mock','live') AND account->>'username'='supa_agency'),
 CHECK(authorization_sha256=encode(sha256(convert_to(grant_body::text,'UTF8')),'hex'))
);
CREATE TABLE social_autonomy.attempts (
 id uuid PRIMARY KEY, grant_id uuid NOT NULL REFERENCES social_autonomy.grants,
 account_id text NOT NULL, text text NOT NULL CHECK(octet_length(text) BETWEEN 1 AND 280),
 text_sha256 text NOT NULL, decision jsonb NOT NULL, topics jsonb NOT NULL,
 state text NOT NULL DEFAULT 'reserved' CHECK(state IN ('reserved','in_flight','posted','failed','uncertain')),
 receipt jsonb, reserved_microusd bigint NOT NULL DEFAULT 25000 CHECK(reserved_microusd=25000),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(account_id,text_sha256), CHECK(text_sha256=encode(sha256(convert_to(text,'UTF8')),'hex'))
);
CREATE INDEX autonomy_account_history ON social_autonomy.attempts(account_id,created_at);
CREATE UNIQUE INDEX autonomy_normalized_text ON social_autonomy.attempts(account_id,(lower(regexp_replace(text,'[^[:alnum:]]','','g'))));
CREATE TABLE social_autonomy.events (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, agent_id uuid NOT NULL, actor uuid NOT NULL,
 body jsonb NOT NULL, previous_sha256 text NOT NULL, sha256 text NOT NULL,
 CHECK(sha256=encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex'))
);
REVOKE ALL ON ALL TABLES IN SCHEMA social_autonomy FROM PUBLIC;
CREATE FUNCTION social_autonomy.policy_mode(actor uuid) RETURNS text
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS $$
 SELECT account->>'mode' FROM social_review.policies WHERE owner_id=actor AND enabled
$$;
REVOKE ALL ON FUNCTION social_autonomy.policy_mode(uuid) FROM PUBLIC;
CREATE FUNCTION social_autonomy.audit(actor uuid, agent uuid, operation text, details jsonb) RETURNS void
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE previous text; body jsonb;
BEGIN
 SELECT sha256 INTO previous FROM social_autonomy.events WHERE agent_id=agent ORDER BY id DESC LIMIT 1;
 previous=coalesce(previous,repeat('0',64));
 body=jsonb_build_object('operation',operation,'actor',actor,'details',details,'recorded_at',clock_timestamp(),'mode',(SELECT account->>'mode' FROM social_review.policies WHERE agent_id=agent));
 INSERT INTO social_autonomy.events(agent_id,actor,body,previous_sha256,sha256)
 VALUES(agent,actor,body,previous,encode(sha256(convert_to(previous||body::text,'UTF8')),'hex'));
END $$;
REVOKE ALL ON FUNCTION social_autonomy.audit(uuid,uuid,text,jsonb) FROM PUBLIC;
CREATE FUNCTION social_autonomy.command(actor uuid, op text, b jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
#variable_conflict use_variable
DECLARE p social_review.policies%ROWTYPE; g social_autonomy.grants%ROWTYPE; a social_autonomy.attempts%ROWTYPE;
 c jsonb; answer jsonb; r jsonb; now_at timestamptz:=clock_timestamp(); chosen uuid;
 latest timestamptz; count_recent integer; reset_at timestamptz; all_history jsonb;
BEGIN
 SET LOCAL statement_timeout='2000ms'; SET LOCAL lock_timeout='1000ms';
 IF actor IS NULL OR actor='00000000-0000-0000-0000-000000000000'::uuid OR EXISTS(SELECT 1 FROM public.application_epoch)
 THEN RAISE EXCEPTION 'autonomy_denied' USING ERRCODE='42501'; END IF;
 SELECT * INTO p FROM social_review.policies WHERE owner_id=actor AND enabled FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'owner_denied' USING ERRCODE='42501'; END IF;
 PERFORM ecosystem_social.lock_agent(p.agent_id);
 PERFORM 1 FROM public.agent_access WHERE agent_id=p.agent_id AND principal_id=actor AND role='owner' FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'owner_revoked' USING ERRCODE='42501'; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('supa-x-account:'||(p.account->>'id'),0));
 chosen=(b->>'grant_id')::uuid;
 SELECT * INTO g FROM social_autonomy.grants WHERE id=chosen FOR UPDATE;
 IF op='authorize' THEN
  IF FOUND THEN
   IF g.owner_id<>actor OR g.grant_body IS DISTINCT FROM b THEN RAISE EXCEPTION 'grant_conflict' USING ERRCODE='40001'; END IF;
   RETURN to_jsonb(g);
  END IF;
  IF b-ARRAY['grant_id','account','confirmed','scope','expires_at','max_posts','interval_seconds','budget_microusd','activation_receipt_id','owner_proof_sha256']<>'{}'::jsonb
   OR chosen IS NULL OR chosen='00000000-0000-0000-0000-000000000000'::uuid OR b->'confirmed' IS DISTINCT FROM 'true'::jsonb
   OR b->>'scope' IS DISTINCT FROM 'original_text_released_metadata_only' OR b->'account' IS DISTINCT FROM p.account
   OR coalesce(b->'account'->>'mode','') NOT IN ('mock','live') OR (b->>'expires_at')::timestamptz<=now_at
   OR (b->>'expires_at')::timestamptz>now_at+interval '7 days'
   OR (p.account->>'mode'='live' AND ((b->>'activation_receipt_id') IS NULL OR (b->>'activation_receipt_id')::uuid='00000000-0000-0000-0000-000000000000'::uuid
      OR coalesce(b->>'owner_proof_sha256','') !~ '^[0-9a-f]{64}$'))
   OR (p.account->>'mode'='mock' AND b ?| ARRAY['activation_receipt_id','owner_proof_sha256'])
  THEN RAISE EXCEPTION 'grant_scope_denied' USING ERRCODE='42501'; END IF;
  INSERT INTO social_autonomy.grants(id,agent_id,owner_id,account,grant_body,authorization_sha256,expires_at,max_posts,interval_seconds,budget)
  VALUES(chosen,p.agent_id,actor,p.account,b,encode(sha256(convert_to(b::text,'UTF8')),'hex'),(b->>'expires_at')::timestamptz,
   (b->>'max_posts')::integer,(b->>'interval_seconds')::integer,(b->>'budget_microusd')::bigint) RETURNING * INTO g;
  PERFORM social_autonomy.audit(actor,p.agent_id,op,jsonb_build_object('grant_id',chosen,'authorization_sha256',g.authorization_sha256));
  RETURN to_jsonb(g);
 END IF;
 IF g.id IS NULL OR g.owner_id<>actor OR g.agent_id<>p.agent_id OR g.account IS DISTINCT FROM p.account
 THEN RAISE EXCEPTION 'grant_denied' USING ERRCODE='42501'; END IF;
 IF op IN ('pause','resume','stop','revoke') THEN
  IF op='resume' AND (EXISTS(SELECT 1 FROM social_autonomy.attempts h WHERE h.account_id=p.account->>'id' AND h.state IN ('in_flight','uncertain')) OR EXISTS(SELECT 1 FROM social_review.attempts h WHERE h.account_id=p.account->>'id' AND h.state='uncertain')) THEN RAISE EXCEPTION 'unconfirmed_account' USING ERRCODE='42501'; END IF;
  IF op='resume' AND (g.state NOT IN ('paused','active') OR g.expires_at<=now_at) THEN RAISE EXCEPTION 'resume_denied' USING ERRCODE='42501'; END IF;
  IF g.state IN ('stopped','revoked') AND op NOT IN ('stop','revoke') THEN RAISE EXCEPTION 'terminal_grant' USING ERRCODE='42501'; END IF;
  UPDATE social_autonomy.grants SET state=CASE op WHEN 'resume' THEN 'active' WHEN 'pause' THEN 'paused' WHEN 'stop' THEN 'stopped' ELSE 'revoked' END,
   next_at=greatest(next_at,now_at) WHERE id=chosen RETURNING * INTO g;
  PERFORM social_autonomy.audit(actor,p.agent_id,op,jsonb_build_object('grant_id',chosen,'state',g.state));
  RETURN to_jsonb(g);
 END IF;
 IF op='recover' THEN
  UPDATE social_autonomy.attempts SET state='uncertain',receipt=jsonb_build_object('mode',g.account->>'mode','state','uncertain','category','restart_unconfirmed_no_retry')
   WHERE grant_id=chosen AND state IN ('reserved','in_flight');
  IF FOUND THEN UPDATE social_autonomy.grants SET state=CASE WHEN state='active' THEN 'paused' ELSE state END WHERE id=chosen; END IF;
  PERFORM social_autonomy.audit(actor,p.agent_id,op,jsonb_build_object('grant_id',chosen));
 END IF;
 c=social_review.context(p.agent_id);
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',hist.id,'state',hist.state,'text',hist.text,'decision',hist.decision,'topics',hist.topics) ORDER BY hist.created_at DESC),'[]')
 INTO all_history FROM social_autonomy.attempts hist WHERE hist.account_id=p.account->>'id';
 -- Keep the existing public-release projection; never add private prose or raw memories.
 c=jsonb_set(c,'{drafts}',all_history || (c->'drafts'));
 SELECT max(created_at),count(*) FILTER(WHERE created_at>now_at-interval '24 hours') INTO latest,count_recent FROM (
  SELECT created_at FROM social_autonomy.attempts WHERE account_id=p.account->>'id'
  UNION ALL SELECT created_at FROM social_review.attempts WHERE account_id=p.account->>'id') q;
 SELECT max(to_timestamp((receipt->>'rate_limit_reset')::double precision)) INTO reset_at
 FROM social_autonomy.attempts WHERE account_id=p.account->>'id' AND receipt->>'http_status'='429';
 IF op IN ('read','recover') THEN
  SELECT * INTO g FROM social_autonomy.grants WHERE id=chosen;
  RETURN jsonb_build_object('mode',g.account->>'mode','publishing_enabled',false,'grant',to_jsonb(g),'context',c,'context_sha256',encode(sha256(convert_to(c::text,'UTF8')),'hex'),
   'due',(g.account->>'mode'<>'live' OR (c->>'paused')::boolean IS NOT DISTINCT FROM false) AND g.state='active' AND g.expires_at>now_at AND g.next_at<=now_at
    AND g.reserved+25000<=g.budget AND count_recent<g.max_posts
    AND (latest IS NULL OR latest<=now_at-make_interval(secs=>g.interval_seconds+8))
    AND (reset_at IS NULL OR reset_at<=now_at)
    AND NOT EXISTS(SELECT 1 FROM social_autonomy.attempts h WHERE h.account_id=p.account->>'id' AND h.state IN ('reserved','in_flight','uncertain'))
    AND NOT EXISTS(SELECT 1 FROM social_review.attempts h WHERE h.account_id=p.account->>'id' AND h.state IN ('reserved','uncertain')),
   'attempts',coalesce((SELECT jsonb_agg(to_jsonb(hist) ORDER BY hist.created_at) FROM social_autonomy.attempts hist WHERE grant_id=chosen),'[]'),
   'audit',coalesce((SELECT jsonb_agg(to_jsonb(e) ORDER BY id) FROM social_autonomy.events e WHERE agent_id=p.agent_id),'[]'));
 END IF;
 SELECT * INTO a FROM social_autonomy.attempts WHERE id=(b->>'attempt_id')::uuid AND grant_id=chosen FOR UPDATE;
 IF op='finish' THEN
  r=b->'receipt';
  IF a.id IS NULL OR a.state NOT IN ('reserved','in_flight') OR r->>'mode' IS DISTINCT FROM g.account->>'mode'
   OR r->>'state' NOT IN ('posted','failed','uncertain') OR (r->>'state'='posted' AND a.state<>'in_flight')
  THEN RAISE EXCEPTION 'receipt_denied' USING ERRCODE='42501'; END IF;
  UPDATE social_autonomy.attempts SET state=r->>'state',receipt=r WHERE id=a.id;
  IF r->>'http_status'='429' AND (r->>'rate_limit_reset' IS NULL OR (r->>'rate_limit_reset')::double precision<=extract(epoch from now_at)) THEN
   r=jsonb_set(r,'{rate_limit_reset}',to_jsonb(extract(epoch from now_at+interval '24 hours')::bigint));
   UPDATE social_autonomy.attempts SET receipt=r WHERE id=a.id;
  END IF;
  IF r->>'state'='uncertain' OR r->>'category'='account_mismatch' OR r->>'http_status' IN ('401','402','403') THEN UPDATE social_autonomy.grants SET state=CASE WHEN state='active' THEN 'paused' ELSE state END WHERE id=chosen; END IF;
  PERFORM social_autonomy.audit(actor,p.agent_id,op,jsonb_build_object('attempt_id',a.id,'receipt',r));
  RETURN r;
 END IF;
 IF g.state<>'active' OR g.expires_at<=now_at THEN RAISE EXCEPTION 'authorization_inactive' USING ERRCODE='42501'; END IF;
 IF op='guard' THEN
  IF g.account->>'mode'='live' AND (c->>'paused')::boolean IS DISTINCT FROM false THEN RAISE EXCEPTION 'social_emergency_pause' USING ERRCODE='42501'; END IF;
  IF a.id IS NULL OR a.state<>'reserved' OR a.text_sha256 IS DISTINCT FROM b->>'text_sha256' OR b->'account' IS DISTINCT FROM g.account
   OR EXISTS(SELECT 1 FROM jsonb_array_elements_text(a.decision->'event_ids') ref
    WHERE NOT EXISTS(SELECT 1 FROM jsonb_array_elements(c->'sources') src WHERE src->'event'->>'id'=ref AND src->'released'='true'::jsonb))
  THEN RAISE EXCEPTION 'dispatch_denied' USING ERRCODE='42501'; END IF;
  UPDATE social_autonomy.attempts SET state='in_flight' WHERE id=a.id;
  PERFORM social_autonomy.audit(actor,p.agent_id,op,jsonb_build_object('attempt_id',a.id,'text_sha256',a.text_sha256));
  RETURN 'true';
 END IF;
 IF op='abstain' THEN
  UPDATE social_autonomy.grants SET next_at=now_at+make_interval(secs=>g.interval_seconds+8) WHERE id=chosen;
  PERFORM social_autonomy.audit(actor,p.agent_id,op,jsonb_build_object('grant_id',chosen,'reason','nothing_new_or_safe'));
  RETURN 'true';
 END IF;
 IF op<>'reserve' THEN RAISE EXCEPTION 'operation_denied' USING ERRCODE='42501'; END IF;
 IF a.id IS NOT NULL THEN RETURN jsonb_build_object('fresh',false,'attempt',to_jsonb(a)); END IF;
 IF EXISTS(SELECT 1 FROM social_autonomy.attempts h WHERE h.account_id=p.account->>'id' AND h.state IN ('reserved','in_flight','uncertain'))
  OR EXISTS(SELECT 1 FROM social_review.attempts h WHERE h.account_id=p.account->>'id' AND h.state IN ('reserved','uncertain'))
  OR g.next_at>now_at OR g.reserved+25000>g.budget OR b->>'context_sha256' IS DISTINCT FROM encode(sha256(convert_to(c::text,'UTF8')),'hex')
 THEN RAISE EXCEPTION 'not_due_or_budget_or_stale' USING ERRCODE='42501'; END IF;
 -- Count every reserved/failed/uncertain request conservatively, across all grants and manual attempts.
 IF count_recent>=g.max_posts OR latest>now_at-make_interval(secs=>g.interval_seconds+8) OR reset_at>now_at
 THEN RAISE EXCEPTION 'posting_cap_or_interval_or_rate_limit' USING ERRCODE='42501'; END IF;
 r=b->'proposal';
 IF EXISTS(SELECT 1 FROM social_review.attempts hist WHERE hist.account_id=p.account->>'id' AND hist.normalized_text=lower(regexp_replace(r->>'text','[^[:alnum:]]','','g')))
  OR EXISTS(SELECT 1 FROM social_autonomy.attempts hist WHERE hist.account_id=p.account->>'id' AND hist.decision->'event_ids' ?| ARRAY(SELECT jsonb_array_elements_text(r->'decision'->'event_ids')))
  OR EXISTS(SELECT 1 FROM social_review.drafts hist WHERE hist.account=p.account AND hist.state IN ('reserved','posted','uncertain','failed') AND hist.decision->'event_ids' ?| ARRAY(SELECT jsonb_array_elements_text(r->'decision'->'event_ids')))
  OR r->>'text' IS NULL OR r->>'text_sha256'<>encode(sha256(convert_to(r->>'text','UTF8')),'hex')
  OR jsonb_array_length(r->'decision'->'event_ids')<1
  OR EXISTS(SELECT 1 FROM jsonb_array_elements_text(r->'decision'->'event_ids') ref
   WHERE NOT EXISTS(SELECT 1 FROM jsonb_array_elements(c->'sources') src WHERE src->'event'->>'id'=ref AND src->'released'='true'::jsonb))
 THEN RAISE EXCEPTION 'unreleased_evidence' USING ERRCODE='42501'; END IF;
 INSERT INTO social_autonomy.attempts(id,grant_id,account_id,text,text_sha256,decision,topics)
 VALUES((b->>'attempt_id')::uuid,chosen,p.account->>'id',r->>'text',r->>'text_sha256',r->'decision',r->'topics') RETURNING * INTO a;
 UPDATE social_autonomy.grants SET reserved=reserved+25000,next_at=now_at+make_interval(secs=>g.interval_seconds+8) WHERE id=chosen;
 PERFORM social_autonomy.audit(actor,p.agent_id,op,jsonb_build_object('attempt_id',a.id,'text_sha256',a.text_sha256,'reservation_microusd',25000));
 RETURN jsonb_build_object('fresh',true,'attempt',to_jsonb(a));
END $$;
REVOKE ALL ON FUNCTION social_autonomy.command(uuid,text,jsonb) FROM PUBLIC;
CREATE TRIGGER events_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON social_autonomy.events FOR EACH STATEMENT EXECUTE FUNCTION ecosystem_social.immutable();
CREATE FUNCTION social_autonomy.manual_fence() RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog AS $$
DECLARE recent integer; latest timestamptz; rate_reset timestamptz; now_at timestamptz:=clock_timestamp();
BEGIN
 PERFORM pg_advisory_xact_lock(hashtextextended('supa-x-account:'||NEW.account_id,0));
 SELECT count(*) FILTER(WHERE created_at>now_at-interval '24 hours'),max(created_at) INTO recent,latest FROM (
  SELECT created_at FROM social_autonomy.attempts WHERE account_id=NEW.account_id
  UNION ALL SELECT created_at FROM social_review.attempts WHERE account_id=NEW.account_id) q;
 SELECT max(to_timestamp((receipt->>'rate_limit_reset')::double precision)) INTO rate_reset FROM social_autonomy.attempts WHERE account_id=NEW.account_id AND receipt->>'http_status'='429';
 IF EXISTS(SELECT 1 FROM social_autonomy.attempts h WHERE h.account_id=NEW.account_id AND h.state IN ('reserved','in_flight','uncertain'))
  OR recent>=6 OR latest>now_at-interval '4 hours 8 seconds' OR rate_reset>now_at
  OR EXISTS(SELECT 1 FROM social_autonomy.attempts hist WHERE hist.account_id=NEW.account_id AND lower(regexp_replace(hist.text,'[^[:alnum:]]','','g'))=NEW.normalized_text) THEN RAISE EXCEPTION 'shared_account_cap' USING ERRCODE='42501'; END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION social_autonomy.manual_fence() FROM PUBLIC;
CREATE TRIGGER autonomous_account_fence BEFORE INSERT ON social_review.attempts FOR EACH ROW EXECUTE FUNCTION social_autonomy.manual_fence();
COMMIT;
