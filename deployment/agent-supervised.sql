-- Explicit additive synthetic-only extension. No automatic installation or grants.
BEGIN;
CREATE SCHEMA agent_supervised;
REVOKE ALL ON SCHEMA agent_supervised FROM PUBLIC;
CREATE TABLE agent_supervised.tasks (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), agent_id uuid NOT NULL REFERENCES public.agents(id),
 creator_id uuid NOT NULL, request_id uuid NOT NULL, input jsonb NOT NULL,
 input_sha256 text NOT NULL, state text NOT NULL DEFAULT 'pending'
 CHECK(state IN ('pending','approved','running','paused','stopped','completed','failed')),
 revision integer NOT NULL DEFAULT 0, step integer NOT NULL DEFAULT 0 CHECK(step BETWEEN 0 AND 3),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(), approved_until timestamptz,
 started_at timestamptz, deadline timestamptz, finished_at timestamptz,
 payloads jsonb NOT NULL DEFAULT '[]', error_code text,
 UNIQUE(creator_id,agent_id,request_id), CHECK(input_sha256=encode(sha256(convert_to(input::text,'UTF8')),'hex'))
);
CREATE FUNCTION agent_supervised.immutable_input() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF ROW(NEW.id,NEW.agent_id,NEW.creator_id,NEW.request_id,NEW.input,NEW.input_sha256,NEW.created_at)
 IS DISTINCT FROM ROW(OLD.id,OLD.agent_id,OLD.creator_id,OLD.request_id,OLD.input,OLD.input_sha256,OLD.created_at) THEN
 RAISE EXCEPTION 'supervised_input_immutable' USING ERRCODE='42501'; END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER supervised_input_immutable BEFORE UPDATE ON agent_supervised.tasks
 FOR EACH ROW EXECUTE FUNCTION agent_supervised.immutable_input();
CREATE UNIQUE INDEX one_supervised_active_per_agent ON agent_supervised.tasks(agent_id)
 WHERE state IN ('approved','running','paused');
CREATE TABLE agent_supervised.commands (
 creator_id uuid NOT NULL, request_id uuid NOT NULL, task_id uuid NOT NULL REFERENCES agent_supervised.tasks,
 body jsonb NOT NULL, PRIMARY KEY(creator_id,request_id)
);
CREATE TABLE agent_supervised.events (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(), task_id uuid NOT NULL REFERENCES agent_supervised.tasks,
 revision integer NOT NULL, body jsonb NOT NULL, previous_sha256 text NOT NULL, sha256 text NOT NULL,
 UNIQUE(task_id,revision), CHECK(sha256=encode(sha256(convert_to(previous_sha256||body::text,'UTF8')),'hex'))
);
REVOKE ALL ON ALL TABLES IN SCHEMA agent_supervised FROM PUBLIC;
CREATE TRIGGER supervised_events_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON agent_supervised.events
 FOR EACH STATEMENT EXECUTE FUNCTION ecosystem_social.immutable();
CREATE TRIGGER supervised_commands_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON agent_supervised.commands
 FOR EACH STATEMENT EXECUTE FUNCTION ecosystem_social.immutable();
CREATE FUNCTION agent_supervised.view_task(t agent_supervised.tasks) RETURNS jsonb
LANGUAGE sql STABLE SET search_path=pg_catalog AS $$
 SELECT to_jsonb(t)||jsonb_build_object('events',coalesce((SELECT jsonb_agg(to_jsonb(e) ORDER BY revision) FROM agent_supervised.events e WHERE task_id=t.id),'[]'))
