# Usage Reporter

This directory contains the orchestrator's built-in usage calculator. It
supersedes the legacy `usage_calculator` crate that used to run inside the
Chelsea process and now handles the full pipeline of collecting VM events,
turning them into per-second usage aggregated per hour, and delivering batches to the billing forwarder.



* Runs as a background task spawned from `crates/orchestrator/src/main.rs`.
* Calculates the previous completed hour (same logic used for catch-up).
* Groups `VmUsageRecord`s by API key so downstream consumers can fan out per
  key (failures isolate to that key instead of the whole hour).
* Stores progress in Postgres (`usage_reporting_state`) for restart catch-up
  and at-least-once delivery guarantees.
* Currently logs each grouped payload; `deliver_batch` is the seam where we’ll
  later forward the data to an HTTP endpoint.

```
spawn_usage_task()
    │
    └─ UsageReporter::run()
        │
        ├─ catchup_missed_intervals()
        │   └─ process_interval() per backlog hour
        │
        └─ steady-state loop
            ├─ wait_duration()
            ├─ process_latest_interval()
            │   ├─ calculate_usage_for_interval()
            │   │   ├─ DB.usage().get_vms_with_usage_in_interval()
            │   │   └─ calculate_vm_usage()
            │   │       ├─ DB.usage().get_last_vm_event_before()
            │   │       ├─ DB.usage().get_vm_events()
            │   │       └─ process_vm_events() + clamped_duration_seconds()
            │   └─ deliver_batch() <-- where flowglad forwarder will be implemented, currently just logs
            └─ update_last_reported_interval()
```

---

## Configuration

Two environment variables (required) control the reporter. They’re defined in `config/public.env and config/secret.env`. missing or
malformed values log an error and panic at startup.

| Variable | Default | Description |
| --- | --- | --- |
| `USAGE_REPORTING_ENABLED` | `1` | Any of `true`/`1`/`yes` enables the reporter; `false`/`0`/`no` disables it (useful in CI or tests). Missing/invalid values cause startup to fail. |
| `USAGE_REPORTING_TEST_INTERVAL_SECS` | *(empty)* | Empty string → normal hourly cadence (runs at `xx:01`). Set to a positive number of seconds to force faster loops when manually testing. |
| `FLOWGLAD_USAGE_URL` | *(none)* | Required when billing is enabled; points at the Flowglad ingestion endpoint (`https://app.flowglad.com` in dev/test). |
| `FLOWGLAD_API_KEY` | *(none)* | Required API key for Flowglad billing; use the test key that matches the seeded account. |

Example dev settings:

```bash
export USAGE_REPORTING_ENABLED=1
export USAGE_REPORTING_TEST_INTERVAL_SECS=60      # optional
export FLOWGLAD_USAGE_URL=https://app.flowglad.com
export FLOWGLAD_API_KEY=sk_test_replace_me        # test key only
```

---

## Detailed Behavior

1. **Catch-up on startup**  
   `usage_reporting_state` stores the last completed hour. On boot we replay
   every fully elapsed hour between that timestamp and “now,” oldest first, so
   missed intervals get delivered before steady state resumes.

2. **Steady-state cadence**  
   * `wait_duration()` sleeps until one minute past the next hour (or the
     configured test interval).  
   * `process_latest_interval()` computes usage for `[interval_start,
     interval_end)` by:
     - querying `get_vms_with_usage_in_interval()` to find candidate VMs,
     - loading `get_last_vm_event_before()` plus all interval events,
     - running `process_vm_events()` to clamp each segment to the interval and
       sum CPU/memory seconds, skipping VMs with malformed timelines.
     The per-VM results are grouped by owner into a `UsageBatch`.
   * `deliver_batch()` transforms each `VmUsageRecord` into a
     `ForwardUsageRecord` and hands the whole interval to the Flowglad forwarder
     (`forward_usage_records`). The forwarder resolves subscriptions, builds one
     Flowglad usage event per metric, and POSTs them via the outbound client.
   * `update_last_reported_interval()` advances the pointer in
     `usage_reporting_state` once the batch succeeds.

3. **Error handling**  
   * Clock issues (e.g., `SystemTimeError`) bubble up and stop the task rather
     than producing bogus timestamps.
   * Database failures abort the run so an operator has to intervene; we never
     silently skip an interval.
   * Forwarder errors return `UsageError::Forward`, which bubbles up; only the
     explicit “Flowglad disabled” branch drops a batch (with a warning).

---

## Database 

Located in `crates/orchestrator/src/db/repos/usage.rs`:

