# Project quotas and project-scoped access

## Quotas are race-free and fail closed
Creating a machine checks the project's quota (VM count, vCPU, memory, storage) and your policy rules **inside the same
write transaction that inserts the machine** (`BEGIN IMMEDIATE`), so two requests at a project's limit can no longer both
succeed. If the quota tables cannot be read, the create is refused with a retryable "could not check quotas" error instead of
silently skipping the check. Covered by a concurrency test (12 simultaneous creates against a limit of 3 admit exactly 3).

## Project-scoped access (opt-in)
Global roles (`admin`, `operator`, `viewer`) say what someone may do; **project roles** (Projects → Members) say *where*.
Set `MACHINA_PROJECT_RBAC` on the controller:

| Value | Behaviour |
|-------|-----------|
| unset / `off` | today's behaviour: global roles only |
| `audit` | nothing is blocked; what `enforce` would deny is logged (`project RBAC: …`) so you can look before switching on |
| `enforce` | non-admin local users only see and act on machines in projects they belong to |

Under `enforce`, for every `/api/v1/vms/{id}[/…]` route: no membership → `403 project_forbidden`; read-only requests need any
project role; changes need `operator` or `admin` **in that project** (a global operator who is only a project viewer cannot
change the machine). `GET /api/v1/vms` returns only machines in your projects. Platform admins are never restricted.

Not covered yet (v1): routes outside `/api/v1/vms` (volumes, networks, backups listings by project), and identities that are
not rows in `users` — **API keys and service accounts keep their global role**, so scope automation by the key's role.
