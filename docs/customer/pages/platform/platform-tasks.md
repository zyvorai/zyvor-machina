# Tasks

## Purpose

The controller's orchestration queue. Every long-running operation (VM create, migrate, backup, agent upgrade, …) is a task with an operation name, status, progress and message.

## When to use it

- Follow a long operation you started elsewhere
- Find out why something failed
- Check controller health

## How to get there

- Route: `/platform/tasks`
- Nav: **Platform → Tasks** (or spotlight / Finder search)

## What you can do

1. Filter by status: **All**, **Pending**, **Running**, **Completed**, **Failed**.
2. **Filter by operation** (for example `vm.migrate`); **Clear filter** resets it.
3. Open a row for **Task detail**: operation, target, progress, message, the controller that ran it, and created/updated times.
4. **Controller health** shows the result of `GET /api/v1/health`.

## If something is wrong

- **Task stuck in Pending:** the controller worker or (with NATS) the task bus is not processing; check controller health and logs.
- **Failed:** the message carries the error and usually a remediation hint.
- Scripts can poll the same data at `/api/v1/tasks/{task_id}`.

## Related pages

- [Platform events](platform-events.md)
- [Activity Monitor](platform-activity.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
