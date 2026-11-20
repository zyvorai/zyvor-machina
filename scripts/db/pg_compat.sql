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
