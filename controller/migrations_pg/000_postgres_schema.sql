-- PostgreSQL baseline for machina-controller: the same schema as SQLite migrations 000-060.
-- Drafted by scripts/db/sqlite_to_pg_schema.py and reviewed by hand. From migration 061 on, every change is
-- written twice: controller/migrations (SQLite) and controller/migrations_pg (here).

-- SQLite compatibility functions for the PostgreSQL build of machina-controller.
--
-- The controller's SQL was written for SQLite and uses its date and JSON helpers. Defining the same names here lets almost all
-- of that SQL run unchanged on PostgreSQL; what these cannot cover is rewritten by controller/src/db/dialect.rs (placeholders,
-- CURRENT_TIMESTAMP, INSERT OR IGNORE, LIKE, CAST types) or written per backend at the call site.
--
-- Timestamps are TEXT in UTC in the formats the controller already writes: 'YYYY-MM-DD HH24:MI:SS' (CURRENT_TIMESTAMP, datetime)
-- and ISO 'YYYY-MM-DDTHH24:MI:SS.mmmZ'. Both parse here, so comparisons between stored values and datetime('now', ...) keep working.

-- parse either stored form (or 'now') into a timestamp
CREATE FUNCTION sqlite_ts(v text) RETURNS timestamp LANGUAGE sql STABLE AS $$
  SELECT CASE WHEN lower($1) = 'now' THEN now() AT TIME ZONE 'utc'
              WHEN $1 IS NULL OR $1 = '' THEN NULL
              ELSE replace(replace($1, 'T', ' '), 'Z', '')::timestamp END
$$;

-- SQLite date modifiers such as '-5 minutes', '+30 seconds', '-1 hour', '+2 days'
CREATE FUNCTION sqlite_ts_modify(ts timestamp, modifier text) RETURNS timestamp LANGUAGE plpgsql STABLE AS $$
DECLARE m text[];
BEGIN
  IF ts IS NULL OR modifier IS NULL THEN RETURN NULL; END IF;
  m := regexp_match(trim(modifier), '^([+-]?[0-9]+(?:\.[0-9]+)?)\s*([a-zA-Z]+)$');
  IF m IS NULL THEN RAISE EXCEPTION 'unsupported date modifier: %', modifier; END IF;
  RETURN ts + (m[1] || ' ' || CASE lower(m[2])
    WHEN 's' THEN 'seconds' WHEN 'sec' THEN 'seconds' WHEN 'secs' THEN 'seconds' WHEN 'second' THEN 'seconds' WHEN 'seconds' THEN 'seconds'
    WHEN 'm' THEN 'minutes' WHEN 'min' THEN 'minutes' WHEN 'mins' THEN 'minutes' WHEN 'minute' THEN 'minutes' WHEN 'minutes' THEN 'minutes'
    WHEN 'h' THEN 'hours' WHEN 'hour' THEN 'hours' WHEN 'hours' THEN 'hours'
    WHEN 'd' THEN 'days' WHEN 'day' THEN 'days' WHEN 'days' THEN 'days'
    ELSE m[2] END)::interval;
END
$$;

CREATE FUNCTION datetime() RETURNS text LANGUAGE sql STABLE AS $$ SELECT to_char(sqlite_ts('now'), 'YYYY-MM-DD HH24:MI:SS') $$;
CREATE FUNCTION datetime(v text) RETURNS text LANGUAGE sql STABLE AS $$ SELECT to_char(sqlite_ts($1), 'YYYY-MM-DD HH24:MI:SS') $$;
CREATE FUNCTION datetime(v text, modifier text) RETURNS text LANGUAGE sql STABLE AS
  $$ SELECT to_char(sqlite_ts_modify(sqlite_ts($1), $2), 'YYYY-MM-DD HH24:MI:SS') $$;

-- julianday(): days since noon, 24 Nov 4714 BC; the controller only subtracts two of them
CREATE FUNCTION julianday(v text) RETURNS double precision LANGUAGE sql STABLE AS
  $$ SELECT extract(epoch FROM sqlite_ts($1))::double precision / 86400.0 + 2440587.5 $$;