| Function | Purpose |
| --- | --- |
| `get_vms_with_usage_in_interval(start, end)` | Finds VMs whose `chelsea.vm_usage_segments` overlap the interval, joined with `vms` to fetch `owner_id`/`node_id`. |
| `get_last_vm_event_before(vm_id, ts)` | Finds the most recent start/stop before the interval to seed in-progress VMs. |
| `get_vm_events(vm_id, start, end)` | Fetches all start/stop events within the interval (ordered by timestamp). |
| `get_last_reported_interval()` / `update_last_reported_interval()` | Reads/writes `usage_reporting_state` to track catch-up progress. |

Migration for the state table lives in `pg/migrations/20251215120000_usage_reporting_state.sql`
and must be applied wherever the orchestrator runs.

---

## Manual Testing

### Single-node testing with seeded data

`./scripts/single-node.sh start` seeds a “Test User” (`20251111063619_seed_db.sql`),
which also exists in Flowglad as “Vers Test User”. 

1. Start the single-node environment.

   ```
   ./scripts/single-node.sh start
   export PG="postgresql://postgres:opensesame@127.0.0.1:5432/vers?sslmode=disable"
   export DATABASE_URL="$PG"
   ```

2. Seed a few VMs that mirror some of the calculator
   test cases (same owner/node IDs as `20251111063619_seed_db.sql`). 
   

```bash
psql "$PG" <<'SQL'
-- Fixed UUIDs and owner/node IDs from 20251111063619_seed_db.sql
INSERT INTO vms (
  vm_id,
  node_id,
  owner_id,
  ip,
  wg_private_key,
  wg_public_key,
  wg_port,
  created_at
)
VALUES
  ('11111111-1111-4111-8111-111111111111',
   '4569f1fe-054b-4e8d-855a-f3545167f8a9',
   'ef90fd52-66b5-47e7-b7dc-e73c4381028f',
   'fd00:fe11:deed:0::120',
   'priv-key-segmented',
   'pub-key-segmented',
   51000,
   now()),
  ('22222222-2222-4222-8222-222222222222',
   '4569f1fe-054b-4e8d-855a-f3545167f8a9',
   'ef90fd52-66b5-47e7-b7dc-e73c4381028f',
   'fd00:fe11:deed:0::121',
   'priv-key-carry',
   'pub-key-carry',
   51001,
   now()),
  ('33333333-3333-4333-8333-333333333333',
   '4569f1fe-054b-4e8d-855a-f3545167f8a9',
   'ef90fd52-66b5-47e7-b7dc-e73c4381028f',
   'fd00:fe11:deed:0::122',
   'priv-key-double',
   'pub-key-double',
   51002,
   now())
ON CONFLICT (vm_id) DO NOTHING;

-- VM #1: two segments spanning the previous hour (total 50 minutes)
INSERT INTO chelsea.vm_usage_segments (
  vm_id,
  start_timestamp,
  start_created_at,
  stop_timestamp,
  stop_created_at,
  vcpu_count,
  ram_mib,
  start_code,
  stop_code
)
VALUES
  (
    '11111111-1111-4111-8111-111111111111',
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '70 minutes')),
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '70 minutes')),
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '50 minutes')),
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '50 minutes')),
    4,
    8192,
    'usage-segmented',
    'usage-segmented'
  ),
  (
    '11111111-1111-4111-8111-111111111111',
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '40 minutes')),
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '40 minutes')),
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '10 minutes')),
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '10 minutes')),
    1,
    8192,
    'usage-segmented',
    'usage-segmented'
  )
ON CONFLICT (vm_id, start_timestamp) DO NOTHING;

-- VM #2: started before the interval and is still running (no stop)
INSERT INTO chelsea.vm_usage_segments (
  vm_id,
  start_timestamp,
  start_created_at,
  stop_timestamp,
  stop_created_at,
  vcpu_count,
  ram_mib,
  start_code,
  stop_code
)
VALUES (
  '22222222-2222-4222-8222-222222222222',
  EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '3 hours')),
  EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '3 hours')),
  NULL,
  NULL,
  2,
  4096,
  'usage-carry',
  NULL
)
ON CONFLICT (vm_id, start_timestamp) DO NOTHING;

-- VM #3: malformed input (two start events in the same interval)
INSERT INTO chelsea.vm_usage_segments (
  vm_id,
  start_timestamp,
  start_created_at,
  stop_timestamp,
  stop_created_at,
  vcpu_count,
  ram_mib,
  start_code,
  stop_code
)
VALUES
  (
    '33333333-3333-4333-8333-333333333333',
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '35 minutes')),
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '35 minutes')),
    NULL,
    NULL,
    8, 12288,
    'usage-double-start',
    NULL
  ),
  (
    '33333333-3333-4333-8333-333333333333',
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '5 minutes')),
    EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '5 minutes')),
    NULL,
    NULL,
    8, 12288,
    'usage-double-start',
    NULL
  );
SQL
```

