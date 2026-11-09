-- Explicit local-only addition. Reuses review drafts/attempts and autonomy audit/fences.
-- No automatic installation, owner enrollment, grants, scheduler or hosted mutation.
BEGIN;
CREATE FUNCTION social_review.first_post(actor uuid, op text, b jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
#variable_conflict use_variable
DECLARE p social_review.policies%ROWTYPE; d social_review.drafts%ROWTYPE;
 chosen uuid:=(b->>'authorization_id')::uuid; now_at timestamptz:=clock_timestamp(); r jsonb;
 expected_hash constant text:='e8bc2bee94696da203aad809d8a80aeb05a4677960ef34d9b0b68b82d47f2dd7';
BEGIN
 SET LOCAL statement_timeout='2000ms'; SET LOCAL lock_timeout='1000ms';
 IF actor IS NULL OR actor='00000000-0000-0000-0000-000000000000'::uuid OR chosen IS NULL
  OR chosen='00000000-0000-0000-0000-000000000000'::uuid OR EXISTS(SELECT 1 FROM public.application_epoch)
 THEN RAISE EXCEPTION 'first_post_denied' USING ERRCODE='42501'; END IF;
 SELECT * INTO p FROM social_review.policies WHERE owner_id=actor AND enabled FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'first_post_owner_denied' USING ERRCODE='42501'; END IF;
 PERFORM ecosystem_social.lock_agent(p.agent_id);
 PERFORM 1 FROM public.agent_access WHERE agent_id=p.agent_id AND principal_id=actor AND role='owner' FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'first_post_owner_revoked' USING ERRCODE='42501'; END IF;
 IF p.account->>'username'<>'supa_agency' OR p.account->>'id' !~ '^[1-9][0-9]{0,19}$'
  OR (p.account->>'id')::numeric>18446744073709551615 OR p.account->>'mode' NOT IN ('mock','live')
 THEN RAISE EXCEPTION 'first_post_account_denied' USING ERRCODE='42501'; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('supa-x-account:'||(p.account->>'id'),0));
 SELECT * INTO d FROM social_review.drafts WHERE id=chosen AND agent_id=p.agent_id FOR UPDATE;
 IF op='reserve' THEN
  IF b-ARRAY['authorization_id','account','text','text_sha256','expires_at','budget_microusd','confirmed','read_write_confirmed','credits_confirmed','identity_receipt_sha256']<>'{}'::jsonb
   OR b->'account' IS DISTINCT FROM p.account OR b->>'text_sha256' IS DISTINCT FROM expected_hash
   OR encode(sha256(convert_to(b->>'text','UTF8')),'hex') IS DISTINCT FROM expected_hash
   OR b->'confirmed' IS DISTINCT FROM 'true'::jsonb OR b->'read_write_confirmed' IS DISTINCT FROM 'true'::jsonb
   OR b->'credits_confirmed' IS DISTINCT FROM 'true'::jsonb OR b->'budget_microusd' IS DISTINCT FROM '25000'::jsonb
   OR coalesce(b->>'identity_receipt_sha256','') !~ '^[0-9a-f]{64}$'
   OR jsonb_typeof(b->'expires_at') IS DISTINCT FROM 'string'
   OR (b->>'expires_at')::timestamptz<=now_at OR (b->>'expires_at')::timestamptz>now_at+interval '10 minutes'
  THEN RAISE EXCEPTION 'first_post_exact_approval_required' USING ERRCODE='42501'; END IF;
  IF d.id IS NOT NULL THEN
   IF d.approval IS DISTINCT FROM b OR d.text IS DISTINCT FROM b->>'text' THEN RAISE EXCEPTION 'first_post_conflict' USING ERRCODE='40001'; END IF;
   RETURN jsonb_build_object('fresh',false,'state',d.state);
  END IF;
  IF (social_review.context(p.agent_id)->>'paused')::boolean IS DISTINCT FROM false
   OR EXISTS(SELECT 1 FROM social_review.attempts WHERE account_id=p.account->>'id' AND state IN ('reserved','uncertain'))
   OR EXISTS(SELECT 1 FROM social_autonomy.attempts WHERE account_id=p.account->>'id' AND state IN ('reserved','in_flight','uncertain'))
   OR EXISTS(SELECT 1 FROM social_review.drafts WHERE account=p.account AND receipt->>'http_status'='429'
       AND (receipt->>'rate_limit_reset' IS NULL OR (receipt->>'rate_limit_reset')::numeric>extract(epoch FROM now_at)))
  THEN RAISE EXCEPTION 'first_post_paused_or_quarantined' USING ERRCODE='42501'; END IF;
  INSERT INTO social_review.drafts(id,agent_id,state,text,text_sha256,decision,topics,account,approval)
  VALUES(chosen,p.agent_id,'reserved',b->>'text',expected_hash,
    jsonb_build_object('kind','owner_reviewed_identity_milestone','identity_receipt_sha256',b->>'identity_receipt_sha256'),
    '["official_account_connection"]',p.account,b) RETURNING * INTO d;
  -- Existing unique normalized text and cross-manual/autonomous trigger enforce duplicates/caps/spacing.
  INSERT INTO social_review.attempts(agent_id,draft_id,account_id,normalized_text)
   VALUES(p.agent_id,chosen,p.account->>'id',lower(regexp_replace(d.text,'[^[:alnum:]]','','g')));
  PERFORM social_autonomy.audit(actor,p.agent_id,'first_post_reserved',jsonb_build_object('authorization_id',chosen,'approval',b,'reserved_microusd',25000));
  RETURN jsonb_build_object('fresh',true,'state','reserved');
 END IF;
 IF d.id IS NULL OR d.account IS DISTINCT FROM p.account OR d.text_sha256<>expected_hash
  OR d.decision->>'kind' IS DISTINCT FROM 'owner_reviewed_identity_milestone'
 THEN RAISE EXCEPTION 'first_post_reservation_denied' USING ERRCODE='42501'; END IF;
 IF op='read' THEN RETURN jsonb_build_object('state',d.state,'receipt',d.receipt,'approval',d.approval); END IF;
 IF op IN ('peek','claim') THEN
  IF d.state<>'reserved' OR d.receipt IS NOT NULL OR d.approval IS NULL
   OR (d.approval->>'expires_at')::timestamptz<=now_at
   OR d.approval->'confirmed' IS DISTINCT FROM 'true'::jsonb
   OR d.approval->'budget_microusd' IS DISTINCT FROM '25000'::jsonb
   OR b->>'text_sha256' IS DISTINCT FROM d.text_sha256 OR b->'account' IS DISTINCT FROM d.account
   OR (social_review.context(p.agent_id)->>'paused')::boolean IS DISTINCT FROM false
  THEN RAISE EXCEPTION 'first_post_dispatch_denied' USING ERRCODE='42501'; END IF;
  IF op='claim' THEN
   -- Keep attempt state reserved so all existing account-wide fences still see it.
   UPDATE social_review.drafts SET receipt=jsonb_build_object('state','in_flight','mode',p.account->>'mode'),updated_at=now_at WHERE id=chosen;
   PERFORM social_autonomy.audit(actor,p.agent_id,'first_post_claimed',jsonb_build_object('authorization_id',chosen,'text_sha256',d.text_sha256));
  END IF;
  RETURN 'true'::jsonb;
 END IF;
 IF op='cancel' AND d.state='reserved' AND d.receipt IS NULL THEN
  UPDATE social_review.drafts SET approval=NULL WHERE id=chosen;
  PERFORM social_autonomy.audit(actor,p.agent_id,'first_post_cancelled',jsonb_build_object('authorization_id',chosen));
  RETURN 'true'::jsonb;
 END IF;
 IF op='recover' THEN
  IF d.state<>'reserved' THEN RETURN jsonb_build_object('state',d.state,'receipt',d.receipt); END IF;
  r=jsonb_build_object('mode',p.account->>'mode','state','uncertain','category','restart_unconfirmed_no_retry');
 ELSIF op='settle' THEN
  r=b->'receipt';
  IF r->>'mode' IS DISTINCT FROM p.account->>'mode' OR r->>'state' NOT IN ('posted','failed','uncertain')
   OR r-ARRAY['mode','state','category','post_id','http_status','rate_limit_reset']<>'{}'::jsonb
   OR r->>'category' IS NULL OR length(r->>'category')>100
  THEN RAISE EXCEPTION 'first_post_receipt_denied' USING ERRCODE='42501'; END IF;
  IF d.state<>'reserved' THEN
   IF r->>'http_status'='429' AND r->>'rate_limit_reset' IS NULL THEN r=jsonb_set(r,'{rate_limit_reset}',d.receipt->'rate_limit_reset',true); END IF;
   IF d.receipt=r THEN RETURN r; END IF;
   RAISE EXCEPTION 'first_post_already_settled' USING ERRCODE='42501';
  END IF;
  IF r->>'state'='posted' AND (d.receipt->>'state' IS DISTINCT FROM 'in_flight' OR coalesce(r->>'post_id','') !~ '^[1-9][0-9]{0,19}$'
      OR (r->>'post_id')::numeric>18446744073709551615 OR coalesce((r->>'http_status')::integer,0) NOT BETWEEN 200 AND 299)
  THEN RAISE EXCEPTION 'first_post_receipt_denied' USING ERRCODE='42501'; END IF;
 ELSE RAISE EXCEPTION 'first_post_operation_denied' USING ERRCODE='42501'; END IF;
 IF r->>'http_status'='429' AND (r->>'rate_limit_reset' IS NULL OR (r->>'rate_limit_reset')::numeric<=extract(epoch FROM now_at)) THEN
  r=jsonb_set(r,'{rate_limit_reset}',to_jsonb(extract(epoch FROM now_at+interval '24 hours')::bigint));
 END IF;
 UPDATE social_review.drafts SET state=r->>'state',receipt=r,updated_at=now_at WHERE id=chosen;
 UPDATE social_review.attempts SET state=r->>'state' WHERE draft_id=chosen;
 PERFORM social_autonomy.audit(actor,p.agent_id,'first_post_settled',jsonb_build_object('authorization_id',chosen,'receipt',r,'reserved_microusd',25000,'actual_billed_cost','unavailable_from_post_endpoint'));
 RETURN r;
END $$;
REVOKE ALL ON FUNCTION social_review.first_post(uuid,text,jsonb) FROM PUBLIC;
-- A manual 429 must also fence later autonomous attempts without new paid probes.
CREATE FUNCTION social_review.first_post_rate_fence() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM social_review.drafts WHERE account->>'id'=NEW.account_id AND receipt->>'http_status'='429'
    AND (receipt->>'rate_limit_reset' IS NULL OR (receipt->>'rate_limit_reset')::numeric>extract(epoch FROM clock_timestamp())))
 THEN RAISE EXCEPTION 'first_post_rate_limit_active' USING ERRCODE='42501'; END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION social_review.first_post_rate_fence() FROM PUBLIC;
CREATE TRIGGER first_post_shared_rate_fence BEFORE INSERT ON social_autonomy.attempts FOR EACH ROW EXECUTE FUNCTION social_review.first_post_rate_fence();
COMMIT;
