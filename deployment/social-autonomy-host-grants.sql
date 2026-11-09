-- Reviewed proposal only: no automatic installation or policy activation.
-- Extend only the existing X-only ledger role; never the platform database role.
BEGIN;
DO $$ BEGIN
 IF current_database()<>'supa_x_first_post'
    OR EXISTS(SELECT 1 FROM public.application_epoch)
    OR NOT EXISTS(SELECT 1 FROM social_review.policies
      WHERE owner_id='6fd863e1-745d-4a06-b68e-eea69e854a69'::uuid
        AND account->>'username'='supa_agency' AND account->>'mode'='live')
 THEN RAISE EXCEPTION 'x_only_ledger_required'; END IF;
END $$;
GRANT USAGE ON SCHEMA social_autonomy TO supa_x_post_runtime;
GRANT EXECUTE ON FUNCTION social_autonomy.command(uuid,text,jsonb),
 social_autonomy.policy_mode(uuid) TO supa_x_post_runtime;
-- No table writes, new identities, platform roles, ownership or enabled flags.
COMMIT;