3. *(Optional)* Seed a second owner with no Flowglad subscription so you can
   observe one owner succeeding while another is skipped. This inserts a new
   `users`/`api_keys` row, a VM, and matching start/stop events in the same
   interval:

```bash
psql "$PG" <<'SQL'
INSERT INTO users (user_id, email, user_name, passwd_algo, passwd_iter, passwd_salt, passwd_hash)
VALUES (
  '44444444-4444-4444-8444-444444444444',
  'nosub@vers.sh',
  'no_flowglad_subscriber',
  'PBKDF2',
  100,
  '11111111111111111111111111111111',
  '22222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222'
)
ON CONFLICT (user_id) DO NOTHING;

INSERT INTO accounts (account_id, name, billing_email)
VALUES (
  '55555555-5555-4555-8555-555555555555',
  'No Flowglad Account',
  'nosub@vers.sh'
)
ON CONFLICT (account_id) DO NOTHING;

INSERT INTO organizations (org_id, account_id, name, description)
VALUES (
  '66666666-6666-4666-8666-666666666666',
  '55555555-5555-4555-8555-555555555555',
  'no_flowglad_subscriber',
  'Org seeded only for subscription-failure tests'
)
ON CONFLICT (org_id) DO NOTHING;

INSERT INTO api_keys (api_key_id, user_id, org_id, label, key_algo, key_iter, key_salt, key_hash)
VALUES (
  '77777777-7777-4777-8777-777777777777',
  '44444444-4444-4444-8444-444444444444',
  '66666666-6666-4666-8666-666666666666',
  'NoSub Key',
  'PBKDF2',
  100,
  '33333333333333333333333333333333',
  '44444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444444'
)
ON CONFLICT (api_key_id) DO NOTHING;

INSERT INTO vms (vm_id, node_id, owner_id, ip, wg_private_key, wg_public_key, wg_port, created_at)
VALUES (
  '88888888-8888-4888-8888-888888888888',
  '4569f1fe-054b-4e8d-855a-f3545167f8a9',
  '77777777-7777-4777-8777-777777777777',
  'fd00:fe11:deed:0::123',
  'priv-key-nosub',
  'pub-key-nosub',
  51003,
  now()
)
ON CONFLICT (vm_id) DO NOTHING;

INSERT INTO chelsea.vm_usage_segments (
  vm_id,
  start_timestamp,
  start_created_at,
  stop_timestamp,
  stop_created_at,
  vcpu_count,
  ram_mib,
  start_code,
  stop_code
)
VALUES (
  '88888888-8888-4888-8888-888888888888',
  EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '55 minutes')),
  EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '55 minutes')),
  EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '25 minutes')),
  EXTRACT(EPOCH FROM (date_trunc('hour', now()) - interval '25 minutes')),
  2,
  4096,
  'usage-nosub',
  'usage-nosub'
)
ON CONFLICT (vm_id, start_timestamp) DO NOTHING;
SQL
```

4. Use env vars the reporter/forwarder needs (use your Flowglad test key or the value pulled from Secrets Manager):

   ```bash
    USAGE_REPORTING_TEST_INTERVAL_SECS=6
    FLOWGLAD_API_KEY=sk_test_replace_me # populated from secrets manager
   ```

5. Start/Restart the orchestrator once the seed data and env vars are in place:

   ```bash
        unset DISCORD_ALERT_WEBHOOK_URL # so aws populates these instead of single node borking them
        unset FLOWGLAD_API_KEY
        USAGE_REPORTING_TEST_INTERVAL_SECS=6 ./scripts/single-node/orchestrator.sh
   ```

6. Watch the orchestrator logs. You should see exactly one error
   (`detected consecutive start events; skipping vm usage`) plus two Flowglad
   deliveries (carry VM at `7200 vCPU seconds`, segmented VM at `4200 vCPU seconds`).
   If you seeded the “no subscription” owner, you’ll also see
   `skipping usage for owner without Flowglad subscription` and that owner will
   be absent from Flowglad while the Vers Test User still receives usage.

---

## Known Gaps / Follow-ups

* **Payload size:** No chunking yet; very large organizations may need a
  per-key cap or split deliveries.
