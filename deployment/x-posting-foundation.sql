-- X-only local ledger dependency closure. No wallet, inference, accounting or worker tables.
-- Definitions extracted unchanged from the reviewed migrations listed below.
BEGIN;
-- Source: migrations/0001_foundation.sql
CREATE TABLE agents (
    id uuid PRIMARY KEY,
    data jsonb NOT NULL,
    simulated_balance_microusd bigint NOT NULL DEFAULT 0 CHECK (simulated_balance_microusd >= 0),
    next_cycle_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE cycles (
    id uuid PRIMARY KEY, agent_id uuid NOT NULL REFERENCES agents(id),
    status text NOT NULL CHECK(status IN ('running','completed','failed','interrupted')),
    lease_until timestamptz NOT NULL, started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz, failure_code text
);
CREATE TABLE activity (
    id uuid PRIMARY KEY, agent_id uuid NOT NULL REFERENCES agents(id), cycle_id uuid REFERENCES cycles(id),
    kind text NOT NULL, code text NOT NULL, simulated boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now()
);
-- Source: migrations/0006_authorization_ownership_health.sql
CREATE TABLE agent_access (
 agent_id uuid NOT NULL REFERENCES agents(id), principal_id uuid NOT NULL,
 role text NOT NULL CHECK(role IN ('owner','operator','viewer')),
 granted_by uuid NOT NULL, granted_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(agent_id,principal_id)
);
-- Source: migrations/0008_application_epoch.sql
CREATE TABLE application_epoch (
    singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
    id uuid NOT NULL UNIQUE CHECK(id <> 'ad91e475-f1da-495a-8247-139643d2d8b1'),
    accounting_namespace uuid NOT NULL UNIQUE CHECK(accounting_namespace <> 'ad91e475-f1da-495a-8247-139643d2d8b1'),
    label text NOT NULL CHECK(length(label) BETWEEN 1 AND 80),
    kind text NOT NULL CHECK(kind = 'development'),
    state text NOT NULL CHECK(state = 'prepared'),
    historical_database text NOT NULL CHECK(historical_database = 'unrecovered'),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX one_agent_owner ON agent_access(agent_id) WHERE role='owner';
-- Source: migrations/0013_work_evaluation_reviews.sql
CREATE FUNCTION work_review_canonical(value jsonb) RETURNS text
LANGUAGE plpgsql IMMUTABLE STRICT AS $$
DECLARE result text;
BEGIN
 CASE jsonb_typeof(value)
 WHEN 'object' THEN
  SELECT '{'||coalesce(string_agg(to_jsonb(key)::text||':'||work_review_canonical(val),',' ORDER BY key COLLATE "C"),'')||'}'
   INTO result FROM jsonb_each(value) AS item(key,val);
 WHEN 'array' THEN
  SELECT '['||coalesce(string_agg(work_review_canonical(val),',' ORDER BY ord),'')||']'
   INTO result FROM jsonb_array_elements(value) WITH ORDINALITY AS item(val,ord);
 ELSE result:=value::text;
 END CASE;
 RETURN result;
END;
$$;
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM PUBLIC;
REVOKE ALL ON FUNCTION work_review_canonical(jsonb) FROM PUBLIC;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
COMMIT;
