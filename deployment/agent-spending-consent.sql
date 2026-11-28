-- Explicit additive upgrade after agent-reasoning.sql; never auto-installed.
-- Install/grant only after separate database release approval.
BEGIN;
CREATE TABLE agent_supervised.spending_consents (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), task_id uuid NOT NULL UNIQUE REFERENCES agent_supervised.tasks,
 creator_id uuid NOT NULL, request_id uuid NOT NULL, budget_id uuid NOT NULL UNIQUE,
 receipt jsonb NOT NULL, receipt_sha256 text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), expires_at timestamptz NOT NULL,
 revoked_at timestamptz, reservation_id uuid UNIQUE,
 UNIQUE(creator_id,request_id),
 CHECK(receipt_sha256=encode(sha256(convert_to(receipt::text,'UTF8')),'hex'))
);
CREATE TABLE agent_supervised.spending_decisions (
 sequence bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, body jsonb NOT NULL,
 actor uuid NOT NULL, previous_sha256 text NOT NULL, sha256 text NOT NULL,
 CHECK(sha256=encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex'))
);
REVOKE ALL ON agent_supervised.spending_consents,agent_supervised.spending_decisions FROM PUBLIC;
CREATE TRIGGER spending_decisions_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON agent_supervised.spending_decisions
 FOR EACH STATEMENT EXECUTE FUNCTION ecosystem_social.immutable();
CREATE FUNCTION agent_supervised.consent_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF ROW(NEW.id,NEW.task_id,NEW.creator_id,NEW.request_id,NEW.budget_id,NEW.receipt,NEW.receipt_sha256,NEW.created_at,NEW.expires_at)
 IS DISTINCT FROM ROW(OLD.id,OLD.task_id,OLD.creator_id,OLD.request_id,OLD.budget_id,OLD.receipt,OLD.receipt_sha256,OLD.created_at,OLD.expires_at)
 OR (OLD.revoked_at IS NOT NULL AND NEW.revoked_at IS DISTINCT FROM OLD.revoked_at)
 OR (OLD.reservation_id IS NOT NULL AND NEW.reservation_id IS DISTINCT FROM OLD.reservation_id) THEN
 RAISE EXCEPTION 'spending_consent_immutable' USING ERRCODE='42501'; END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER spending_consent_immutable BEFORE UPDATE ON agent_supervised.spending_consents
 FOR EACH ROW EXECUTE FUNCTION agent_supervised.consent_immutable();
CREATE TRIGGER spending_consent_no_delete BEFORE DELETE OR TRUNCATE ON agent_supervised.spending_consents
 FOR EACH STATEMENT EXECUTE FUNCTION ecosystem_social.immutable();
CREATE FUNCTION agent_supervised.spending(actor uuid, op text, b jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
#variable_conflict use_variable
DECLARE t agent_supervised.tasks%ROWTYPE; c agent_supervised.spending_consents%ROWTYPE;
 now_at timestamptz:=clock_timestamp(); r jsonb; result jsonb; outcome text:='denied';
 owned boolean:=false; valid boolean:=false; previous text; event_body jsonb; request uuid;
BEGIN
 IF actor IS NULL OR actor='00000000-0000-0000-0000-000000000000'::uuid
 OR EXISTS(SELECT 1 FROM public.application_epoch) OR jsonb_typeof(b) IS DISTINCT FROM 'object'
 OR op NOT IN ('status','issue','revoke','receipt','reserve','check','deny') THEN
 RAISE EXCEPTION 'spending_scope_denied' USING ERRCODE='42501'; END IF;
 -- Same owner serialization as task commands; decisions survive denial because
 -- denial is returned, not raised. Rust commits it before reporting HTTP denial.
 PERFORM pg_advisory_xact_lock(hashtextextended('supervised:'||actor::text,0));
 SELECT * INTO t FROM agent_supervised.tasks WHERE id=(b->>'task_id')::uuid AND creator_id=actor FOR UPDATE;
 IF FOUND THEN
  PERFORM 1 FROM public.agent_access WHERE agent_id=t.agent_id AND principal_id=actor AND role='owner' FOR SHARE;
  owned=FOUND AND EXISTS(SELECT 1 FROM agent_factory.submissions WHERE agent_id=t.agent_id AND creator_id=actor);
 END IF;
 IF owned THEN
  SELECT * INTO c FROM agent_supervised.spending_consents WHERE task_id=t.id FOR UPDATE;
  valid=FOUND AND c.creator_id=actor AND c.revoked_at IS NULL AND c.expires_at>now_at
   AND c.receipt_sha256=encode(sha256(convert_to(c.receipt::text,'UTF8')),'hex')
   AND c.receipt->>'owner_id'=actor::text AND c.receipt->>'task_id'=t.id::text
   AND c.receipt->>'input_sha256'=t.input_sha256 AND t.approved_until>now_at
   AND t.state IN ('approved','running','paused','awaiting_review');
  IF op='status' THEN
   RETURN jsonb_build_object('consent',CASE WHEN c.id IS NULL THEN NULL ELSE
    jsonb_build_object('id',c.id,'task_id',t.id,'expires_at',c.expires_at,'max_cost_microusd',378,
     'state',CASE WHEN c.revoked_at IS NOT NULL THEN 'revoked' WHEN c.expires_at<=now_at THEN 'expired'
      WHEN c.reservation_id IS NOT NULL THEN 'consumed' WHEN valid THEN 'authorized' ELSE 'unavailable' END) END);
  ELSIF op='issue' THEN
   r=b->'receipt';request=(b->>'request_id')::uuid;
   IF c.id IS NULL AND t.state='approved' AND t.step=0 AND t.approved_until>now_at
    AND NOT EXISTS(SELECT 1 FROM agent_supervised.inferences WHERE task_id=t.id)
    AND t.revision=(b->>'expected_revision')::integer AND t.input_sha256=b->>'input_sha256'
    AND b->'confirmed'='true'::jsonb AND request IS NOT NULL AND request<>'00000000-0000-0000-0000-000000000000'::uuid
    AND NOT EXISTS(SELECT 1 FROM agent_supervised.commands WHERE creator_id=actor AND request_id=request)
    AND jsonb_typeof(r)='object' AND (SELECT count(*) FROM jsonb_object_keys(r))=9
    AND agent_supervised.reasoning_plan_valid(t.input->'reasoning') AND t.input->'reasoning' ? 'real'
    AND r->>'owner_id'=actor::text AND r->>'task_id'=t.id::text AND r->>'input_sha256'=t.input_sha256
    AND r->>'project'=t.input->'reasoning'->'real'->>'project' AND r->>'model'='gpt-6-luna'
    AND r->>'budget_id'=t.input->'reasoning'->'real'->>'budget_id'
    AND r->'per_task_microusd'='378'::jsonb AND r->'cumulative_microusd'='378'::jsonb
    AND (r->>'expires_at')::timestamptz=(t.input->'reasoning'->'real'->>'expires_at')::timestamptz
    AND (r->>'expires_at')::timestamptz>now_at AND (r->>'expires_at')::timestamptz<=now_at+interval '10 minutes'
    AND (r->>'expires_at')::timestamptz<=t.approved_until
    AND NOT EXISTS(SELECT 1 FROM agent_supervised.inferences WHERE policy->'real'->>'budget_id'=r->>'budget_id') THEN
    INSERT INTO agent_supervised.spending_consents(task_id,creator_id,request_id,budget_id,receipt,receipt_sha256,expires_at)
     VALUES(t.id,actor,request,(r->>'budget_id')::uuid,r,encode(sha256(convert_to(r::text,'UTF8')),'hex'),(r->>'expires_at')::timestamptz)
     ON CONFLICT DO NOTHING RETURNING * INTO c;
    IF FOUND THEN
     t.revision=t.revision+1;
     UPDATE agent_supervised.tasks SET revision=t.revision WHERE id=t.id;
     INSERT INTO agent_supervised.commands VALUES(actor,request,t.id,(b-'receipt')||jsonb_build_object('operation','spending_consent'));
     SELECT sha256 INTO previous FROM agent_supervised.events WHERE task_id=t.id ORDER BY revision DESC LIMIT 1;
     previous=coalesce(previous,repeat('0',64));
     event_body=jsonb_build_object('actor',actor,'operation','spending_consent','state',t.state,'step',t.step,'recorded_at',now_at,
      'input_sha256',t.input_sha256,'consent_id',c.id,'receipt_sha256',c.receipt_sha256,'max_cost_microusd',378,'expires_at',c.expires_at);
     INSERT INTO agent_supervised.events(task_id,revision,body,previous_sha256,sha256)
      VALUES(t.id,t.revision,event_body,previous,encode(sha256(convert_to(previous||event_body::text,'UTF8')),'hex'));
     outcome='authorized';
     result=jsonb_build_object('consent',jsonb_build_object('id',c.id,'task_id',t.id,'state','authorized','expires_at',c.expires_at,'max_cost_microusd',378));
    END IF;
   END IF;
  ELSIF op='revoke' AND c.id IS NOT NULL AND c.revoked_at IS NULL AND b->'confirmed'='true'::jsonb
   AND t.revision=(b->>'expected_revision')::integer AND t.input_sha256=b->>'input_sha256' THEN
   UPDATE agent_supervised.spending_consents SET revoked_at=now_at WHERE id=c.id;
   IF t.state NOT IN ('completed','failed','stopped') THEN
    PERFORM agent_supervised.command(actor,'stop',b);
   END IF;
   outcome='revoked';result=jsonb_build_object('consent',jsonb_build_object('id',c.id,'task_id',t.id,'state','revoked','expires_at',c.expires_at,'max_cost_microusd',378));
  ELSIF op='receipt' AND valid AND c.reservation_id IS NULL AND t.step=1 AND t.state='running' AND t.deadline>now_at THEN
   RETURN jsonb_build_object('id',c.id,'receipt',c.receipt,'receipt_sha256',c.receipt_sha256);
  ELSIF op='reserve' AND valid AND c.reservation_id IS NULL AND t.step=1 AND t.state='running' AND t.deadline>now_at
   AND c.id=(b->>'consent_id')::uuid AND t.input_sha256=b->>'input_sha256' THEN
   result=agent_supervised.command(actor,'reserve_inference',b-'consent_id');
   UPDATE agent_supervised.spending_consents SET reservation_id=(b->>'reservation_id')::uuid WHERE id=c.id;
   RETURN result;
  ELSIF op='check' AND valid AND c.reservation_id=(b->>'reservation_id')::uuid AND t.state='running' AND t.step=1
   AND t.deadline>now_at AND EXISTS(SELECT 1 FROM agent_supervised.inferences WHERE task_id=t.id AND state='reserved' AND reservation_id=c.reservation_id) THEN
   RETURN jsonb_build_object('valid',true);
  END IF;
 END IF;
 -- No raw request bodies, prompts, credentials or server receipts in decisions.
 SELECT sha256 INTO previous FROM agent_supervised.spending_decisions d WHERE d.actor=actor ORDER BY sequence DESC LIMIT 1;
 previous=coalesce(previous,repeat('0',64));
 event_body=jsonb_build_object('actor',actor,'operation',op,'outcome',outcome,'task_id',CASE WHEN owned THEN t.id ELSE NULL END,'recorded_at',now_at);
 INSERT INTO agent_supervised.spending_decisions(body,actor,previous_sha256,sha256)
  VALUES(event_body,actor,previous,encode(sha256(convert_to(previous||event_body::text,'UTF8')),'hex'));
 RETURN coalesce(result,jsonb_build_object('denied',true));
END;
$$;
REVOKE ALL ON FUNCTION agent_supervised.consent_immutable(),agent_supervised.spending(uuid,text,jsonb) FROM PUBLIC;
-- Explicit release grant: EXECUTE(spending) to the existing restricted runtime only.
COMMIT;
