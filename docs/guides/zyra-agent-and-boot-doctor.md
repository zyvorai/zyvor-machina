# Zyra agent, Boot Doctor and guest-agent setup

What shipped, how it stays safe, and how to turn it on.

## Boot Doctor (VM detail → Overview)

Shown when a machine is not running, has a last error, or you arrive with `?action=bootdoctor`
(also in the command palette: "Boot Doctor").

1. **Check disk** — shuts the guest down cleanly (never force-stops; gives up after 2.5 min and changes nothing),
   then runs `guestkit doctor --explain -o json` on its disk.
2. **Preview the fix** — `guestkit repair --fix boot --dry-run`; nothing is written.
3. **Repair now** — gated behind a confirm box. Backs the disk up first (`--backup`), repairs, starts the VM and runs a
   health check. The backup path is shown so the repair can be undone by restoring it.

Needs GuestKit on the hypervisor that owns the VM. Without it the card says so instead of failing.

Daemon API (same host as the VM; VM must be powered off):

| Route | Purpose |
|-------|---------|
| `GET  /api/v1/guest-repair/capabilities` | `{cli_found, cli_path, agent_binary, agent_binary_found}` |
| `POST /api/v1/vms/{name}/guest-repair/diagnose` | offline doctor report |
| `POST /api/v1/vms/{name}/guest-repair/apply` | `{dry_run, backup}` |
| `POST /api/v1/vms/{name}/guest-agent/inject` | install the guest agent into the disk |

## Guest-agent setup

VM detail → "Set up guest agent" (or the fleet fix-it list). Routes, easiest first:

- **Install automatically** (Linux guests): clean shutdown → `guestkit agent-inject` → start → verify the agent answers.
  Needs `[libvirt] guestkit_agent_binary` in `/etc/machina/config.toml` pointing at the static musl agent
  (`/usr/local/lib/machina/guestkit-agent`). The host CLI must be built with `--features agent`.
- Manual routes (cloud-init, package, ISO) are listed in the same dialog.

The automatic route checks the daemon supports it *before* shutting anything down, and starts the VM again if the
install fails after a shutdown it caused.

## Verify and Undo (Zyra approvals → Recent actions)

Every executed AI action records the VM's state before it ran (`ai_actions`, migration `031`). Each row has
**Check it worked** (re-reads live state) and **Undo** where a safe inverse exists:

| Action | Undo |
|--------|------|
| `enable_ha` | `disable_ha` (only if HA was off before) |
| `start_vm` | `shutdown_vm` (only if the VM was off before) |

Controller routes: `GET /api/v1/ai/actions/history`, `POST /api/v1/ai/actions/{id}/verify|undo`.
Older controllers lack them; the UI hides the section rather than erroring.

## Ask Zyra (agent loop)

Zyra approvals → **Ask Zyra to take care of something**. `POST /api/v1/ai/agent/run {prompt}` runs a bounded
tool-calling loop (max 6 steps) over your configured provider (Anthropic, or OpenAI-compatible incl. Ollama).

- **Read tools:** `list_vms`, `list_hosts`, `recent_events`, `find_idle_vms` (idle ≥ 24 h: CPU average < 5%, never > 25%, with monthly cost), `plan_environment` (machines, sizing, storage, monthly cost for a described environment).
- **Write tool:** `propose_action` (`start_vm`, `stop_vm`, `enable_ha`, `create_backup`, `install_guest_tools`, `create_environment`). It only
  **queues** an item in the approval queue with risk "Review required" (so autopilot never auto-runs it) and source
  `zyra-agent`. Nothing changes until a person approves it, and execution is audited like any other action.
- Tool output is wrapped as untrusted data, so text inside a VM name or event cannot give the model instructions.

Turn it on: Settings → AI Providers, add a provider and key. Until then the endpoint answers
"Zyra AI is turned off". Set `MACHINA_API_KEY_MASTER_KEY` to encrypt stored keys.

### Describe it, get it
Ask for an environment in plain words ("staging for 10 developers"). Zyra plans it (read-only) and queues a
`create_environment` proposal showing machine count and monthly cost. Only an **administrator** approving it creates
the machines (same path as `POST /api/v1/ai/intent/environment/execute`, placed by the scheduler, built by the task bus).
GPU environments are not supported by this action.

### FinOps that acts
`stop_vm` is only proposed for machines the idle detector agrees are idle, and the saving shown to the approver is
computed on the server, not by the model. It is a clean guest shutdown and **Undo** starts the machine again.
Idle detection needs at least a day of samples (below).

### Live steps
`POST /api/v1/ai/agent/stream` (same body) streams server-sent events as the agent works:
`{"type":"step","kind":"tool_call|tool_result","tool":…}` per step, then `{"type":"done","run":{…}}` or
`{"type":"error","message":…}`. The panel shows steps live and falls back to `/agent/run` on older controllers.

## Migration copilot (Migration → Plan waves)
Scores every machine's disk for a move to KVM with GuestKit (`migrate-plan`), groups them into waves — 1: score ≥ 85 and
nothing blocking; 2: 70–84; 3: below 70, a licensing warning, or a boot score under 60 — quickest cutovers first, with the
changes and blockers listed per machine and a link to Boot Doctor for boot problems. Planning only; nothing is changed.

## Drift check (VM detail, powered-off machines)
"Compare with a golden image" runs `guestkit drift` between this machine's disk and another powered-off machine's,
read-only. `POST /api/v1/vms/{name}/guest-drift {baseline}` on the daemon.

## Trust ladder (Zyra approvals → "What Zyra may do on its own")

Each class starts at **Ask me**. After 5 approvals in a row (a rejection or failure resets the streak) Zyra offers
**Automatic**; it never switches it on itself. Only `create_backup`, `enable_ha`, `install_guest_tools` and `start_vm`
can be automatic. Stop, environment creation, network policy and temporary access always need a person. Automatic
classes run only when AI mode is `autopilot`, at most `max_per_run` (default 3) per scheduled run, through the normal
approve path (audited, verifiable, undoable).
`GET /api/v1/ai/trust`, `PUT /api/v1/ai/trust/{action_type}` `{level: ask|auto, max_per_run}` (admin).

## Machina as an MCP server

`POST /api/v1/mcp` (JSON-RPC 2.0, streamable-HTTP transport with JSON responses; `initialize`, `ping`, `tools/list`,
`tools/call`). Same tools and same rules as the built-in agent: reads are live, the only write is a proposal queued for
human approval. Authenticate like any controller call (bearer token, operator role or above), e.g. in Claude Code:
`claude mcp add --transport http machina https://HOST:5092/api/v1/platform/controller/api/v1/mcp --header "Authorization: Bearer <token>"`.

## Forecasting

A leader-only recorder stores VM memory/CPU and storage-pool fill every 5 minutes (`metric_samples`, pruned at 14 days).
`/api/v1/ai/sre/forecast` fits a straight line to recent samples and reports when the metric reaches 95%, with a
confidence from the fit quality and amount of history. With under 12 samples or 1 hour of history, a flat or falling
trend, a poor fit, or a crossing more than 30 days out, there is **no forecast** (only a plain "at N% now" for
machines already above 90%). It never invents a time.

## Tests

- Web: `web/e2e/platform-zyra-agent.spec.ts`, `web/e2e/platform-boot-doctor.spec.ts` (mocked API),
  vitest for `summariseDoctorOutput` and the agent response adapters.
- Rust: unit tests in `controller/src/engine/ai/agent_loop.rs` and `action_audit.rs`
  (run `cargo test -p machina-controller` on a Linux host).
- Live: test only on a throwaway VM, never a production machine.