-- strftime() for the formats the controller uses; anything else fails loudly instead of returning something plausible
CREATE FUNCTION strftime(fmt text, v text) RETURNS text LANGUAGE plpgsql STABLE AS $$
DECLARE t timestamp := sqlite_ts(v);
BEGIN
  IF t IS NULL THEN RETURN NULL; END IF;
  RETURN CASE fmt
    WHEN '%s' THEN floor(extract(epoch FROM t))::bigint::text
    WHEN '%Y-%m-%dT%H:%M:%SZ' THEN to_char(t, 'YYYY-MM-DD"T"HH24:MI:SS"Z"')
    WHEN '%Y-%m-%dT%H:%M:%fZ' THEN to_char(t, 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
    WHEN '%Y-%m-%d %H:%M:%S' THEN to_char(t, 'YYYY-MM-DD HH24:MI:SS')
    WHEN '%Y-%m-%d' THEN to_char(t, 'YYYY-MM-DD')
    ELSE NULL END
  || '';
END
$$;
CREATE FUNCTION strftime(fmt text, v text, modifier text) RETURNS text LANGUAGE sql STABLE AS
  $$ SELECT strftime($1, to_char(sqlite_ts_modify(sqlite_ts($2), $3), 'YYYY-MM-DD HH24:MI:SS')) $$;

-- hex() of a 16-byte id; callers write lower(hex(id)) to compare against a hex string
CREATE FUNCTION hex(v uuid) RETURNS text LANGUAGE sql IMMUTABLE AS $$ SELECT upper(replace($1::text, '-', '')) $$;

-- printf('-%d hours', n)
CREATE FUNCTION printf(fmt text, a bigint) RETURNS text LANGUAGE sql IMMUTABLE AS $$ SELECT format(replace($1, '%d', '%s'), $2) $$;
CREATE FUNCTION printf(fmt text, a integer) RETURNS text LANGUAGE sql IMMUTABLE AS $$ SELECT format(replace($1, '%d', '%s'), $2) $$;

-- JSON stored as TEXT, queried the SQLite way. Paths are the simple forms the controller uses: $.a.b and $.a[0].
CREATE FUNCTION json_extract(doc text, path text) RETURNS text LANGUAGE sql IMMUTABLE AS $$
  SELECT (doc::jsonb #>> string_to_array(regexp_replace(regexp_replace(substr($2, 3), '\[([0-9]+)\]', '.\1', 'g'), '^\.', ''), '.'))
$$;
CREATE FUNCTION json_each(doc text) RETURNS TABLE(key text, value text, type text) LANGUAGE sql IMMUTABLE AS $$
  SELECT (t.ord - 1)::text, CASE WHEN jsonb_typeof(t.e) = 'string' THEN t.e #>> '{}' ELSE t.e::text END, jsonb_typeof(t.e)
    FROM jsonb_array_elements(CASE WHEN jsonb_typeof($1::jsonb) = 'array' THEN $1::jsonb ELSE '[]'::jsonb END) WITH ORDINALITY AS t(e, ord)
  UNION ALL
  SELECT o.key, CASE WHEN jsonb_typeof(o.value) = 'string' THEN o.value #>> '{}' ELSE o.value::text END, jsonb_typeof(o.value)
    FROM jsonb_each(CASE WHEN jsonb_typeof($1::jsonb) = 'object' THEN $1::jsonb ELSE '{}'::jsonb END) AS o
$$;

-- SQLite's two-argument max()/min() are scalar functions (the larger/smaller of two values); PostgreSQL spells them GREATEST/LEAST
CREATE FUNCTION max(a bigint, b bigint) RETURNS bigint LANGUAGE sql IMMUTABLE AS $$ SELECT GREATEST($1, $2) $$;
CREATE FUNCTION max(a double precision, b double precision) RETURNS double precision LANGUAGE sql IMMUTABLE AS $$ SELECT GREATEST($1, $2) $$;
CREATE FUNCTION min(a bigint, b bigint) RETURNS bigint LANGUAGE sql IMMUTABLE AS $$ SELECT LEAST($1, $2) $$;
CREATE FUNCTION min(a double precision, b double precision) RETURNS double precision LANGUAGE sql IMMUTABLE AS $$ SELECT LEAST($1, $2) $$;

-- json_insert(tags, '$[#]', 'x'): append to a JSON array stored as text (the only form the controller uses)
CREATE FUNCTION json_insert(doc text, path text, val text) RETURNS text LANGUAGE plpgsql IMMUTABLE AS $$
BEGIN
  IF path <> '$[#]' THEN RAISE EXCEPTION 'unsupported json path: %', path; END IF;
  RETURN (COALESCE(doc, '[]')::jsonb || to_jsonb(val))::text;
END
$$;

-- SQLite's typeof() and length() on a stored id: ids are 16-byte BLOBs there (`typeof(id) = 'blob' AND length(id) = 16`); a uuid
-- column answers the same way here
CREATE FUNCTION typeof(v uuid) RETURNS text LANGUAGE sql IMMUTABLE AS $$ SELECT 'blob'::text $$;
CREATE FUNCTION typeof(v text) RETURNS text LANGUAGE sql IMMUTABLE AS $$ SELECT 'text'::text $$;
CREATE FUNCTION length(v uuid) RETURNS integer LANGUAGE sql IMMUTABLE AS $$ SELECT 16 $$;

-- the current time as the controller stores it (CURRENT_TIMESTAMP in SQL text is rewritten to this)
CREATE FUNCTION machina_now() RETURNS text LANGUAGE sql STABLE AS $$ SELECT datetime() $$;

-- hosts.agent_grpc_addr must not be downgraded from a routable value to loopback/empty while the host address is routable
-- (the SQLite version is migration 005; a BEFORE trigger can fix NEW in place, so no second UPDATE or recursion guard is needed)
CREATE FUNCTION preserve_routable_agent_addr() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF (NEW.agent_grpc_addr ILIKE '127.0.0.1:%' OR NEW.agent_grpc_addr ILIKE 'localhost:%' OR NEW.agent_grpc_addr = '')
     AND OLD.agent_grpc_addr NOT ILIKE '127.0.0.1:%' AND OLD.agent_grpc_addr NOT ILIKE 'localhost:%' AND OLD.agent_grpc_addr <> ''
     AND COALESCE(NEW.address, '') NOT ILIKE '127.%' AND COALESCE(NEW.address, '') <> 'localhost' AND COALESCE(NEW.address, '') <> ''
  THEN
    NEW.agent_grpc_addr := OLD.agent_grpc_addr;
  END IF;
  RETURN NEW;
END
$$;

CREATE TABLE ai_actions (
    id UUID NOT NULL PRIMARY KEY,
    source TEXT NOT NULL DEFAULT 'zeus',
    action_type TEXT NOT NULL,
    label TEXT NOT NULL,
    review TEXT NOT NULL DEFAULT '',
    risk TEXT NOT NULL DEFAULT 'Review required',
    object_ref TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'pending',
    requested_by TEXT NOT NULL DEFAULT '',
    approved_by TEXT,
    executed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    before_state TEXT NOT NULL DEFAULT '{}',
    verify_result TEXT NOT NULL DEFAULT '{}',
    undone_at TEXT
);

CREATE TABLE ai_agent_plugins (
    slug TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    agent_id TEXT NOT NULL,
    config_schema_json TEXT NOT NULL DEFAULT '{}',
    installed BOOLEAN NOT NULL DEFAULT FALSE,
    published_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE ai_conversations (
    id UUID NOT NULL PRIMARY KEY,
    user_id TEXT NOT NULL,
    project_id TEXT NOT NULL DEFAULT '',
    agent_id TEXT NOT NULL DEFAULT 'auto',
    summary TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE ai_incidents (
    id UUID NOT NULL PRIMARY KEY,
    title TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    severity TEXT NOT NULL DEFAULT 'medium',
    status TEXT NOT NULL DEFAULT 'open',
    affected_resources TEXT NOT NULL DEFAULT '[]',
    root_cause TEXT,
    evidence_json TEXT NOT NULL DEFAULT '{}',
    window_start TEXT,
    window_end TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE ai_memory_entries (
    id UUID NOT NULL PRIMARY KEY,
    scope TEXT NOT NULL DEFAULT 'user',
    subject_kind TEXT NOT NULL DEFAULT 'conversation',
    subject_id TEXT NOT NULL DEFAULT '',
    owner_id TEXT NOT NULL DEFAULT '',
    project_id TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE ai_models (
    id UUID NOT NULL PRIMARY KEY,
    provider_id UUID NOT NULL,
    model_id TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    capabilities_json TEXT NOT NULL DEFAULT '{}',
    context_window BIGINT NOT NULL DEFAULT 128000,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE (provider_id, model_id)
);

CREATE TABLE ai_prompts (
    id UUID NOT NULL PRIMARY KEY,
    scope TEXT NOT NULL DEFAULT 'personal',
    owner_id TEXT NOT NULL DEFAULT '',
    team_id TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]',
    agent_id TEXT NOT NULL DEFAULT 'auto',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE ai_providers (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'openai',
    base_url TEXT NOT NULL DEFAULT '',
    org_id TEXT NOT NULL DEFAULT '',
    deployment_name TEXT NOT NULL DEFAULT '',
    api_key_encrypted TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE ai_routing_rules (
    id UUID NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
    task_class TEXT NOT NULL UNIQUE,
    provider_id UUID,
    model_id UUID,
    priority BIGINT NOT NULL DEFAULT 0,
    enabled BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE ai_trust_rules (
    action_type TEXT NOT NULL PRIMARY KEY,
    level TEXT NOT NULL DEFAULT 'ask' CHECK (level IN ('ask', 'auto')),
    max_per_run BIGINT NOT NULL DEFAULT 3,
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE ai_user_preferences (
    user_id TEXT PRIMARY KEY,
    default_provider_id UUID,
    default_model_id UUID,
    default_agent TEXT NOT NULL DEFAULT 'auto',
    memory_enabled BIGINT NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE air_gap_bundles (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    checksum TEXT NOT NULL DEFAULT '',
    manifest_json TEXT NOT NULL DEFAULT '{}',
    size_bytes BIGINT NOT NULL DEFAULT 0,
    exported_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE alert_rules (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    metric TEXT NOT NULL DEFAULT 'cpu_percent',
    comparator TEXT NOT NULL DEFAULT 'gt',
    threshold DOUBLE PRECISION NOT NULL DEFAULT 90,
    severity TEXT NOT NULL DEFAULT 'warning',
    scope_project TEXT NOT NULL DEFAULT '',
    scope_tag TEXT NOT NULL DEFAULT '',
    cooldown_minutes BIGINT NOT NULL DEFAULT 30,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_fired_at TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE api_keys (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL DEFAULT 'operator',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    last_used_at TEXT,
    projects TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE api_trace_spans (
    id UUID NOT NULL PRIMARY KEY,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    status_code BIGINT NOT NULL,
    duration_ms BIGINT NOT NULL,
    recorded_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE application_group_vms (
    group_id UUID NOT NULL,
    vm_id UUID NOT NULL,
    PRIMARY KEY (group_id, vm_id)
);

CREATE TABLE application_groups (
    id UUID NOT NULL PRIMARY KEY,
    cluster_id UUID NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(cluster_id, name)
);

CREATE TABLE audit_logs (
    id UUID NOT NULL PRIMARY KEY,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT,
    resource_id UUID,
    detail TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE backup_records (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID NOT NULL,
    backup_type TEXT NOT NULL DEFAULT 'full',
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    backup_path TEXT NOT NULL DEFAULT '',
    restore_status TEXT NOT NULL DEFAULT '',
    verified_at TEXT,
    verify_status TEXT NOT NULL DEFAULT '',
    verify_message TEXT
);

CREATE TABLE backup_schedules (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    project TEXT NOT NULL DEFAULT '',
    tag_filter TEXT NOT NULL DEFAULT '',
    backup_type TEXT NOT NULL DEFAULT 'full',
    target_id UUID,
    interval_hours BIGINT NOT NULL DEFAULT 24,
    retain_count BIGINT NOT NULL DEFAULT 7,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE backup_targets (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL DEFAULT 'local',
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE baremetal_servers (
    id UUID NOT NULL PRIMARY KEY,
    hostname TEXT NOT NULL,
    bmc_address TEXT NOT NULL DEFAULT '',
    bmc_type TEXT NOT NULL DEFAULT 'redfish',
    state TEXT NOT NULL DEFAULT 'discovered',
    cpu_cores BIGINT NOT NULL DEFAULT 0,
    memory_mib BIGINT NOT NULL DEFAULT 0,
    tags TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    firewall_profile TEXT NOT NULL DEFAULT 'BareMetalBmc',
    firewall_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    bmc_vlan TEXT NOT NULL DEFAULT '',
    pxe_vlan TEXT NOT NULL DEFAULT '',
    posture_json TEXT,
    last_exposure_scan_at TEXT
);

CREATE TABLE blueprints (
    id UUID NOT NULL PRIMARY KEY,
    cluster_id UUID NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    actions TEXT NOT NULL DEFAULT '[]',
    vm_ids TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE (cluster_id, name)
);

CREATE TABLE bpf_policies (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL,
    match_value TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    scope TEXT NOT NULL DEFAULT 'fleet',
    description TEXT NOT NULL DEFAULT '',
    applied_hosts TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE channel_deliveries (
    id UUID NOT NULL PRIMARY KEY,
    channel_id UUID NOT NULL,
    kind TEXT NOT NULL,
    target TEXT NOT NULL,
    subject TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    event_kind TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    attempts BIGINT NOT NULL DEFAULT 0,
    max_attempts BIGINT NOT NULL DEFAULT 6,
    next_retry_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    last_error TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    claimed_by TEXT
);

CREATE TABLE chaos_experiments (
    id UUID PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    spec_json TEXT NOT NULL,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE chaos_runs (
    id UUID PRIMARY KEY NOT NULL,
    experiment_id UUID NOT NULL,
    status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running','passed','failed','aborted','interrupted')),
    started_by TEXT NOT NULL DEFAULT '',
    started_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    finished_at TEXT,
    abort_reason TEXT NOT NULL DEFAULT '',
    report_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE cloud_alarms (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    subject TEXT NOT NULL,
    metric TEXT NOT NULL,
    statistic TEXT NOT NULL DEFAULT 'Average',
    period_secs BIGINT NOT NULL DEFAULT 300,
    evaluation_periods BIGINT NOT NULL DEFAULT 1,
    comparator TEXT NOT NULL DEFAULT 'gt',
    threshold DOUBLE PRECISION NOT NULL,
    state TEXT NOT NULL DEFAULT 'INSUFFICIENT_DATA',
    state_reason TEXT NOT NULL DEFAULT '',
    state_updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    action TEXT NOT NULL DEFAULT 'none',
    group_id UUID,
    step BIGINT NOT NULL DEFAULT 0,
    cooldown_secs BIGINT NOT NULL DEFAULT 300,
    last_action_at TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE cloud_group_members (
    group_id UUID NOT NULL,
    slot BIGINT NOT NULL CHECK(slot >= 0),
    vm_id UUID UNIQUE,
    draining_since TEXT,
    PRIMARY KEY(group_id, slot)
);

CREATE TABLE cloud_instance_groups (
    id UUID PRIMARY KEY NOT NULL,
    project_id UUID NOT NULL,
    template_id UUID NOT NULL,
    subnet_id UUID NOT NULL,
    name TEXT NOT NULL,
    policy_json TEXT NOT NULL,
    paused BOOLEAN NOT NULL DEFAULT FALSE,
    last_scaled_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    last_error TEXT NOT NULL DEFAULT '',
    UNIQUE(project_id, name)
);

CREATE TABLE cloud_ip_allocations (
    id UUID PRIMARY KEY NOT NULL,
    subnet_id UUID NOT NULL,
    request_key TEXT NOT NULL,
    address TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(subnet_id, request_key),
    UNIQUE(subnet_id, address)
);

CREATE TABLE cloud_launch_templates (
    id UUID PRIMARY KEY NOT NULL,
    project_id UUID NOT NULL,
    name TEXT NOT NULL,
    spec_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(project_id, name)
);

CREATE TABLE cloud_peerings (
    id UUID PRIMARY KEY NOT NULL,
    requester_id UUID NOT NULL,
    accepter_id UUID NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending_acceptance' CHECK(status IN ('pending_acceptance','planned')),
    CHECK(requester_id != accepter_id),
    UNIQUE(requester_id, accepter_id)
);

CREATE TABLE cloud_routes (
    id UUID PRIMARY KEY NOT NULL,
    vpc_id UUID NOT NULL,
    destination TEXT NOT NULL,
    target TEXT NOT NULL CHECK(target IN ('blackhole','local','peering')),
    target_id UUID,
    UNIQUE(vpc_id, destination)
);

CREATE TABLE cloud_subnets (
    id UUID PRIMARY KEY NOT NULL,
    vpc_id UUID NOT NULL,
    network_id UUID NOT NULL UNIQUE,
    name TEXT NOT NULL,
    cidr TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','ready','error')),
    last_error TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    nat_enabled BIGINT NOT NULL DEFAULT 0,
    UNIQUE(vpc_id, name),
    UNIQUE(vpc_id, cidr)
);

CREATE TABLE cloud_vpcs (
    id UUID PRIMARY KEY NOT NULL,
    project_id UUID NOT NULL,
    host_id UUID NOT NULL,
    name TEXT NOT NULL,
    cidr TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(project_id, name)
);

CREATE TABLE clusters (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    ha_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    placement_policy TEXT NOT NULL DEFAULT 'balanced',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    drs_auto_migrate BOOLEAN NOT NULL DEFAULT FALSE,
    drs_cpu_threshold DOUBLE PRECISION NOT NULL DEFAULT 85.0,
    oidc_enabled BIGINT NOT NULL DEFAULT 0,
    oidc_issuer TEXT NOT NULL DEFAULT '',
    oidc_client_id TEXT NOT NULL DEFAULT '',
    cpu_compat_matrix TEXT NOT NULL DEFAULT '[]',
    oidc_client_secret TEXT NOT NULL DEFAULT '',
    oidc_redirect_uri TEXT NOT NULL DEFAULT '',
    inventory_sync_interval_secs BIGINT NOT NULL DEFAULT 30,
    require_vm_delete_approval BOOLEAN NOT NULL DEFAULT FALSE,
    finops_vcpu_hour_usd DOUBLE PRECISION NOT NULL DEFAULT 0.02,
    finops_gib_hour_usd DOUBLE PRECISION NOT NULL DEFAULT 0.005,
    ai_enabled BIGINT NOT NULL DEFAULT 0,
    ai_mode TEXT NOT NULL DEFAULT 'advisor',
    ai_provider TEXT NOT NULL DEFAULT 'openai',
    ai_model TEXT NOT NULL DEFAULT 'gpt-4o-mini',
    ai_api_key TEXT NOT NULL DEFAULT '',
    ai_autopilot_interval_secs BIGINT NOT NULL DEFAULT 0,
    ai_autopilot_last_run TEXT,
    ai_autopilot_max_actions BIGINT NOT NULL DEFAULT 5,
    ai_fleet_peer_urls TEXT NOT NULL DEFAULT '[]',
    firewall_approval_sla_hours BIGINT NOT NULL DEFAULT 72,
    inventory_prune_unmanaged BOOLEAN NOT NULL DEFAULT TRUE,
    inventory_mark_managed_missing BOOLEAN NOT NULL DEFAULT TRUE,
    zeus_multi_provider BIGINT NOT NULL DEFAULT 1,
    zeus_agents_enabled BIGINT NOT NULL DEFAULT 1,
    zeus_ambient_ux BIGINT NOT NULL DEFAULT 1,
    zeus_memory_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    zeus_memory_team_scope BOOLEAN NOT NULL DEFAULT FALSE,
    zeus_memory_project_scope BOOLEAN NOT NULL DEFAULT TRUE,
    zeus_memory_retention_days BIGINT NOT NULL DEFAULT 90,
    zeus_air_gap_llm BOOLEAN NOT NULL DEFAULT FALSE,
    ha_allow_unfenced_recovery BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE console_access_requests (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID NOT NULL,
    requester TEXT NOT NULL,
    requester_user_id UUID,
    protocol TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    approved_by TEXT,
    approved_at TEXT,
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE console_sessions (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID NOT NULL,
    host_id UUID,
    actor TEXT NOT NULL DEFAULT '',
    actor_user_id UUID,
    protocol TEXT NOT NULL,
    backend TEXT NOT NULL DEFAULT 'native',
    guac_token TEXT,
    agent_proxy_base TEXT NOT NULL DEFAULT '',
    emergency_url TEXT,
    started_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    ended_at TEXT,
    expires_at TEXT NOT NULL,
    audit_id TEXT,
    recording_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    recording_path TEXT,
    spectator_token TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE content_images (
    id UUID NOT NULL PRIMARY KEY,
    cluster_id UUID NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'iso',
    path TEXT NOT NULL,
    size_gib BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'available',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    category TEXT NOT NULL DEFAULT 'Custom Appliances',
    description TEXT NOT NULL DEFAULT '',
    submitted_by TEXT,
    approved_by TEXT,
    approved_at TEXT,
    rejected_reason TEXT,
    checksum TEXT,
    UNIQUE(cluster_id, name)
);

CREATE TABLE controller_leadership (
    id BIGINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    holder_id TEXT NOT NULL DEFAULT '',
    lease_until TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    epoch BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE ec2_access_keys (
    access_key_id TEXT NOT NULL PRIMARY KEY,
    secret_enc TEXT NOT NULL,
    username TEXT NOT NULL,
    role TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    last_used_at TEXT,
    revoked BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE eip_pools (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    cidr TEXT NOT NULL,
    host_id UUID NOT NULL,
    interface TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE elastic_ips (
    id UUID NOT NULL PRIMARY KEY,
    pool_id UUID NOT NULL,
    address TEXT NOT NULL UNIQUE,
    vm_id UUID,
    allocated_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    associated_at TEXT
);

CREATE TABLE enrollment_tokens (
    token TEXT PRIMARY KEY,
    cluster_id UUID,
    expires_at TEXT,
    used_at TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE events (
    id UUID NOT NULL PRIMARY KEY,
    kind TEXT NOT NULL,
    resource_type TEXT,
    resource_id UUID,
    message TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE fence_events (
    id UUID NOT NULL PRIMARY KEY,
    host_id UUID NOT NULL,
    action TEXT NOT NULL,
    command TEXT,
    success BOOLEAN NOT NULL DEFAULT FALSE,
    message TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE fips_crypto_profiles (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    tls_min_version TEXT NOT NULL DEFAULT '1.2',
    fips_mode TEXT NOT NULL DEFAULT 'disabled',
    cipher_suites TEXT NOT NULL DEFAULT 'system-default',
    notes TEXT NOT NULL DEFAULT ''
);

CREATE TABLE firewall_approvals (
    id UUID NOT NULL PRIMARY KEY,
    target_kind TEXT NOT NULL DEFAULT 'host',
    target_id UUID NOT NULL,
    profile TEXT,
    plan_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'pending',
    requested_by TEXT NOT NULL,
    reviewed_by TEXT,
    review_note TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    reviewed_at TEXT
);

CREATE TABLE firewall_checkpoints (
    id UUID NOT NULL PRIMARY KEY,
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    label TEXT NOT NULL DEFAULT 'rollback',
    adapter_state TEXT NOT NULL DEFAULT '{}',
    created_by TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_cloud_snapshots (
    id UUID NOT NULL PRIMARY KEY,
    provider TEXT NOT NULL,
    summary TEXT NOT NULL,
    inventory_json TEXT NOT NULL DEFAULT '{}',
    captured_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_connectivity_runs (
    id UUID NOT NULL PRIMARY KEY,
    target_id UUID NOT NULL,
    profile TEXT NOT NULL,
    matrix_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_k8s_apply_log (
    id UUID NOT NULL PRIMARY KEY,
    namespace TEXT NOT NULL,
    profile TEXT NOT NULL,
    backend TEXT NOT NULL,
    actor TEXT,
    detail_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_policies (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    spec_yaml TEXT NOT NULL,
    workspace_id UUID,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    source TEXT NOT NULL DEFAULT 'manual'
);

CREATE TABLE firewall_policy_reconcile_log (
    id UUID NOT NULL PRIMARY KEY,
    policies_synced BIGINT NOT NULL DEFAULT 0,
    detail_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_policy_sync_log (
    id UUID NOT NULL PRIMARY KEY,
    direction TEXT NOT NULL,
    policy_count BIGINT NOT NULL DEFAULT 0,
    actor TEXT,
    detail_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_posture_snapshots (
    id UUID NOT NULL PRIMARY KEY,
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    checksum TEXT NOT NULL,
    posture_json TEXT NOT NULL DEFAULT '{}',
    captured_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_profiles (
    id UUID NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    spec_json TEXT NOT NULL DEFAULT '{}',
    builtin BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_site_drift (
    id UUID NOT NULL PRIMARY KEY,
    site_id UUID NOT NULL,
    peer_site_id UUID,
    drift_json TEXT NOT NULL DEFAULT '{}',
    captured_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_site_policies (
    id UUID NOT NULL PRIMARY KEY,
    site_id UUID NOT NULL,
    policy_name TEXT NOT NULL,
    profile TEXT NOT NULL DEFAULT 'ProductionServer',
    spec_yaml TEXT NOT NULL DEFAULT '',
    geo_fence TEXT,
    dr_pair TEXT,
    stretch_deny BIGINT NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE (site_id, policy_name)
);

CREATE TABLE firewall_site_timeline (
    id UUID NOT NULL PRIMARY KEY,
    site_id UUID NOT NULL,
    kind TEXT NOT NULL,
    detail_json TEXT NOT NULL DEFAULT '{}',
    actor TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_sites (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    region TEXT NOT NULL DEFAULT 'local',
    role TEXT NOT NULL DEFAULT 'primary',
    gitops_namespace TEXT NOT NULL DEFAULT 'default',
    lockdown_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    geo_fence TEXT,
    dr_pair TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_temporary_rules (
    id UUID NOT NULL PRIMARY KEY,
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    source_cidr TEXT NOT NULL,
    dest_port BIGINT NOT NULL,
    protocol TEXT NOT NULL DEFAULT 'tcp',
    reason TEXT NOT NULL,
    owner TEXT,
    approval_id UUID,
    expires_at TEXT NOT NULL,
    applied BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE firewall_timeline (
    id UUID NOT NULL PRIMARY KEY,
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    detail_json TEXT NOT NULL DEFAULT '{}',
    actor TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE flavors (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    vcpus BIGINT NOT NULL,
    memory_mib BIGINT NOT NULL,
    disk_gib BIGINT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    is_public BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE fleet_snapshot_schedules (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    cron_expr TEXT NOT NULL DEFAULT '0 2 * * *',
    project TEXT NOT NULL DEFAULT '',
    tag_filter TEXT NOT NULL DEFAULT '',
    disk_only BOOLEAN NOT NULL DEFAULT TRUE,
    quiesce BOOLEAN NOT NULL DEFAULT FALSE,
    retain_count BIGINT NOT NULL DEFAULT 5,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE ha_events (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID,
    host_id UUID,
    action TEXT NOT NULL,
    message TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    seq BIGINT GENERATED ALWAYS AS IDENTITY
);

CREATE TABLE ha_policies (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    restart_attempts BIGINT NOT NULL DEFAULT 3,
    restart_priority TEXT NOT NULL DEFAULT 'medium',
    anti_affinity BOOLEAN NOT NULL DEFAULT FALSE,
    fence_on_failure BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE host_lldp_cache (
    host_id UUID PRIMARY KEY,
    source TEXT NOT NULL DEFAULT '',
    neighbors_json TEXT NOT NULL DEFAULT '[]',
    summary TEXT NOT NULL DEFAULT '',
    fetched_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE hosts (
    id UUID NOT NULL PRIMARY KEY,
    cluster_id UUID,
    hostname TEXT NOT NULL,
    address TEXT NOT NULL DEFAULT '',
    agent_version TEXT NOT NULL DEFAULT '',
    libvirt_uri TEXT NOT NULL DEFAULT 'qemu:///system',
    agent_grpc_addr TEXT NOT NULL DEFAULT '127.0.0.1:50051',
    state TEXT NOT NULL DEFAULT 'unknown',
    maintenance_mode BOOLEAN NOT NULL DEFAULT FALSE,
    last_heartbeat_at TEXT,
    cpu_percent DOUBLE PRECISION NOT NULL DEFAULT 0,
    memory_used_mib BIGINT NOT NULL DEFAULT 0,
    memory_total_mib BIGINT NOT NULL DEFAULT 0,
    vm_count BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    agent_console_addr TEXT NOT NULL DEFAULT '127.0.0.1:50052',
    cpu_model TEXT NOT NULL DEFAULT '',
    libvirt_version TEXT NOT NULL DEFAULT '',
    qemu_version TEXT NOT NULL DEFAULT '',
    fenced BOOLEAN NOT NULL DEFAULT FALSE,
    notes TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT '[]',
    fence_method TEXT NOT NULL DEFAULT 'shell',
    ipmi_address TEXT NOT NULL DEFAULT '',
    ipmi_username TEXT NOT NULL DEFAULT '',
    ipmi_password TEXT NOT NULL DEFAULT '',
    validation_status TEXT NOT NULL DEFAULT 'pending',
    validation_report TEXT NOT NULL DEFAULT '[]',
    baremetal_origin_id UUID,
    site TEXT NOT NULL DEFAULT '',
    rack TEXT NOT NULL DEFAULT '',
    rack_u BIGINT,
    guacamole_base_url TEXT NOT NULL DEFAULT '',
    guacamole_json_secret_hex TEXT NOT NULL DEFAULT '',
    schedulable BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE (cluster_id, hostname)
);

CREATE TABLE image_shares (
    template_id UUID NOT NULL,
    project TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    PRIMARY KEY (template_id, project)
);

CREATE TABLE instance_security_groups (
    vm_id UUID NOT NULL,
    sg_id UUID NOT NULL,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    PRIMARY KEY (vm_id, sg_id)
);

CREATE TABLE keypairs (
    id UUID NOT NULL PRIMARY KEY,
    project_id UUID,
    name TEXT NOT NULL UNIQUE,
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE lb_members (
    id UUID NOT NULL PRIMARY KEY,
    load_balancer_id UUID NOT NULL,
    vm_id UUID NOT NULL,
    port BIGINT NOT NULL,
    weight BIGINT NOT NULL DEFAULT 1,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    health TEXT NOT NULL DEFAULT 'unknown',
    health_ok BIGINT NOT NULL DEFAULT 0,
    health_fail BIGINT NOT NULL DEFAULT 0,
    health_detail TEXT NOT NULL DEFAULT '',
    health_changed_at TEXT,
    UNIQUE (load_balancer_id, vm_id, port)
);

CREATE TABLE load_balancers (
    id UUID NOT NULL PRIMARY KEY,
    project_id UUID,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL DEFAULT 'tcp',
    host_id UUID NOT NULL,
    listener_port BIGINT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    status_message TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    hc_protocol TEXT NOT NULL DEFAULT 'none',
    hc_port BIGINT,
    hc_path TEXT NOT NULL DEFAULT '/',
    hc_interval_secs BIGINT NOT NULL DEFAULT 10,
    hc_timeout_secs BIGINT NOT NULL DEFAULT 3,
    hc_healthy_threshold BIGINT NOT NULL DEFAULT 2,
    hc_unhealthy_threshold BIGINT NOT NULL DEFAULT 3,
    hc_last_run TEXT,
    UNIQUE (host_id, protocol, listener_port)
);

CREATE TABLE maintenance_schedules (
    id UUID NOT NULL PRIMARY KEY,
    host_id UUID NOT NULL,
    action TEXT NOT NULL DEFAULT 'enter',
    evacuate BOOLEAN NOT NULL DEFAULT TRUE,
    run_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE maintenance_windows (
    id UUID NOT NULL PRIMARY KEY,
    host_id UUID NOT NULL,
    action TEXT NOT NULL DEFAULT 'enter',
    evacuate BOOLEAN NOT NULL DEFAULT TRUE,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE metric_hourly (
    subject TEXT NOT NULL,
    metric TEXT NOT NULL,
    hour BIGINT NOT NULL,
    avg DOUBLE PRECISION NOT NULL,
    max DOUBLE PRECISION NOT NULL,
    n BIGINT NOT NULL,
    PRIMARY KEY (subject, metric, hour)
);

CREATE TABLE metric_samples (
    subject TEXT NOT NULL,
    metric TEXT NOT NULL,
    ts BIGINT NOT NULL,
    value DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (subject, metric, ts)
);

CREATE TABLE migration_jobs (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID NOT NULL,
    source_host_id UUID NOT NULL,
    dest_host_id UUID NOT NULL,
    live BOOLEAN NOT NULL DEFAULT TRUE,
    status TEXT NOT NULL DEFAULT 'pending',
    precheck TEXT NOT NULL DEFAULT '{}',
    progress BIGINT NOT NULL DEFAULT 0,
    message TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE network_ipam_pools (
    id UUID NOT NULL PRIMARY KEY,
    segment_id UUID NOT NULL,
    cidr TEXT NOT NULL,
    gateway TEXT,
    dns_json TEXT NOT NULL DEFAULT '[]',
    next_offset BIGINT NOT NULL DEFAULT 2,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE network_reservations (
    id UUID NOT NULL PRIMARY KEY,
    network_id UUID NOT NULL,
    vm_id UUID,
    mac_address TEXT,
    ip_address TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    pool_id UUID,
    hostname TEXT
);

CREATE TABLE network_segments (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    tier TEXT NOT NULL DEFAULT 'tier1',
    cidr TEXT NOT NULL,
    east_west_default TEXT NOT NULL DEFAULT 'allow',
    firewall_profile TEXT,
    gitops_namespace TEXT NOT NULL DEFAULT 'network-segments',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE networks (
    id UUID NOT NULL PRIMARY KEY,
    cluster_id UUID,
    name TEXT NOT NULL,
    backend TEXT NOT NULL DEFAULT 'linux-bridge',
    vlan_id BIGINT,
    bridge TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    segment_id UUID,
    UNIQUE (cluster_id, name)
);

CREATE TABLE notification_channels (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    target TEXT NOT NULL,
    events TEXT NOT NULL DEFAULT '["alert.*"]',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE notification_outbox (
    id UUID NOT NULL PRIMARY KEY,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}',
    delivered BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    delivered_at TEXT
);

CREATE TABLE oidc_states (
    state TEXT PRIMARY KEY,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    nonce TEXT
);

CREATE TABLE ops_runbook_catalog (
    id UUID NOT NULL PRIMARY KEY,
    incident TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'incident',
    severity TEXT NOT NULL DEFAULT 'medium',
    auto_trigger TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    last_triggered_at TEXT
);

CREATE TABLE ops_runbook_executions (
    id UUID NOT NULL PRIMARY KEY,
    incident TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'completed',
    steps_json TEXT NOT NULL DEFAULT '[]',
    actor TEXT,
    summary TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE ops_showback_snapshots (
    id UUID NOT NULL PRIMARY KEY,
    project_name TEXT NOT NULL,
    cost_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
    compliance_grade TEXT NOT NULL DEFAULT 'B',
    vm_count BIGINT NOT NULL DEFAULT 0,
    captured_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE placement_recommendations (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID NOT NULL,
    from_host_id UUID NOT NULL,
    to_host_id UUID NOT NULL,
    reason TEXT NOT NULL,
    score DOUBLE PRECISION NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'open',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE platform_plugins (
    id UUID NOT NULL PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'integration',
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    author TEXT NOT NULL DEFAULT 'Zyvor',
    featured BOOLEAN NOT NULL DEFAULT FALSE,
    installed BOOLEAN NOT NULL DEFAULT FALSE,
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE policy_rules (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    rule_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE ports (
    id UUID NOT NULL PRIMARY KEY,
    network_id UUID NOT NULL,
    project_id UUID,
    vm_id UUID,
    mac_address TEXT,
    security_group_id UUID,
    status TEXT NOT NULL DEFAULT 'DOWN',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    subnet_id UUID,
    private_ip TEXT,
    description TEXT NOT NULL DEFAULT '',
    dhcp_pinned BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE preempt_settings (
    id BIGINT PRIMARY KEY CHECK (id = 1),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    reserve_pct BIGINT NOT NULL DEFAULT 10,
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE project_quotas (
    project TEXT PRIMARY KEY,
    max_vms BIGINT NOT NULL DEFAULT 0,
    max_vcpu BIGINT NOT NULL DEFAULT 0,
    max_memory_mib BIGINT NOT NULL DEFAULT 0,
    max_storage_gib BIGINT NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE project_role_assignments (
    id UUID NOT NULL PRIMARY KEY,
    user_id UUID NOT NULL,
    project_id UUID NOT NULL,
    role TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(user_id, project_id, role)
);

CREATE TABLE projects (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE resource_tags (
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    PRIMARY KEY (resource_type, resource_id, key)
);

CREATE TABLE scheduled_jobs (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    operation TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}',
    target_host_id UUID,
    interval_minutes BIGINT NOT NULL DEFAULT 60,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE security_group_rules (
    id UUID NOT NULL PRIMARY KEY,
    security_group_id UUID NOT NULL,
    direction TEXT NOT NULL DEFAULT 'ingress',
    protocol TEXT,
    port_min BIGINT,
    port_max BIGINT,
    remote_cidr TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    remote_sg_id TEXT,
    description TEXT NOT NULL DEFAULT ''
);

CREATE TABLE security_groups (
    id UUID NOT NULL PRIMARY KEY,
    project_id UUID,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    mode TEXT NOT NULL DEFAULT 'audit'
);

CREATE TABLE slo_policies (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    target TEXT NOT NULL,
    objective_pct DOUBLE PRECISION NOT NULL DEFAULT 99.9,
    window_hours BIGINT NOT NULL DEFAULT 720,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE snapshot_records (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    snapshot_path TEXT NOT NULL DEFAULT ''
);

CREATE TABLE soc_alerts (
    id UUID NOT NULL PRIMARY KEY,
    rule_id UUID,
    title TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'medium',
    status TEXT NOT NULL DEFAULT 'open',
    assigned_to TEXT,
    dedupe_key TEXT NOT NULL,
    first_seen TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    last_seen TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    event_count BIGINT NOT NULL DEFAULT 1,
    event_ids TEXT NOT NULL DEFAULT '[]',
    detail_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE soc_detection_rules (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    severity TEXT NOT NULL DEFAULT 'medium',
    query_json TEXT NOT NULL DEFAULT '{}',
    throttle_minutes BIGINT NOT NULL DEFAULT 60,
    builtin BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE soc_event_exports (
    integration_id UUID NOT NULL,
    resource_type TEXT NOT NULL DEFAULT 'event',
    resource_id UUID NOT NULL,
    exported_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    PRIMARY KEY (integration_id, resource_type, resource_id)
);

CREATE TABLE soc_events (
    id UUID NOT NULL PRIMARY KEY,
    occurred_at TEXT NOT NULL,
    source TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'security',
    severity TEXT NOT NULL DEFAULT 'info',
    host_id UUID,
    vm_id UUID,
    actor TEXT,
    summary TEXT NOT NULL,
    ecs_json TEXT NOT NULL DEFAULT '{}',
    raw_ref TEXT NOT NULL DEFAULT '{}',
    dedupe_key TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE soc_forwarder_cursors (
    integration_id UUID NOT NULL,
    cursor_kind TEXT NOT NULL DEFAULT 'events',
    last_occurred_at TEXT,
    last_event_id TEXT,
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    PRIMARY KEY (integration_id, cursor_kind)
);

CREATE TABLE soc_ingest_watermarks (
    source TEXT PRIMARY KEY,
    last_at TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z'
);

CREATE TABLE soc_integrations (
    id UUID NOT NULL PRIMARY KEY,
    integration_type TEXT NOT NULL,
    name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    config_json TEXT NOT NULL DEFAULT '{}',
    last_success_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(integration_type, name)
);

CREATE TABLE soc_playbook_runs (
    id UUID NOT NULL PRIMARY KEY,
    playbook_id UUID NOT NULL,
    alert_id UUID,
    status TEXT NOT NULL DEFAULT 'running',
    step_results TEXT NOT NULL DEFAULT '[]',
    error TEXT,
    started_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    finished_at TEXT
);

CREATE TABLE soc_playbooks (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    trigger_json TEXT NOT NULL DEFAULT '{}',
    steps_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE soc_saved_hunts (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    query_text TEXT NOT NULL,
    schedule_cron TEXT,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    last_run_at TEXT,
    created_by TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE soc_settings (
    id BIGINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    webhook_url TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE stacks (
    id UUID NOT NULL PRIMARY KEY,
    project_id UUID,
    name TEXT NOT NULL,
    template_json TEXT NOT NULL,
    resources_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'creating',
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    drift_json TEXT NOT NULL DEFAULT '{}',
    checked_at TEXT,
    auto_heal BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TEXT,
    previous_template_json TEXT
);

CREATE TABLE storage_backup_sla (
    id UUID NOT NULL PRIMARY KEY,
    pool_id UUID NOT NULL,
    rpo_hours BIGINT NOT NULL DEFAULT 24,
    rto_hours BIGINT NOT NULL DEFAULT 4,
    retention_days BIGINT NOT NULL DEFAULT 30,
    last_backup_at TEXT,
    compliance_grade TEXT NOT NULL DEFAULT 'B',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE (pool_id)
);

CREATE TABLE storage_pools (
    id UUID NOT NULL PRIMARY KEY,
    cluster_id UUID,
    name TEXT NOT NULL,
    storage_class TEXT NOT NULL DEFAULT 'silver',
    backend TEXT NOT NULL DEFAULT 'directory',
    path TEXT,
    capacity_gib BIGINT NOT NULL DEFAULT 0,
    used_gib BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    tier_id UUID,
    UNIQUE (cluster_id, name)
);

CREATE TABLE storage_tiers (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    tier_class TEXT NOT NULL DEFAULT 'silver',
    iops_tier TEXT NOT NULL DEFAULT 'standard',
    replication TEXT NOT NULL DEFAULT 'local',
    snapshot_retention_days BIGINT NOT NULL DEFAULT 7,
    backup_rpo_hours BIGINT NOT NULL DEFAULT 24,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE task_steps (
    id UUID NOT NULL PRIMARY KEY,
    task_id UUID NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE tasks (
    id UUID NOT NULL PRIMARY KEY,
    operation TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    progress BIGINT NOT NULL DEFAULT 0,
    resource_type TEXT,
    resource_id UUID,
    host_id UUID,
    payload TEXT NOT NULL DEFAULT '{}',
    message TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    attempts BIGINT NOT NULL DEFAULT 0,
    claimed_by TEXT,
    seq BIGINT GENERATED ALWAYS AS IDENTITY
);

CREATE TABLE templates (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    source_disk TEXT NOT NULL,
    cloud_init BOOLEAN NOT NULL DEFAULT FALSE,
    os_family TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    category TEXT NOT NULL DEFAULT 'Linux',
    description TEXT NOT NULL DEFAULT '',
    featured BOOLEAN NOT NULL DEFAULT FALSE,
    marketplace BOOLEAN NOT NULL DEFAULT TRUE,
    icon TEXT,
    firewall_profile TEXT,
    workload TEXT NOT NULL DEFAULT '',
    approval_status TEXT NOT NULL DEFAULT 'approved',
    git_ref TEXT NOT NULL DEFAULT '',
    daemon_json_path TEXT NOT NULL DEFAULT '',
    project TEXT NOT NULL DEFAULT '',
    visibility TEXT NOT NULL DEFAULT 'public',
    UNIQUE (name, version)
);

CREATE TABLE tenant_isolation_policies (
    id UUID NOT NULL PRIMARY KEY,
    project_name TEXT NOT NULL UNIQUE,
    network_isolation TEXT NOT NULL DEFAULT 'shared',
    max_vms BIGINT NOT NULL DEFAULT 0,
    max_storage_gib BIGINT NOT NULL DEFAULT 0,
    enforce_quotas BIGINT NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE terminated_instances (
    id UUID NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    instance_type TEXT,
    project TEXT,
    vcpus BIGINT NOT NULL DEFAULT 0,
    memory_mib BIGINT NOT NULL DEFAULT 0,
    terminated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE users (
    id UUID NOT NULL PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'viewer',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE vm_atlas_volumes (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID NOT NULL,
    volume_id TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'data_disk',
    size_bytes BIGINT NOT NULL DEFAULT 0,
    policy TEXT NOT NULL DEFAULT 'general',
    backend_native_id TEXT,
    state TEXT NOT NULL DEFAULT 'provisioning',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE vm_disks (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID NOT NULL,
    name TEXT NOT NULL,
    size_gib BIGINT NOT NULL,
    storage_class TEXT NOT NULL DEFAULT 'silver',
    path TEXT
);

CREATE TABLE vm_forks (
    fork_vm_id UUID PRIMARY KEY,
    source_vm_id UUID NOT NULL,
    restore_point_id UUID,
    memory BIGINT NOT NULL DEFAULT 0,
    isolated BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'))
);

CREATE TABLE vm_metrics (
    vm_id UUID PRIMARY KEY,
    cpu_percent DOUBLE PRECISION NOT NULL DEFAULT 0,
    memory_used_mib BIGINT NOT NULL DEFAULT 0,
    disk_read_iops BIGINT NOT NULL DEFAULT 0,
    disk_write_iops BIGINT NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    net_bytes BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE vm_netpol_host_status (
    host_id UUID NOT NULL PRIMARY KEY,
    hostname TEXT NOT NULL DEFAULT '',
    synced_at TEXT,
    ok BOOLEAN NOT NULL DEFAULT FALSE,
    error TEXT,
    vms BIGINT NOT NULL DEFAULT 0,
    rules BIGINT NOT NULL DEFAULT 0,
    peers BIGINT NOT NULL DEFAULT 0,
    generation BIGINT NOT NULL DEFAULT 0,
    warnings TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE vm_netpol_overlay (
    key TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE vm_netpol_projects (
    project TEXT NOT NULL PRIMARY KEY,
    settings TEXT NOT NULL,
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE vm_netpol_threat_feeds (
    name TEXT NOT NULL PRIMARY KEY,
    source TEXT NOT NULL DEFAULT '',
    block BOOLEAN NOT NULL DEFAULT FALSE,
    domains TEXT NOT NULL DEFAULT '[]',
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE vm_network_policies (
    name TEXT NOT NULL PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'VmNetworkPolicy',
    policy_json TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_by TEXT NOT NULL DEFAULT '',
    generation BIGINT NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE vm_restore_points (
    id UUID PRIMARY KEY,
    vm_id UUID NOT NULL,
    label TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'manual',
    note TEXT,
    layers TEXT NOT NULL DEFAULT '[]',
    quiesced BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')),
    seq BIGINT GENERATED ALWAYS AS IDENTITY
);

CREATE TABLE vm_schedules (
    id UUID NOT NULL PRIMARY KEY,
    vm_id UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN ('start', 'shutdown', 'stop', 'snapshot')),
    interval_minutes BIGINT NOT NULL DEFAULT 1440,
    retention BIGINT,
    label TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    next_run_at TEXT NOT NULL,
    last_run_at TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE vm_sleep_events (
    id BIGINT PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
    vm_id UUID NOT NULL,
    kind TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE vm_sleep_project_policies (
    project TEXT PRIMARY KEY,
    sleep_after_minutes BIGINT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE vm_watchdog (
    vm_id UUID NOT NULL PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    failure_threshold_secs BIGINT NOT NULL DEFAULT 120,
    cooldown_secs BIGINT NOT NULL DEFAULT 600,
    max_restarts_per_hour BIGINT NOT NULL DEFAULT 3,
    unhealthy_since TEXT,
    last_restart_at TEXT,
    restarts_this_hour BIGINT NOT NULL DEFAULT 0,
    hour_window_start TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE vms (
    id UUID NOT NULL PRIMARY KEY,
    cluster_id UUID,
    host_id UUID,
    name TEXT NOT NULL,
    project TEXT,
    spec_json TEXT NOT NULL DEFAULT '{}',
    desired_state TEXT NOT NULL DEFAULT 'running',
    observed_state TEXT NOT NULL DEFAULT 'unknown',
    uuid TEXT,
    vcpus BIGINT NOT NULL DEFAULT 1,
    memory_mib BIGINT NOT NULL DEFAULT 1024,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    ha_recovery_count BIGINT NOT NULL DEFAULT 0,
    tags TEXT NOT NULL DEFAULT '[]',
    lifecycle_phase TEXT NOT NULL DEFAULT 'idle',
    last_error TEXT NOT NULL DEFAULT '',
    managed BOOLEAN NOT NULL DEFAULT TRUE,
    guest_tools_status TEXT NOT NULL DEFAULT 'unknown',
    guest_ip TEXT,
    guest_hostname TEXT,
    os_family TEXT,
    inventory_source TEXT NOT NULL DEFAULT 'libvirt',
    last_seen_at TEXT,
    k8s_namespace TEXT,
    k8s_uid TEXT,
    labels TEXT NOT NULL DEFAULT '{}',
    guest_ips TEXT NOT NULL DEFAULT '[]',
    sleep_after_minutes BIGINT,
    last_active_at TEXT,
    slept_at TEXT,
    restore_point_minutes BIGINT,
    restore_point_keep BIGINT,
    flavor_id UUID,
    preemptible BOOLEAN NOT NULL DEFAULT FALSE,
    preempt_priority BIGINT NOT NULL DEFAULT 0,
    preempted_at TEXT
);

CREATE TABLE volume_snapshots (
    id UUID NOT NULL PRIMARY KEY,
    volume_id UUID NOT NULL,
    name TEXT NOT NULL,
    atlas_snapshot_id TEXT,
    status TEXT NOT NULL DEFAULT 'available',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE volumes (
    id UUID NOT NULL PRIMARY KEY,
    project_id UUID,
    name TEXT NOT NULL,
    size_gib BIGINT NOT NULL,
    volume_class TEXT NOT NULL DEFAULT 'silver',
    storage_pool_id UUID,
    path TEXT,
    atlas_volume_id TEXT,
    status TEXT NOT NULL DEFAULT 'creating',
    attached_vm_id UUID,
    attached_device TEXT,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    delete_on_termination BOOLEAN NOT NULL DEFAULT FALSE,
    read_iops BIGINT,
    write_iops BIGINT,
    read_bps BIGINT,
    write_bps BIGINT
);

CREATE TABLE webhook_deliveries (
    id UUID NOT NULL PRIMARY KEY,
    webhook_id UUID,
    url TEXT NOT NULL,
    secret TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL,
    event_kind TEXT NOT NULL DEFAULT '',
    attempts BIGINT NOT NULL DEFAULT 0,
    max_attempts BIGINT NOT NULL DEFAULT 5,
    next_retry_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    status TEXT NOT NULL DEFAULT 'pending',
    last_error TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    claimed_by TEXT
);

CREATE TABLE webhooks (
    id UUID NOT NULL PRIMARY KEY,
    url TEXT NOT NULL,
    events TEXT NOT NULL DEFAULT '{}',
    secret TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);

-- rows the SQLite migrations seed (defaults and catalogs)
INSERT INTO ai_agent_plugins (slug, name, description, agent_id, config_schema_json, installed, published_at) VALUES ('aws-expert', 'AWS Expert', 'Cloud architecture and AWS service guidance', 'architect', '{}', FALSE, '2026-10-06 20:13:56');
INSERT INTO ai_agent_plugins (slug, name, description, agent_id, config_schema_json, installed, published_at) VALUES ('azure-expert', 'Azure Expert', 'Azure landing zones and NSG guidance', 'architect', '{}', FALSE, '2026-10-06 20:13:56');
INSERT INTO ai_agent_plugins (slug, name, description, agent_id, config_schema_json, installed, published_at) VALUES ('gcp-expert', 'GCP Expert', 'GCP networking and GKE guidance', 'architect', '{}', FALSE, '2026-10-06 20:13:56');
INSERT INTO ai_agent_plugins (slug, name, description, agent_id, config_schema_json, installed, published_at) VALUES ('linux-expert', 'Linux Expert', 'Host tuning and systemd diagnostics', 'sre', '{}', FALSE, '2026-10-06 20:13:56');
INSERT INTO ai_agent_plugins (slug, name, description, agent_id, config_schema_json, installed, published_at) VALUES ('kubernetes-expert', 'Kubernetes Expert', 'Cluster ops and workload placement', 'kubernetes', '{}', FALSE, '2026-10-06 20:13:56');
INSERT INTO ai_agent_plugins (slug, name, description, agent_id, config_schema_json, installed, published_at) VALUES ('terraform-expert', 'Terraform Expert', 'IaC generation and module guidance', 'architect', '{}', FALSE, '2026-10-06 20:13:56');
INSERT INTO ai_agent_plugins (slug, name, description, agent_id, config_schema_json, installed, published_at) VALUES ('finops-expert', 'FinOps Expert', 'Cost optimization and chargeback', 'cost', '{}', FALSE, '2026-10-06 20:13:56');
INSERT INTO ai_agent_plugins (slug, name, description, agent_id, config_schema_json, installed, published_at) VALUES ('security-expert', 'Security Expert', 'Threat hunting and compliance', 'security', '{}', FALSE, '2026-10-06 20:13:56');
INSERT INTO ai_routing_rules (id, task_class, provider_id, model_id, priority, enabled) VALUES ('5733b5c5-7acc-fc87-f8a6-6bebe92b2e64'::uuid, 'infrastructure', NULL, NULL, 10, TRUE);
INSERT INTO ai_routing_rules (id, task_class, provider_id, model_id, priority, enabled) VALUES ('d6c2b969-996f-4982-9ad8-c0f9a0b4f997'::uuid, 'code_generation', NULL, NULL, 20, TRUE);
INSERT INTO ai_routing_rules (id, task_class, provider_id, model_id, priority, enabled) VALUES ('01f117b7-c439-b081-1a2e-99331cdbe05c'::uuid, 'security_analysis', NULL, NULL, 30, TRUE);
INSERT INTO ai_routing_rules (id, task_class, provider_id, model_id, priority, enabled) VALUES ('61b4b109-6ee4-4b5e-3323-2cb463899365'::uuid, 'research', NULL, NULL, 40, TRUE);
INSERT INTO ai_routing_rules (id, task_class, provider_id, model_id, priority, enabled) VALUES ('f1e41c87-a514-8cff-068d-8a45a17c9963'::uuid, 'long_context', NULL, NULL, 50, TRUE);
INSERT INTO ai_routing_rules (id, task_class, provider_id, model_id, priority, enabled) VALUES ('39f2c048-cbb8-d346-0dcf-0b4f1bd4370d'::uuid, 'fast_local', NULL, NULL, 60, TRUE);
INSERT INTO controller_leadership (id, holder_id, lease_until, updated_at, epoch) VALUES (1, '', '2026-10-06 20:13:56', '2026-10-06 20:13:56', 0);
INSERT INTO fips_crypto_profiles (id, name, tls_min_version, fips_mode, cipher_suites, notes) VALUES ('f1000000-0000-4000-8000-000000000001'::uuid, 'platform-default', '1.2', 'disabled', 'TLS_AES_128_GCM_SHA256,TLS_AES_256_GCM_SHA384', 'Controller TLS via system OpenSSL — FIPS module not selected');
INSERT INTO fips_crypto_profiles (id, name, tls_min_version, fips_mode, cipher_suites, notes) VALUES ('f1000000-0000-4000-8000-000000000002'::uuid, 'fips-ready', '1.2', 'required', 'TLS_AES_256_GCM_SHA384', 'Target profile for FIPS 140-3 validated module rollout');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('6f9fe218-5e94-9ebe-1731-2b98aad98f75'::uuid, 'Public', 'Public', '{"default_inbound":"deny"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('66224a33-8313-1479-0fbd-97b2594ff202'::uuid, 'Private', 'Private', '{"default_inbound":"allow"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('d7405f40-2700-6fee-bd23-c74bdc5f690b'::uuid, 'ProductionServer', 'Production Server', '{"default_inbound":"deny"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('3941f8e2-d15e-8a35-a7d0-2cab9c67cb31'::uuid, 'DatabaseServer', 'Database Server', '{"default_inbound":"deny"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('b7a247d1-141d-dacd-b90b-2c79a175a9ea'::uuid, 'WebServer', 'Web Server', '{"default_inbound":"deny"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('448cd5b0-059d-a7f0-ea5a-8016fcc9e01a'::uuid, 'KubernetesNode', 'Kubernetes Node', '{"default_inbound":"deny"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('5f145c1e-aa88-9416-d623-eeca3597486f'::uuid, 'StorageNode', 'Storage Node', '{"default_inbound":"deny"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('676015bc-cc6b-d56d-1469-b24aee1aee10'::uuid, 'ManagementNode', 'Management Node', '{"default_inbound":"deny"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('ac962565-7a11-7e9c-4855-481d4f7688da'::uuid, 'DevelopmentVm', 'Development VM', '{"default_inbound":"allow"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('38fb799a-8b1e-1c61-f147-666736633b3a'::uuid, 'LockedDown', 'Locked Down', '{"default_inbound":"deny","default_outbound":"deny"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_profiles (id, name, display_name, spec_json, builtin, created_at) VALUES ('4a03a41d-62c0-2a82-f3b4-8f67bc971096'::uuid, 'EmergencyIsolation', 'Emergency Isolation', '{"default_inbound":"deny","default_outbound":"deny","stealth":"emergency"}', TRUE, '2026-10-06 20:13:56');
INSERT INTO firewall_sites (id, name, region, role, gitops_namespace, lockdown_enabled, geo_fence, dr_pair, created_at) VALUES ('10000000-0000-4000-8000-000000000001'::uuid, 'primary-local', 'local', 'primary', 'site-primary', FALSE, NULL, 'dr-replica', '2026-10-06 20:13:56');
INSERT INTO firewall_sites (id, name, region, role, gitops_namespace, lockdown_enabled, geo_fence, dr_pair, created_at) VALUES ('20000000-0000-4000-8000-000000000001'::uuid, 'dr-replica', 'dr', 'replica', 'site-dr', FALSE, NULL, 'primary-local', '2026-10-06 20:13:56');
INSERT INTO network_ipam_pools (id, segment_id, cidr, gateway, dns_json, next_offset, created_at) VALUES ('b1000000-0000-4000-8000-000000000001'::uuid, 'a1000000-0000-4000-8000-000000000001'::uuid, '10.10.0.0/16', '10.10.0.1', '["10.10.0.1"]', 10, '2026-10-06 20:13:56');
INSERT INTO network_ipam_pools (id, segment_id, cidr, gateway, dns_json, next_offset, created_at) VALUES ('b1000000-0000-4000-8000-000000000002'::uuid, 'a1000000-0000-4000-8000-000000000002'::uuid, '172.16.0.0/24', '172.16.0.1', '["172.16.0.1"]', 10, '2026-10-06 20:13:56');
INSERT INTO network_segments (id, name, tier, cidr, east_west_default, firewall_profile, gitops_namespace, created_at) VALUES ('a1000000-0000-4000-8000-000000000001'::uuid, 'prod-tier1', 'tier1', '10.10.0.0/16', 'allow', 'ProductionServer', 'prod-segments', '2026-10-06 20:13:56');
INSERT INTO network_segments (id, name, tier, cidr, east_west_default, firewall_profile, gitops_namespace, created_at) VALUES ('a1000000-0000-4000-8000-000000000002'::uuid, 'dmz-tier0', 'tier0', '172.16.0.0/24', 'deny', 'WebServer', 'dmz-segments', '2026-10-06 20:13:56');
INSERT INTO ops_runbook_catalog (id, incident, title, category, severity, auto_trigger, enabled, created_at, last_triggered_at) VALUES ('f1000000-0000-4000-8000-000000000001'::uuid, 'host_offline', 'Host offline recovery', 'incident', 'high', 'host.state=offline', TRUE, '2026-10-06 20:13:56', NULL);
INSERT INTO ops_runbook_catalog (id, incident, title, category, severity, auto_trigger, enabled, created_at, last_triggered_at) VALUES ('f1000000-0000-4000-8000-000000000002'::uuid, 'backup_failed', 'Backup failure triage', 'incident', 'medium', 'task.failed:backup', TRUE, '2026-10-06 20:13:56', NULL);
INSERT INTO ops_runbook_catalog (id, incident, title, category, severity, auto_trigger, enabled, created_at, last_triggered_at) VALUES ('f1000000-0000-4000-8000-000000000003'::uuid, 'migration_failed', 'Migration failure triage', 'incident', 'medium', 'task.failed:migrate', TRUE, '2026-10-06 20:13:56', NULL);
INSERT INTO ops_runbook_catalog (id, incident, title, category, severity, auto_trigger, enabled, created_at, last_triggered_at) VALUES ('f1000000-0000-4000-8000-000000000004'::uuid, 'firewall_drift', 'Firewall drift remediation', 'compliance', 'high', 'zeus.drift_detected', TRUE, '2026-10-06 20:13:56', NULL);
INSERT INTO ops_runbook_catalog (id, incident, title, category, severity, auto_trigger, enabled, created_at, last_triggered_at) VALUES ('f1000000-0000-4000-8000-000000000005'::uuid, 'storage_full', 'Storage pool capacity', 'capacity', 'critical', 'storage.used_pct>85', TRUE, '2026-10-06 20:13:56', NULL);
INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed, config_json, created_at) VALUES ('c2000000-0000-4000-8000-000000000001'::uuid, 'guestkit', 'GuestKit', 'automation', 'Guest health checks, job runner, and in-VM automation bridge.', '1.0.0', 'Zyvor', TRUE, FALSE, '{}', '2026-10-06 20:13:56');
INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed, config_json, created_at) VALUES ('c2000000-0000-4000-8000-000000000002'::uuid, 'native-bpf', 'Native eBPF', 'observability', 'Kernel-native flows, process telemetry, enforcement, capture and QoS (machina-bpfd).', '1.0.0', 'Zyvor', TRUE, TRUE, '{}', '2026-10-06 20:13:56');
INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed, config_json, created_at) VALUES ('c2000000-0000-4000-8000-000000000003'::uuid, 'hypersdk', 'HyperSDK', 'migration', 'P2V migration assistant and Windows VM discovery.', '1.0.0', 'Zyvor', TRUE, FALSE, '{}', '2026-10-06 20:13:56');
INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed, config_json, created_at) VALUES ('c2000000-0000-4000-8000-000000000004'::uuid, 'zeus-firewall', 'Zeus Firewall', 'security', 'Fleet machine shield, profiles, and connectivity simulation.', '1.0.0', 'Zyvor', TRUE, TRUE, '{}', '2026-10-06 20:13:56');
INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed, config_json, created_at) VALUES ('c2000000-0000-4000-8000-000000000005'::uuid, 'kubevirt-bridge', 'KubeVirt Bridge', 'kubernetes', 'Export libvirt VMs and qcow2 bundles for Kubernetes.', '1.0.0', 'Zyvor', FALSE, FALSE, '{}', '2026-10-06 20:13:56');
INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed, config_json, created_at) VALUES ('c2000000-0000-4000-8000-000000000006'::uuid, 'network-overlay', 'Network Overlay', 'networking', 'NSX-class segments, IPAM pools, and micro-segmentation stubs.', '1.0.0', 'Zyvor', FALSE, TRUE, '{}', '2026-10-06 20:13:56');
INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed, config_json, created_at) VALUES ('c2000000-0000-4000-8000-000000000010'::uuid, 'kasm-workspaces', 'Kasm Workspaces', 'console', 'Disposable browser and isolated desktop labs (Marketplace workload — not core ConsoleHub).', '1.0.0', 'Kasm', FALSE, FALSE, '{}', '2026-10-06 20:13:56');
INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed, config_json, created_at) VALUES ('c2000000-0000-4000-8000-000000000011'::uuid, 'rustdesk', 'RustDesk', 'console', 'TeamViewer-style remote support sessions via Marketplace plugin.', '1.0.0', 'RustDesk', FALSE, FALSE, '{}', '2026-10-06 20:13:56');
INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed, config_json, created_at) VALUES ('c2000000-0000-4000-8000-000000000012'::uuid, 'meshcentral', 'MeshCentral', 'console', 'Remote management and support gateway as optional Marketplace plugin.', '1.0.0', 'MeshCentral', FALSE, FALSE, '{}', '2026-10-06 20:13:56');
INSERT INTO policy_rules (id, name, enabled, rule_json, created_at) VALUES ('00000000-0000-4000-8000-000000000001'::uuid, 'production-ha-required', TRUE, '{"when":{"tags_contains":"production"},"require":{"ha_enabled":true}}', '2026-10-06 20:13:56');
INSERT INTO preempt_settings (id, enabled, reserve_pct, updated_at) VALUES (1, TRUE, 10, '2026-10-06 20:13:56');
INSERT INTO slo_policies (id, name, target, objective_pct, window_hours, description, created_at) VALUES ('a1000000-0000-4000-8000-000000000001'::uuid, 'api-availability', 'controller /api/v1/*', 99.5, 720, 'HTTP 2xx/3xx rate for platform API', '2026-10-06 20:13:56');
INSERT INTO slo_policies (id, name, target, objective_pct, window_hours, description, created_at) VALUES ('a1000000-0000-4000-8000-000000000002'::uuid, 'task-success', 'platform tasks', 98.0, 168, 'Completed vs failed task ratio', '2026-10-06 20:13:56');
INSERT INTO slo_policies (id, name, target, objective_pct, window_hours, description, created_at) VALUES ('a1000000-0000-4000-8000-000000000003'::uuid, 'host-availability', 'online hosts', 99.0, 720, 'Hosts reporting online vs registered', '2026-10-06 20:13:56');
INSERT INTO soc_detection_rules (id, name, description, enabled, severity, query_json, throttle_minutes, builtin, created_at, updated_at) VALUES ('a1000001-0001-4001-8001-000000000001'::uuid, 'critical_anomaly', 'Native eBPF critical or high severity anomaly', TRUE, 'high', '{"type":"match","match":{"source":"machina-bpf","severity":["critical","high"]}}', 30, TRUE, '2026-10-06 20:13:56', '2026-10-06 20:13:56');
INSERT INTO soc_detection_rules (id, name, description, enabled, severity, query_json, throttle_minutes, builtin, created_at, updated_at) VALUES ('a1000001-0001-4001-8001-000000000002'::uuid, 'firewall_deny_spike', 'Three or more firewall deny events in 15 minutes', TRUE, 'medium', '{"type":"threshold","match":{"source":"firewall","category":"firewall"},"window_minutes":15,"min_count":3}', 60, TRUE, '2026-10-06 20:13:56', '2026-10-06 20:13:56');
INSERT INTO soc_detection_rules (id, name, description, enabled, severity, query_json, throttle_minutes, builtin, created_at, updated_at) VALUES ('a1000001-0001-4001-8001-000000000003'::uuid, 'brute_force_ssh', 'Repeated failed SSH or auth audit events', TRUE, 'high', '{"type":"threshold","match":{"source":"audit","ecs.event.action":["auth.failure","login.failed"]},"window_minutes":10,"min_count":5}', 120, TRUE, '2026-10-06 20:13:56', '2026-10-06 20:13:56');
INSERT INTO soc_detection_rules (id, name, description, enabled, severity, query_json, throttle_minutes, builtin, created_at, updated_at) VALUES ('a1000001-0001-4001-8001-000000000004'::uuid, 'new_admin_api_key', 'New API key created by admin actor', TRUE, 'medium', '{"type":"match","match":{"source":"audit","ecs.event.action":["api_key.create","api_keys.create"]}}', 60, TRUE, '2026-10-06 20:13:56', '2026-10-06 20:13:56');
INSERT INTO soc_ingest_watermarks (source, last_at) VALUES ('firewall_timeline', '1970-01-01T00:00:00Z');
INSERT INTO soc_ingest_watermarks (source, last_at) VALUES ('audit_logs', '1970-01-01T00:00:00Z');
INSERT INTO soc_ingest_watermarks (source, last_at) VALUES ('platform_events', '1970-01-01T00:00:00Z');
INSERT INTO soc_ingest_watermarks (source, last_at) VALUES ('machina-bpf', '1970-01-01T00:00:00Z');
INSERT INTO soc_integrations (id, integration_type, name, enabled, config_json, last_success_at, last_error, created_at, updated_at) VALUES ('b2000002-0002-4002-8002-000000000001'::uuid, 'splunk_hec', 'default', FALSE, '{"url":"","token":"","index":"machina","sourcetype_events":"machina:soc:ecs","sourcetype_alerts":"machina:soc:alert","host":""}', NULL, NULL, '2026-10-06 20:13:56', '2026-10-06 20:13:56');
INSERT INTO soc_integrations (id, integration_type, name, enabled, config_json, last_success_at, last_error, created_at, updated_at) VALUES ('b2000002-0002-4002-8002-000000000002'::uuid, 'elastic_bulk', 'default', FALSE, '{"url":"","api_key":"","index":"logs-machina.soc","pipeline":""}', NULL, NULL, '2026-10-06 20:13:56', '2026-10-06 20:13:56');
INSERT INTO soc_integrations (id, integration_type, name, enabled, config_json, last_success_at, last_error, created_at, updated_at) VALUES ('b2000002-0002-4002-8002-000000000003'::uuid, 'sentinel_dcr', 'default', FALSE, '{"dce_endpoint":"","dcr_immutable_id":"","stream_name":"","tenant_id":"","client_id":"","client_secret":""}', NULL, NULL, '2026-10-06 20:13:56', '2026-10-06 20:13:56');
INSERT INTO soc_integrations (id, integration_type, name, enabled, config_json, last_success_at, last_error, created_at, updated_at) VALUES ('b2000002-0002-4002-8002-000000000004'::uuid, 'qradar_rest', 'default', FALSE, '{"url":"","api_token":"","log_source_id":""}', NULL, NULL, '2026-10-06 20:13:56', '2026-10-06 20:13:56');
INSERT INTO soc_playbooks (id, name, description, enabled, trigger_json, steps_json, created_at, updated_at) VALUES ('c3000003-0003-4003-8003-000000000001'::uuid, 'notify_on_critical', 'Webhook notify when critical SOC alert opens', TRUE, '{"min_severity":"high","rule_names":[]}', '[{"type":"webhook","url_from_setting":"soc_webhook_url","body":{"alert_id":"{{alert_id}}","title":"{{title}}","severity":"{{severity}}"}}]', '2026-10-06 20:13:56', '2026-10-06 20:13:56');
INSERT INTO soc_settings (id, webhook_url, updated_at) VALUES (1, '', '2026-10-06 20:13:56');
INSERT INTO storage_tiers (id, name, tier_class, iops_tier, replication, snapshot_retention_days, backup_rpo_hours, description, created_at) VALUES ('d1000000-0000-4000-8000-000000000001'::uuid, 'gold-performance', 'gold', 'nvme', 'sync-mirror', 30, 4, 'Low-latency NVMe tier with synchronous mirror stub', '2026-10-06 20:13:56');
INSERT INTO storage_tiers (id, name, tier_class, iops_tier, replication, snapshot_retention_days, backup_rpo_hours, description, created_at) VALUES ('d1000000-0000-4000-8000-000000000002'::uuid, 'silver-standard', 'silver', 'standard', 'local', 14, 24, 'Default production datastore tier', '2026-10-06 20:13:56');
INSERT INTO storage_tiers (id, name, tier_class, iops_tier, replication, snapshot_retention_days, backup_rpo_hours, description, created_at) VALUES ('d1000000-0000-4000-8000-000000000003'::uuid, 'bronze-archive', 'bronze', 'hdd', 'local', 7, 72, 'Capacity-optimized cold tier', '2026-10-06 20:13:56');
INSERT INTO tenant_isolation_policies (id, project_name, network_isolation, max_vms, max_storage_gib, enforce_quotas, updated_at) VALUES ('10000000-0000-4000-8000-000000000001'::uuid, 'default', 'shared', 0, 0, 0, '2026-10-06 20:13:56');
INSERT INTO tenant_isolation_policies (id, project_name, network_isolation, max_vms, max_storage_gib, enforce_quotas, updated_at) VALUES ('10000000-0000-4000-8000-000000000002'::uuid, 'production', 'segmented', 50, 10240, 1, '2026-10-06 20:13:56');

ALTER TABLE ai_models ADD CONSTRAINT ai_models_provider_id_fk0 FOREIGN KEY (provider_id) REFERENCES ai_providers(id) ON DELETE CASCADE;
ALTER TABLE ai_routing_rules ADD CONSTRAINT ai_routing_rules_provider_id_fk1 FOREIGN KEY (provider_id) REFERENCES ai_providers(id) ON DELETE SET NULL;
ALTER TABLE ai_routing_rules ADD CONSTRAINT ai_routing_rules_model_id_fk2 FOREIGN KEY (model_id) REFERENCES ai_models(id) ON DELETE SET NULL;
ALTER TABLE ai_user_preferences ADD CONSTRAINT ai_user_preferences_default_provider_id_fk3 FOREIGN KEY (default_provider_id) REFERENCES ai_providers(id) ON DELETE SET NULL;
ALTER TABLE ai_user_preferences ADD CONSTRAINT ai_user_preferences_default_model_id_fk4 FOREIGN KEY (default_model_id) REFERENCES ai_models(id) ON DELETE SET NULL;
ALTER TABLE application_group_vms ADD CONSTRAINT application_group_vms_group_id_fk5 FOREIGN KEY (group_id) REFERENCES application_groups(id) ON DELETE CASCADE;
ALTER TABLE application_group_vms ADD CONSTRAINT application_group_vms_vm_id_fk6 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE application_groups ADD CONSTRAINT application_groups_cluster_id_fk7 FOREIGN KEY (cluster_id) REFERENCES clusters(id) ON DELETE CASCADE;
ALTER TABLE backup_records ADD CONSTRAINT backup_records_vm_id_fk8 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE blueprints ADD CONSTRAINT blueprints_cluster_id_fk9 FOREIGN KEY (cluster_id) REFERENCES clusters(id) ON DELETE CASCADE;
ALTER TABLE chaos_runs ADD CONSTRAINT chaos_runs_experiment_id_fk10 FOREIGN KEY (experiment_id) REFERENCES chaos_experiments(id) ON DELETE CASCADE;
ALTER TABLE cloud_group_members ADD CONSTRAINT cloud_group_members_group_id_fk11 FOREIGN KEY (group_id) REFERENCES cloud_instance_groups(id) ON DELETE RESTRICT;
ALTER TABLE cloud_group_members ADD CONSTRAINT cloud_group_members_vm_id_fk12 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE SET NULL;
ALTER TABLE cloud_instance_groups ADD CONSTRAINT cloud_instance_groups_project_id_fk13 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE RESTRICT;
ALTER TABLE cloud_instance_groups ADD CONSTRAINT cloud_instance_groups_template_id_fk14 FOREIGN KEY (template_id) REFERENCES cloud_launch_templates(id) ON DELETE RESTRICT;
ALTER TABLE cloud_instance_groups ADD CONSTRAINT cloud_instance_groups_subnet_id_fk15 FOREIGN KEY (subnet_id) REFERENCES cloud_subnets(id) ON DELETE RESTRICT;
ALTER TABLE cloud_ip_allocations ADD CONSTRAINT cloud_ip_allocations_subnet_id_fk16 FOREIGN KEY (subnet_id) REFERENCES cloud_subnets(id) ON DELETE RESTRICT;
ALTER TABLE cloud_launch_templates ADD CONSTRAINT cloud_launch_templates_project_id_fk17 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE RESTRICT;
ALTER TABLE cloud_peerings ADD CONSTRAINT cloud_peerings_requester_id_fk18 FOREIGN KEY (requester_id) REFERENCES cloud_vpcs(id) ON DELETE RESTRICT;
ALTER TABLE cloud_peerings ADD CONSTRAINT cloud_peerings_accepter_id_fk19 FOREIGN KEY (accepter_id) REFERENCES cloud_vpcs(id) ON DELETE RESTRICT;
ALTER TABLE cloud_routes ADD CONSTRAINT cloud_routes_vpc_id_fk20 FOREIGN KEY (vpc_id) REFERENCES cloud_vpcs(id) ON DELETE CASCADE;
ALTER TABLE cloud_subnets ADD CONSTRAINT cloud_subnets_vpc_id_fk21 FOREIGN KEY (vpc_id) REFERENCES cloud_vpcs(id) ON DELETE RESTRICT;
ALTER TABLE cloud_subnets ADD CONSTRAINT cloud_subnets_network_id_fk22 FOREIGN KEY (network_id) REFERENCES networks(id) ON DELETE RESTRICT;
ALTER TABLE cloud_vpcs ADD CONSTRAINT cloud_vpcs_project_id_fk23 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE RESTRICT;
ALTER TABLE cloud_vpcs ADD CONSTRAINT cloud_vpcs_host_id_fk24 FOREIGN KEY (host_id) REFERENCES hosts(id) ON DELETE RESTRICT;
ALTER TABLE console_access_requests ADD CONSTRAINT console_access_requests_vm_id_fk25 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE console_access_requests ADD CONSTRAINT console_access_requests_requester_user_id_fk26 FOREIGN KEY (requester_user_id) REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE console_sessions ADD CONSTRAINT console_sessions_vm_id_fk27 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE console_sessions ADD CONSTRAINT console_sessions_host_id_fk28 FOREIGN KEY (host_id) REFERENCES hosts(id) ON DELETE SET NULL;
ALTER TABLE console_sessions ADD CONSTRAINT console_sessions_actor_user_id_fk29 FOREIGN KEY (actor_user_id) REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE content_images ADD CONSTRAINT content_images_cluster_id_fk30 FOREIGN KEY (cluster_id) REFERENCES clusters(id) ON DELETE CASCADE;
ALTER TABLE elastic_ips ADD CONSTRAINT elastic_ips_pool_id_fk31 FOREIGN KEY (pool_id) REFERENCES eip_pools(id) ON DELETE RESTRICT;
ALTER TABLE enrollment_tokens ADD CONSTRAINT enrollment_tokens_cluster_id_fk32 FOREIGN KEY (cluster_id) REFERENCES clusters(id);
ALTER TABLE fence_events ADD CONSTRAINT fence_events_host_id_fk33 FOREIGN KEY (host_id) REFERENCES hosts(id);
ALTER TABLE firewall_site_drift ADD CONSTRAINT firewall_site_drift_site_id_fk34 FOREIGN KEY (site_id) REFERENCES firewall_sites(id) ON DELETE CASCADE;
ALTER TABLE firewall_site_drift ADD CONSTRAINT firewall_site_drift_peer_site_id_fk35 FOREIGN KEY (peer_site_id) REFERENCES firewall_sites(id) ON DELETE SET NULL;
ALTER TABLE firewall_site_policies ADD CONSTRAINT firewall_site_policies_site_id_fk36 FOREIGN KEY (site_id) REFERENCES firewall_sites(id) ON DELETE CASCADE;
ALTER TABLE firewall_site_timeline ADD CONSTRAINT firewall_site_timeline_site_id_fk37 FOREIGN KEY (site_id) REFERENCES firewall_sites(id) ON DELETE CASCADE;
ALTER TABLE ha_events ADD CONSTRAINT ha_events_vm_id_fk38 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE SET NULL;
ALTER TABLE ha_events ADD CONSTRAINT ha_events_host_id_fk39 FOREIGN KEY (host_id) REFERENCES hosts(id) ON DELETE SET NULL;
ALTER TABLE ha_policies ADD CONSTRAINT ha_policies_vm_id_fk40 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE host_lldp_cache ADD CONSTRAINT host_lldp_cache_host_id_fk41 FOREIGN KEY (host_id) REFERENCES hosts(id) ON DELETE CASCADE;
ALTER TABLE hosts ADD CONSTRAINT hosts_cluster_id_fk42 FOREIGN KEY (cluster_id) REFERENCES clusters(id);
ALTER TABLE hosts ADD CONSTRAINT hosts_baremetal_origin_id_fk43 FOREIGN KEY (baremetal_origin_id) REFERENCES baremetal_servers(id);
ALTER TABLE image_shares ADD CONSTRAINT image_shares_template_id_fk44 FOREIGN KEY (template_id) REFERENCES templates(id) ON DELETE CASCADE;
ALTER TABLE instance_security_groups ADD CONSTRAINT instance_security_groups_sg_id_fk45 FOREIGN KEY (sg_id) REFERENCES security_groups(id) ON DELETE CASCADE;
ALTER TABLE keypairs ADD CONSTRAINT keypairs_project_id_fk46 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL;
ALTER TABLE lb_members ADD CONSTRAINT lb_members_load_balancer_id_fk47 FOREIGN KEY (load_balancer_id) REFERENCES load_balancers(id) ON DELETE CASCADE;
ALTER TABLE lb_members ADD CONSTRAINT lb_members_vm_id_fk48 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE load_balancers ADD CONSTRAINT load_balancers_project_id_fk49 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL;
ALTER TABLE load_balancers ADD CONSTRAINT load_balancers_host_id_fk50 FOREIGN KEY (host_id) REFERENCES hosts(id) ON DELETE CASCADE;
ALTER TABLE maintenance_schedules ADD CONSTRAINT maintenance_schedules_host_id_fk51 FOREIGN KEY (host_id) REFERENCES hosts(id) ON DELETE CASCADE;
ALTER TABLE maintenance_windows ADD CONSTRAINT maintenance_windows_host_id_fk52 FOREIGN KEY (host_id) REFERENCES hosts(id);
ALTER TABLE migration_jobs ADD CONSTRAINT migration_jobs_vm_id_fk53 FOREIGN KEY (vm_id) REFERENCES vms(id);
ALTER TABLE migration_jobs ADD CONSTRAINT migration_jobs_source_host_id_fk54 FOREIGN KEY (source_host_id) REFERENCES hosts(id);
ALTER TABLE migration_jobs ADD CONSTRAINT migration_jobs_dest_host_id_fk55 FOREIGN KEY (dest_host_id) REFERENCES hosts(id);
ALTER TABLE network_ipam_pools ADD CONSTRAINT network_ipam_pools_segment_id_fk56 FOREIGN KEY (segment_id) REFERENCES network_segments(id) ON DELETE CASCADE;
ALTER TABLE network_reservations ADD CONSTRAINT network_reservations_network_id_fk57 FOREIGN KEY (network_id) REFERENCES networks(id) ON DELETE CASCADE;
ALTER TABLE network_reservations ADD CONSTRAINT network_reservations_vm_id_fk58 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE SET NULL;
ALTER TABLE networks ADD CONSTRAINT networks_cluster_id_fk59 FOREIGN KEY (cluster_id) REFERENCES clusters(id);
ALTER TABLE placement_recommendations ADD CONSTRAINT placement_recommendations_vm_id_fk60 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE placement_recommendations ADD CONSTRAINT placement_recommendations_from_host_id_fk61 FOREIGN KEY (from_host_id) REFERENCES hosts(id);
ALTER TABLE placement_recommendations ADD CONSTRAINT placement_recommendations_to_host_id_fk62 FOREIGN KEY (to_host_id) REFERENCES hosts(id);
ALTER TABLE ports ADD CONSTRAINT ports_network_id_fk63 FOREIGN KEY (network_id) REFERENCES networks(id) ON DELETE CASCADE;
ALTER TABLE ports ADD CONSTRAINT ports_project_id_fk64 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL;
ALTER TABLE ports ADD CONSTRAINT ports_vm_id_fk65 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE SET NULL;
ALTER TABLE ports ADD CONSTRAINT ports_security_group_id_fk66 FOREIGN KEY (security_group_id) REFERENCES security_groups(id) ON DELETE SET NULL;
ALTER TABLE project_role_assignments ADD CONSTRAINT project_role_assignments_user_id_fk67 FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE project_role_assignments ADD CONSTRAINT project_role_assignments_project_id_fk68 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;
ALTER TABLE security_group_rules ADD CONSTRAINT security_group_rules_security_group_id_fk69 FOREIGN KEY (security_group_id) REFERENCES security_groups(id) ON DELETE CASCADE;
ALTER TABLE security_groups ADD CONSTRAINT security_groups_project_id_fk70 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL;
ALTER TABLE snapshot_records ADD CONSTRAINT snapshot_records_vm_id_fk71 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE soc_alerts ADD CONSTRAINT soc_alerts_rule_id_fk72 FOREIGN KEY (rule_id) REFERENCES soc_detection_rules(id) ON DELETE SET NULL;
ALTER TABLE soc_event_exports ADD CONSTRAINT soc_event_exports_integration_id_fk73 FOREIGN KEY (integration_id) REFERENCES soc_integrations(id) ON DELETE CASCADE;
ALTER TABLE soc_forwarder_cursors ADD CONSTRAINT soc_forwarder_cursors_integration_id_fk74 FOREIGN KEY (integration_id) REFERENCES soc_integrations(id) ON DELETE CASCADE;
ALTER TABLE soc_playbook_runs ADD CONSTRAINT soc_playbook_runs_playbook_id_fk75 FOREIGN KEY (playbook_id) REFERENCES soc_playbooks(id) ON DELETE CASCADE;
ALTER TABLE soc_playbook_runs ADD CONSTRAINT soc_playbook_runs_alert_id_fk76 FOREIGN KEY (alert_id) REFERENCES soc_alerts(id) ON DELETE SET NULL;
ALTER TABLE stacks ADD CONSTRAINT stacks_project_id_fk77 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL;
ALTER TABLE storage_backup_sla ADD CONSTRAINT storage_backup_sla_pool_id_fk78 FOREIGN KEY (pool_id) REFERENCES storage_pools(id) ON DELETE CASCADE;
ALTER TABLE storage_pools ADD CONSTRAINT storage_pools_cluster_id_fk79 FOREIGN KEY (cluster_id) REFERENCES clusters(id);
ALTER TABLE task_steps ADD CONSTRAINT task_steps_task_id_fk80 FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE;
ALTER TABLE tasks ADD CONSTRAINT tasks_host_id_fk81 FOREIGN KEY (host_id) REFERENCES hosts(id);
ALTER TABLE vm_atlas_volumes ADD CONSTRAINT vm_atlas_volumes_vm_id_fk82 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE vm_disks ADD CONSTRAINT vm_disks_vm_id_fk83 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE vm_forks ADD CONSTRAINT vm_forks_fork_vm_id_fk84 FOREIGN KEY (fork_vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE vm_metrics ADD CONSTRAINT vm_metrics_vm_id_fk85 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE vm_restore_points ADD CONSTRAINT vm_restore_points_vm_id_fk86 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE vm_schedules ADD CONSTRAINT vm_schedules_vm_id_fk87 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE vm_sleep_events ADD CONSTRAINT vm_sleep_events_vm_id_fk88 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE vm_watchdog ADD CONSTRAINT vm_watchdog_vm_id_fk89 FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
ALTER TABLE vms ADD CONSTRAINT vms_cluster_id_fk90 FOREIGN KEY (cluster_id) REFERENCES clusters(id);
ALTER TABLE vms ADD CONSTRAINT vms_host_id_fk91 FOREIGN KEY (host_id) REFERENCES hosts(id);
ALTER TABLE volume_snapshots ADD CONSTRAINT volume_snapshots_volume_id_fk92 FOREIGN KEY (volume_id) REFERENCES volumes(id) ON DELETE CASCADE;
ALTER TABLE volumes ADD CONSTRAINT volumes_project_id_fk93 FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL;
ALTER TABLE volumes ADD CONSTRAINT volumes_storage_pool_id_fk94 FOREIGN KEY (storage_pool_id) REFERENCES storage_pools(id) ON DELETE SET NULL;
ALTER TABLE volumes ADD CONSTRAINT volumes_attached_vm_id_fk95 FOREIGN KEY (attached_vm_id) REFERENCES vms(id) ON DELETE SET NULL;

CREATE TRIGGER preserve_routable_agent_addr BEFORE UPDATE OF agent_grpc_addr ON hosts
  FOR EACH ROW EXECUTE FUNCTION preserve_routable_agent_addr();

CREATE INDEX chaos_runs_experiment ON chaos_runs(experiment_id, started_at);
CREATE UNIQUE INDEX chaos_runs_one_live ON chaos_runs(experiment_id) WHERE status = 'running';
CREATE INDEX cloud_group_project ON cloud_instance_groups(project_id);
CREATE INDEX cloud_subnets_vpc ON cloud_subnets(vpc_id);
CREATE INDEX idx_ai_actions_status ON ai_actions(status);
CREATE INDEX idx_ai_conversations_owner ON ai_conversations(user_id, created_at DESC);
CREATE UNIQUE INDEX idx_ai_conversations_user_agent
    ON ai_conversations(user_id, agent_id);
CREATE INDEX idx_ai_incidents_created ON ai_incidents(created_at DESC);
CREATE INDEX idx_ai_incidents_status ON ai_incidents(status);
CREATE INDEX idx_ai_memory_owner ON ai_memory_entries(owner_id);
CREATE INDEX idx_ai_prompts_owner ON ai_prompts(owner_id);
CREATE INDEX idx_air_gap_bundles_exported ON air_gap_bundles(exported_at DESC);
CREATE INDEX idx_alert_rules_enabled ON alert_rules(enabled);
CREATE INDEX idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX idx_api_trace_path ON api_trace_spans(path, recorded_at DESC);
CREATE INDEX idx_api_trace_recorded ON api_trace_spans(recorded_at DESC);
CREATE INDEX idx_app_group_vms_vm ON application_group_vms(vm_id);
CREATE INDEX idx_audit_actor ON audit_logs(actor);
CREATE INDEX idx_audit_created ON audit_logs(created_at DESC);
CREATE INDEX idx_backup_schedules_enabled ON backup_schedules(enabled);
CREATE INDEX idx_backup_status ON backup_records(status);
CREATE INDEX idx_backup_vm ON backup_records(vm_id);
CREATE INDEX idx_baremetal_servers_hostname ON baremetal_servers(hostname);
CREATE INDEX idx_blueprints_cluster ON blueprints(cluster_id);
CREATE INDEX idx_channel_deliveries_pending ON channel_deliveries(status, next_retry_at);
CREATE INDEX idx_cloud_alarms_group ON cloud_alarms(group_id);
CREATE INDEX idx_console_access_requests_status ON console_access_requests(status, created_at DESC);
CREATE INDEX idx_console_sessions_active ON console_sessions(ended_at) WHERE ended_at IS NULL;
CREATE INDEX idx_console_sessions_actor ON console_sessions(actor, started_at DESC);
CREATE INDEX idx_console_sessions_vm ON console_sessions(vm_id, started_at DESC);
CREATE INDEX idx_content_images_kind ON content_images(kind);
CREATE INDEX idx_content_images_status ON content_images(status);
CREATE INDEX idx_elastic_ips_vm ON elastic_ips(vm_id);
CREATE INDEX idx_events_created ON events(created_at DESC);
CREATE INDEX idx_events_kind ON events(kind);
CREATE INDEX idx_fleet_snapshot_schedules_enabled ON fleet_snapshot_schedules(enabled);
CREATE INDEX idx_fw_approvals_status ON firewall_approvals(status, created_at DESC);
CREATE INDEX idx_fw_approvals_target ON firewall_approvals(target_kind, target_id);
CREATE INDEX idx_fw_cloud_provider ON firewall_cloud_snapshots(provider, captured_at DESC);
CREATE INDEX idx_fw_posture_target ON firewall_posture_snapshots(target_kind, target_id, captured_at DESC);
CREATE INDEX idx_fw_site_drift_site ON firewall_site_drift(site_id, captured_at DESC);
CREATE INDEX idx_fw_site_policies_site ON firewall_site_policies(site_id);
CREATE INDEX idx_fw_temp_expiry ON firewall_temporary_rules(expires_at) WHERE applied = TRUE;
CREATE INDEX idx_fw_timeline_target ON firewall_timeline(target_kind, target_id, created_at DESC);
CREATE INDEX idx_ha_events_created ON ha_events(created_at DESC);
CREATE INDEX idx_host_lldp_cache_fetched ON host_lldp_cache(fetched_at DESC);
CREATE INDEX idx_hosts_site_rack ON hosts(site, rack);
CREATE INDEX idx_hosts_validation_status ON hosts(validation_status);
CREATE INDEX idx_ipam_pools_segment ON network_ipam_pools(segment_id);
CREATE INDEX idx_isg_sg ON instance_security_groups(sg_id);
CREATE INDEX idx_lb_members_lb ON lb_members(load_balancer_id);
CREATE INDEX idx_maintenance_sched_run ON maintenance_schedules(run_at) WHERE status = 'pending';
CREATE INDEX idx_maintenance_windows_host ON maintenance_windows(host_id);
CREATE INDEX idx_metric_hourly_hour ON metric_hourly(hour);
CREATE INDEX idx_metric_samples_ts ON metric_samples(ts);
CREATE INDEX idx_migration_jobs_status ON migration_jobs(status);
CREATE INDEX idx_migration_jobs_vm ON migration_jobs(vm_id);
CREATE INDEX idx_net_reservations_network ON network_reservations(network_id);
CREATE INDEX idx_net_reservations_vm ON network_reservations(vm_id);
CREATE INDEX idx_networks_segment ON networks(segment_id);
CREATE INDEX idx_notification_channels_enabled ON notification_channels(enabled);
CREATE INDEX idx_oidc_states_created ON oidc_states(created_at);
CREATE INDEX idx_ops_runbook_exec_created ON ops_runbook_executions(created_at DESC);
CREATE INDEX idx_ops_showback_project ON ops_showback_snapshots(project_name, captured_at DESC);
CREATE INDEX idx_placement_open ON placement_recommendations(status) WHERE status = 'open';
CREATE INDEX idx_platform_plugins_category ON platform_plugins(category, featured);
CREATE INDEX idx_ports_network ON ports(network_id);
CREATE INDEX idx_ports_vm ON ports(vm_id);
CREATE INDEX idx_project_role_assignments_project ON project_role_assignments(project_id);
CREATE INDEX idx_project_role_assignments_user ON project_role_assignments(user_id);
CREATE INDEX idx_resource_tags_key ON resource_tags(key, value);
CREATE INDEX idx_scheduled_jobs_enabled ON scheduled_jobs(enabled);
CREATE INDEX idx_secgroup_rules_group ON security_group_rules(security_group_id);
CREATE INDEX idx_snapshot_status ON snapshot_records(status);
CREATE INDEX idx_snapshot_vm ON snapshot_records(vm_id);
CREATE UNIQUE INDEX idx_soc_alerts_dedupe_open
    ON soc_alerts(dedupe_key) WHERE status IN ('open', 'acknowledged');
CREATE INDEX idx_soc_alerts_status ON soc_alerts(status, last_seen DESC);
CREATE UNIQUE INDEX idx_soc_events_dedupe
    ON soc_events(dedupe_key) WHERE dedupe_key IS NOT NULL;
CREATE INDEX idx_soc_events_occurred ON soc_events(occurred_at DESC);
CREATE INDEX idx_soc_events_severity ON soc_events(severity, occurred_at DESC);
CREATE INDEX idx_soc_events_source ON soc_events(source, occurred_at DESC);
CREATE INDEX idx_soc_playbook_runs_alert ON soc_playbook_runs(alert_id, started_at DESC);
CREATE INDEX idx_soc_playbook_runs_playbook ON soc_playbook_runs(playbook_id, started_at DESC);
CREATE INDEX idx_storage_pools_tier ON storage_pools(tier_id);
CREATE INDEX idx_tasks_created ON tasks(created_at DESC);
CREATE INDEX idx_tasks_host ON tasks(host_id);
CREATE INDEX idx_tasks_operation ON tasks(operation);
CREATE INDEX idx_tasks_operation_resource_created
    ON tasks(operation, resource_id, created_at);
CREATE INDEX idx_tasks_resource ON tasks(resource_id, resource_type);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_templates_approval ON templates(approval_status);
CREATE INDEX idx_templates_marketplace ON templates(marketplace, featured);
CREATE INDEX idx_templates_workload ON templates(workload);
CREATE INDEX idx_vm_atlas_volumes_vm ON vm_atlas_volumes(vm_id);
CREATE UNIQUE INDEX idx_vm_atlas_volumes_volume ON vm_atlas_volumes(volume_id);
CREATE INDEX idx_vm_disks_vm ON vm_disks(vm_id);
CREATE INDEX idx_vm_forks_source ON vm_forks(source_vm_id);
CREATE INDEX idx_vm_restore_points_vm ON vm_restore_points(vm_id, created_at);
CREATE INDEX idx_vm_schedules_next_run
    ON vm_schedules(next_run_at) WHERE enabled = TRUE;
CREATE INDEX idx_vm_schedules_vm
    ON vm_schedules(vm_id);
CREATE INDEX idx_vm_sleep_events_vm ON vm_sleep_events(vm_id, at);
CREATE INDEX idx_vm_watchdog_enabled ON vm_watchdog(enabled);
CREATE INDEX idx_vms_guest_tools ON vms(guest_tools_status);
CREATE INDEX idx_vms_host ON vms(host_id);
CREATE INDEX idx_vms_host_source ON vms(host_id, inventory_source);
CREATE INDEX idx_vms_inventory_source ON vms(inventory_source);
CREATE UNIQUE INDEX idx_vms_kubevirt_name
    ON vms(cluster_id, k8s_namespace, name)
    WHERE inventory_source = 'kubevirt';
CREATE UNIQUE INDEX idx_vms_libvirt_uuid
    ON vms(cluster_id, uuid)
    WHERE inventory_source = 'libvirt' AND uuid IS NOT NULL AND uuid != '';
CREATE INDEX idx_vms_lifecycle_phase ON vms(lifecycle_phase);
CREATE INDEX idx_vms_managed ON vms(managed);
CREATE INDEX idx_vms_observed_state ON vms(observed_state);
CREATE INDEX idx_volume_snapshots_volume ON volume_snapshots(volume_id);
CREATE INDEX idx_volumes_attached_vm ON volumes(attached_vm_id);
CREATE INDEX idx_volumes_project ON volumes(project_id);
CREATE INDEX idx_webhook_deliveries_pending
    ON webhook_deliveries(next_retry_at) WHERE status = 'pending';
