-- Additive X-only manual pilot boundary. Never installed or provisioned at startup.
BEGIN;
CREATE TABLE social_review.pilot_approvals (
 authorization_id uuid PRIMARY KEY CHECK(authorization_id<>'00000000-0000-0000-0000-000000000000'),
 task_id uuid NOT NULL UNIQUE CHECK(task_id<>'00000000-0000-0000-0000-000000000000'),
 owner_id uuid NOT NULL, account jsonb NOT NULL,
 text_sha256 text NOT NULL CHECK(text_sha256~'^[0-9a-f]{64}$'),
 receipt_sha256 text NOT NULL UNIQUE CHECK(receipt_sha256~'^[0-9a-f]{64}$'),
 expires_at timestamptz NOT NULL, revoked boolean NOT NULL DEFAULT false
);
REVOKE ALL ON social_review.pilot_approvals FROM PUBLIC;
-- Existing runtime cannot create/modify approvals; owner provisioning is separate.
CREATE FUNCTION social_review.pilot_post(actor uuid, op text, b jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
#variable_conflict use_variable
DECLARE p social_review.policies%ROWTYPE; d social_review.drafts%ROWTYPE;
 chosen uuid:=(b->>'authorization_id')::uuid; now_at timestamptz:=clock_timestamp(); r jsonb;
 expected_hash text; permit social_review.pilot_approvals%ROWTYPE;
BEGIN
 SET LOCAL statement_timeout='2000ms'; SET LOCAL lock_timeout='1000ms';
 IF actor IS NULL OR actor='00000000-0000-0000-0000-000000000000'::uuid OR chosen IS NULL
  OR chosen='00000000-0000-0000-0000-000000000000'::uuid OR EXISTS(SELECT 1 FROM public.application_epoch)
 THEN RAISE EXCEPTION 'pilot_denied' USING ERRCODE='42501'; END IF;
 SELECT * INTO p FROM social_review.policies WHERE owner_id=actor AND enabled FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'pilot_owner_denied' USING ERRCODE='42501'; END IF;
 PERFORM ecosystem_social.lock_agent(p.agent_id);
 PERFORM 1 FROM public.agent_access WHERE agent_id=p.agent_id AND principal_id=actor AND role='owner' FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'pilot_owner_revoked' USING ERRCODE='42501'; END IF;
 IF p.account->>'username'<>'supa_agency' OR p.account->>'id' !~ '^[1-9][0-9]{0,19}$'
  OR (p.account->>'id')::numeric>18446744073709551615 OR p.account->>'mode' NOT IN ('mock','live')
 THEN RAISE EXCEPTION 'pilot_account_denied' USING ERRCODE='42501'; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('supa-x-account:'||(p.account->>'id'),0));
 SELECT * INTO permit FROM social_review.pilot_approvals WHERE authorization_id=chosen AND owner_id=actor FOR SHARE;
 IF NOT FOUND OR permit.account IS DISTINCT FROM p.account OR permit.task_id IS DISTINCT FROM (b->>'task_id')::uuid
  OR permit.receipt_sha256 IS DISTINCT FROM b->>'receipt_sha256' OR (permit.revoked AND op NOT IN ('read','recover','settle'))
 THEN RAISE EXCEPTION 'pilot_protected_authority_required' USING ERRCODE='42501'; END IF;
 expected_hash=permit.text_sha256;
 IF op IN ('reserve','peek','claim') AND permit.expires_at<=now_at THEN RAISE EXCEPTION 'pilot_expired' USING ERRCODE='42501'; END IF;

 SELECT * INTO d FROM social_review.drafts WHERE id=chosen AND agent_id=p.agent_id FOR UPDATE;
 IF op='reserve' THEN
  IF lower(regexp_replace(b->>'text','[^[:alnum:]]','','g'))=lower(regexp_replace('Supa Agent reporting for onboarding. Our official X account is connected and its identity verified. Autonomous posting is still off. Apparently even AI needs a permission slip.','[^[:alnum:]]','','g'))
  THEN RAISE EXCEPTION 'pilot_historical_content_denied' USING ERRCODE='42501'; END IF;

  IF b-ARRAY['authorization_id','account','text','text_sha256','expires_at','budget_microusd','confirmed','read_write_confirmed','credits_confirmed','identity_receipt_sha256','task_id','receipt_sha256']<>'{}'::jsonb
   OR b->'account' IS DISTINCT FROM p.account OR b->>'text_sha256' IS DISTINCT FROM expected_hash
   OR encode(sha256(convert_to(b->>'text','UTF8')),'hex') IS DISTINCT FROM expected_hash
   OR b->'confirmed' IS DISTINCT FROM 'true'::jsonb OR b->'read_write_confirmed' IS DISTINCT FROM 'true'::jsonb
   OR b->'credits_confirmed' IS DISTINCT FROM 'true'::jsonb OR b->'budget_microusd' IS DISTINCT FROM '25000'::jsonb
   OR coalesce(b->>'identity_receipt_sha256','') !~ '^[0-9a-f]{64}$'
   OR (b->>'expires_at')::timestamptz IS DISTINCT FROM permit.expires_at
   OR jsonb_typeof(b->'expires_at') IS DISTINCT FROM 'string'
   OR (b->>'expires_at')::timestamptz<=now_at OR (b->>'expires_at')::timestamptz>now_at+interval '10 minutes'
  THEN RAISE EXCEPTION 'pilot_exact_approval_required' USING ERRCODE='42501'; END IF;
  IF d.id IS NOT NULL THEN
   IF d.approval IS DISTINCT FROM b OR d.text IS DISTINCT FROM b->>'text' THEN RAISE EXCEPTION 'pilot_conflict' USING ERRCODE='40001'; END IF;
   RETURN jsonb_build_object('fresh',false,'state',d.state);
  END IF;
  IF (social_review.context(p.agent_id)->>'paused')::boolean IS DISTINCT FROM false
   OR EXISTS(SELECT 1 FROM social_review.attempts WHERE account_id=p.account->>'id' AND state IN ('reserved','uncertain'))
   OR EXISTS(SELECT 1 FROM social_autonomy.attempts WHERE account_id=p.account->>'id' AND state IN ('reserved','in_flight','uncertain'))
   OR EXISTS(SELECT 1 FROM social_review.drafts WHERE account=p.account AND receipt->>'http_status'='429'
       AND (receipt->>'rate_limit_reset' IS NULL OR (receipt->>'rate_limit_reset')::numeric>extract(epoch FROM now_at)))
  THEN RAISE EXCEPTION 'pilot_paused_or_quarantined' USING ERRCODE='42501'; END IF;
  INSERT INTO social_review.drafts(id,agent_id,state,text,text_sha256,decision,topics,account,approval)
  VALUES(chosen,p.agent_id,'reserved',b->>'text',expected_hash,
    jsonb_build_object('kind','owner_reviewed_exact_pilot','identity_receipt_sha256',b->>'identity_receipt_sha256'),
    '["supervised_hosted_pilot"]',p.account,b) RETURNING * INTO d;
  -- Existing unique normalized text and cross-manual/autonomous trigger enforce duplicates/caps/spacing.
  INSERT INTO social_review.attempts(agent_id,draft_id,account_id,normalized_text)
   VALUES(p.agent_id,chosen,p.account->>'id',lower(regexp_replace(d.text,'[^[:alnum:]]','','g')));
  PERFORM social_autonomy.audit(actor,p.agent_id,'pilot_reserved',jsonb_build_object('authorization_id',chosen,'approval',b,'reserved_microusd',25000));
  RETURN jsonb_build_object('fresh',true,'state','reserved');
 END IF;
 IF d.id IS NULL OR d.account IS DISTINCT FROM p.account OR d.text_sha256<>expected_hash
  OR d.decision->>'kind' IS DISTINCT FROM 'owner_reviewed_exact_pilot'
 THEN RAISE EXCEPTION 'pilot_reservation_denied' USING ERRCODE='42501'; END IF;
 IF op='read' THEN RETURN jsonb_build_object('state',d.state,'receipt',d.receipt,'approval',d.approval); END IF;
 IF op IN ('peek','claim') THEN
  IF d.state<>'reserved' OR d.receipt IS NOT NULL OR d.approval IS NULL
   OR (d.approval->>'expires_at')::timestamptz<=now_at
   OR d.approval->'confirmed' IS DISTINCT FROM 'true'::jsonb
   OR d.approval->'budget_microusd' IS DISTINCT FROM '25000'::jsonb
   OR b->>'text_sha256' IS DISTINCT FROM d.text_sha256 OR b->'account' IS DISTINCT FROM d.account
   OR (social_review.context(p.agent_id)->>'paused')::boolean IS DISTINCT FROM false
  THEN RAISE EXCEPTION 'pilot_dispatch_denied' USING ERRCODE='42501'; END IF;
  IF op='claim' THEN
   -- Keep attempt state reserved so all existing account-wide fences still see it.
   UPDATE social_review.drafts SET receipt=jsonb_build_object('state','in_flight','mode',p.account->>'mode'),updated_at=now_at WHERE id=chosen;
   PERFORM social_autonomy.audit(actor,p.agent_id,'pilot_claimed',jsonb_build_object('authorization_id',chosen,'text_sha256',d.text_sha256));
  END IF;
  RETURN 'true'::jsonb;
 END IF;
 IF op='cancel' AND d.state='reserved' AND d.receipt IS NULL THEN
  UPDATE social_review.drafts SET approval=NULL WHERE id=chosen;
  PERFORM social_autonomy.audit(actor,p.agent_id,'pilot_cancelled',jsonb_build_object('authorization_id',chosen));
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
  THEN RAISE EXCEPTION 'pilot_receipt_denied' USING ERRCODE='42501'; END IF;
  IF d.state<>'reserved' THEN
   IF r->>'http_status'='429' AND r->>'rate_limit_reset' IS NULL THEN r=jsonb_set(r,'{rate_limit_reset}',d.receipt->'rate_limit_reset',true); END IF;
   IF d.receipt=r THEN RETURN r; END IF;
   RAISE EXCEPTION 'pilot_already_settled' USING ERRCODE='42501';
  END IF;
  IF r->>'state'='posted' AND (d.receipt->>'state' IS DISTINCT FROM 'in_flight' OR coalesce(r->>'post_id','') !~ '^[1-9][0-9]{0,19}$'
      OR (r->>'post_id')::numeric>18446744073709551615 OR coalesce((r->>'http_status')::integer,0) NOT BETWEEN 200 AND 299)
  THEN RAISE EXCEPTION 'pilot_receipt_denied' USING ERRCODE='42501'; END IF;
 ELSE RAISE EXCEPTION 'pilot_operation_denied' USING ERRCODE='42501'; END IF;
 IF r->>'http_status'='429' AND (r->>'rate_limit_reset' IS NULL OR (r->>'rate_limit_reset')::numeric<=extract(epoch FROM now_at)) THEN
  r=jsonb_set(r,'{rate_limit_reset}',to_jsonb(extract(epoch FROM now_at+interval '24 hours')::bigint));
 END IF;
 UPDATE social_review.drafts SET state=r->>'state',receipt=r,updated_at=now_at WHERE id=chosen;
 UPDATE social_review.attempts SET state=r->>'state' WHERE draft_id=chosen;
 PERFORM social_autonomy.audit(actor,p.agent_id,'pilot_settled',jsonb_build_object('authorization_id',chosen,'receipt',r,'reserved_microusd',25000,'actual_billed_cost','unavailable_from_post_endpoint'));
 RETURN r;
END $$;
REVOKE ALL ON FUNCTION social_review.pilot_post(uuid,text,jsonb) FROM PUBLIC;
CREATE FUNCTION social_review.pilot_cluster() RETURNS text LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog AS 'SELECT system_identifier::text FROM pg_control_system()';
REVOKE ALL ON FUNCTION social_review.pilot_cluster() FROM PUBLIC;
-- Apply only in the reviewed dedicated ledger; no identities, approval rows or grants created.
COMMIT;
