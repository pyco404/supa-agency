-- Synthetic-only fixture. Never install into an imported/live ledger.
CREATE ROLE supa_x_post_runtime LOGIN;
REVOKE ALL ON DATABASE supa_x_first_post FROM PUBLIC;
GRANT CONNECT ON DATABASE supa_x_first_post TO supa_x_post_runtime;
INSERT INTO agents(id,data) VALUES('00000000-0000-0000-0000-00000000000a','{"name":"synthetic-x-worker","reference_only":true,"spending_authority":0}');
INSERT INTO agent_access(agent_id,principal_id,role,granted_by) VALUES('00000000-0000-0000-0000-00000000000a','00000000-0000-0000-0000-000000000001','owner','00000000-0000-0000-0000-000000000001');
INSERT INTO social_review.policies(agent_id,owner_id,account,enabled) VALUES('00000000-0000-0000-0000-00000000000a','00000000-0000-0000-0000-000000000001','{"id":"1","username":"supa_agency","mode":"mock"}',true);
INSERT INTO social_review.pauses(agent_id,paused) VALUES('00000000-0000-0000-0000-00000000000a',false);
INSERT INTO activity(id,agent_id,kind,code,simulated) VALUES('00000000-0000-0000-0000-0000000000c8','00000000-0000-0000-0000-00000000000a','cycle','completed',true);
INSERT INTO social_review.releases(agent_id,event_id,actor,snapshot_sha256,released)
 SELECT '00000000-0000-0000-0000-00000000000a',(s->'event'->>'id')::uuid,'00000000-0000-0000-0000-000000000001',s->>'snapshot_sha256',true
 FROM jsonb_array_elements(social_review.context('00000000-0000-0000-0000-00000000000a')->'sources') s;
GRANT USAGE ON SCHEMA social_review,social_autonomy TO supa_x_post_runtime;
GRANT EXECUTE ON FUNCTION social_review.first_post(uuid,text,jsonb),social_autonomy.command(uuid,text,jsonb),social_autonomy.policy_mode(uuid) TO supa_x_post_runtime;