$$;
CREATE FUNCTION agent_supervised.command(actor uuid, op text, b jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
#variable_conflict use_variable
DECLARE t agent_supervised.tasks%ROWTYPE; old agent_supervised.commands%ROWTYPE;
 aid uuid; request uuid; f agent_factory.submissions%ROWTYPE; event_body jsonb; previous text;
 now_at timestamptz:=clock_timestamp(); content jsonb; action text;
BEGIN
 IF actor IS NULL OR actor='00000000-0000-0000-0000-000000000000'::uuid
 OR EXISTS(SELECT 1 FROM public.application_epoch) OR jsonb_typeof(b)<>'object' THEN
  RAISE EXCEPTION 'supervised_scope_denied' USING ERRCODE='42501'; END IF;
 IF op IN ('list','create') THEN aid=(b->>'agent_id')::uuid;
 ELSE SELECT agent_id INTO aid FROM agent_supervised.tasks WHERE id=(b->>'task_id')::uuid AND creator_id=actor; END IF;
 -- Serialize revocation with all owner operations, including result commits.
 PERFORM 1 FROM public.agent_access WHERE agent_id=aid AND principal_id=actor AND role='owner' FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'supervised_owner_unavailable' USING ERRCODE='42501'; END IF;
 SELECT * INTO f FROM agent_factory.submissions WHERE agent_id=aid AND creator_id=actor;
 IF NOT FOUND THEN RAISE EXCEPTION 'supervised_factory_agent_required' USING ERRCODE='42501'; END IF;
 IF op='list' THEN RETURN jsonb_build_object('tasks',coalesce((SELECT jsonb_agg(agent_supervised.view_task(q) ORDER BY q.created_at) FROM agent_supervised.tasks q WHERE q.agent_id=aid AND q.creator_id=actor),'[]')); END IF;
 IF op='get' THEN SELECT * INTO t FROM agent_supervised.tasks WHERE id=(b->>'task_id')::uuid AND creator_id=actor; RETURN agent_supervised.view_task(t); END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('supervised:'||actor::text,0));
 request=(b->>'request_id')::uuid;
 IF request IS NULL OR request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION 'request_required' USING ERRCODE='22023'; END IF;
 IF op='create' THEN
  content=b->'input';
  IF jsonb_typeof(content)<>'object' OR (SELECT count(*) FROM jsonb_object_keys(content))<>5
  OR NOT content ?& ARRAY['task','desired_outcome','material','synthetic_only','retain_memory']
  OR content->'synthetic_only'<>'true'::jsonb OR jsonb_typeof(content->'retain_memory')<>'boolean'
  OR jsonb_typeof(content->'task')<>'string' OR length(content->>'task') NOT BETWEEN 1 AND 600
  OR jsonb_typeof(content->'desired_outcome')<>'string' OR length(content->>'desired_outcome') NOT BETWEEN 1 AND 120
  OR jsonb_typeof(content->'material')<>'string' OR octet_length(content->>'material') NOT BETWEEN 1 AND 4000
  OR (content->'retain_memory'='true'::jsonb AND NOT coalesce(f.configuration->'requested_capabilities' ? 'remember',false)) THEN
   RAISE EXCEPTION 'supervised_input_or_permission_invalid' USING ERRCODE='42501'; END IF;
  SELECT * INTO t FROM agent_supervised.tasks WHERE agent_id=aid AND creator_id=actor AND request_id=request;
  IF FOUND THEN IF t.input<>content THEN RAISE EXCEPTION 'request_conflict' USING ERRCODE='40001'; END IF; RETURN agent_supervised.view_task(t); END IF;
  IF (SELECT count(*) FROM agent_supervised.tasks WHERE creator_id=actor)>=100 OR (SELECT count(*) FROM agent_supervised.tasks WHERE agent_id=aid)>=20 THEN RAISE EXCEPTION 'task_quota' USING ERRCODE='54000'; END IF;
  INSERT INTO agent_supervised.tasks(agent_id,creator_id,request_id,input,input_sha256) VALUES(aid,actor,request,content,encode(sha256(convert_to(content::text,'UTF8')),'hex')) RETURNING * INTO t;
 ELSE
  SELECT * INTO t FROM agent_supervised.tasks WHERE id=(b->>'task_id')::uuid AND creator_id=actor FOR UPDATE;
  SELECT * INTO old FROM agent_supervised.commands WHERE creator_id=actor AND request_id=request;
  IF FOUND THEN IF old.task_id<>t.id OR old.body<>(b-'payload')||jsonb_build_object('operation',op) THEN RAISE EXCEPTION 'command_conflict' USING ERRCODE='40001'; END IF; RETURN agent_supervised.view_task(t); END IF;
  IF t.revision IS DISTINCT FROM (b->>'expected_revision')::integer OR t.input_sha256 IS DISTINCT FROM b->>'input_sha256' THEN RAISE EXCEPTION 'stale_review' USING ERRCODE='40001'; END IF;
  IF t.state IN ('completed','stopped','failed') OR (t.revision>=32 AND op<>'stop') THEN RAISE EXCEPTION 'terminal_or_command_limit' USING ERRCODE='42501'; END IF;
  IF op='stop' THEN t.state='stopped';t.finished_at=now_at;
  ELSIF op='pause' AND t.state='running' THEN t.state='paused';
  ELSIF op='approve' AND t.state='pending' AND b->'confirmed'='true'::jsonb AND now_at<t.created_at+interval '1 hour' THEN t.state='approved';t.approved_until=now_at+interval '10 minutes';
  ELSIF op IN ('start','resume') AND b->'confirmed'='true'::jsonb AND t.approved_until>now_at AND ((op='start' AND t.state='approved') OR (op='resume' AND t.state='paused')) THEN
   t.state='running';t.started_at=coalesce(t.started_at,now_at);t.deadline=coalesce(t.deadline,least(now_at+interval '120 seconds',t.created_at+interval '1 hour'));
   IF t.deadline<=now_at THEN RAISE EXCEPTION 'execution_expired' USING ERRCODE='42501'; END IF;
  ELSIF op='advance' AND t.state='running' THEN
   IF t.deadline<=now_at OR t.approved_until<=now_at THEN t.state='failed';t.error_code='execution_deadline_exceeded';t.finished_at=now_at;
   ELSE
    action=CASE t.step WHEN 0 THEN 'inspect_input' WHEN 1 THEN 'extract_evidence' WHEN 2 THEN 'retain_and_assess' END;
    IF jsonb_typeof(b->'payload')<>'object' OR b->'payload'->>'action' IS DISTINCT FROM action OR octet_length((b->'payload')::text)>16000 THEN RAISE EXCEPTION 'invalid_runtime_result' USING ERRCODE='22023'; END IF;
    t.payloads=t.payloads||jsonb_build_array(b->'payload');t.step=t.step+1;
    IF t.step=3 THEN t.state='completed';t.finished_at=now_at;END IF;
   END IF;
  ELSE RAISE EXCEPTION 'transition_not_authorized' USING ERRCODE='42501'; END IF;
  t.revision=t.revision+1;
  UPDATE agent_supervised.tasks SET state=t.state,revision=t.revision,step=t.step,approved_until=t.approved_until,started_at=t.started_at,deadline=t.deadline,finished_at=t.finished_at,payloads=t.payloads,error_code=t.error_code WHERE id=t.id;
 END IF;
 INSERT INTO agent_supervised.commands VALUES(actor,request,t.id,(b-'payload')||jsonb_build_object('operation',op));
 SELECT sha256 INTO previous FROM agent_supervised.events WHERE task_id=t.id ORDER BY revision DESC LIMIT 1;previous=coalesce(previous,repeat('0',64));
 event_body=jsonb_build_object('actor',actor,'operation',op,'state',t.state,'step',t.step,'recorded_at',now_at,'input_sha256',t.input_sha256,'payloads_sha256',encode(sha256(convert_to(t.payloads::text,'UTF8')),'hex'),'error_code',t.error_code);
 INSERT INTO agent_supervised.events(task_id,revision,body,previous_sha256,sha256) VALUES(t.id,t.revision,event_body,previous,encode(sha256(convert_to(previous||event_body::text,'UTF8')),'hex'));
 RETURN agent_supervised.view_task(t);
END;
$$;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA agent_supervised FROM PUBLIC;
-- Runtime receives only USAGE(agent_supervised) and EXECUTE(command). No table writes.
COMMIT;
