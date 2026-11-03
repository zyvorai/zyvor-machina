---
sidebar_position: 12
title: Zyra AI decision tools
description: Change review, postmortems, fleet digests and migration ordering. Read-only advice that sits in front of every decision.
---

# Zyra AI decision tools

Zyra finds problems and proposes fixes. These four tools help the person who has to decide. All of them are **read-only**: they
read what the controller already knows, they change nothing, and they give the same answer with or without an LLM provider
configured (a model may rephrase text elsewhere in Zyra; none of these needs one).

:::note Status
Unit-tested. Not yet run on a real fleet; see claims C67 to C70 in the
[claims ledger](https://github.com/zyvorai/zyvor-machina/blob/main/docs/claims.md) for what has and has not been exercised.
:::

All routes need the operator role and are served by the controller; through the daemon use `/api/v1/platform/controller`.

## Change Review

In **Platform → Zyra → Approvals**, expand **What happens if I approve?** under any pending action.

```bash
curl -sk -H "Authorization: Bearer $TOKEN" \
  https://HOST:5092/api/v1/platform/controller/api/v1/ai/actions/$ACTION_ID/review
```

You get a risk (`low`, `medium`, `high`), a verdict (`approve`, `approve_with_care`, `hold`), the machine, host and application
groups in reach, whether the action is reversible, the rollback, and checks to do first. The declared risk is never lowered;
group size, irreversibility and a missing machine name can only raise it.

## Postmortem Writer

On **Incident Commander**, **Draft postmortem** downloads a Markdown file.

```bash
curl -sk -X POST -H "Authorization: Bearer $TOKEN" \
  https://HOST:5092/api/v1/platform/controller/api/v1/ai/incidents/$INCIDENT_ID/postmortem
```

Sections: summary, impact, root cause, timeline, actions taken (Zyra actions proposed in the incident's window) and follow-ups.
A section with no data says so (`_Not determined._`) rather than guessing, so the draft is safe to hand to a person to finish.

## Fleet Digest

```bash
curl -sk -H "Authorization: Bearer $TOKEN" \
  "https://HOST:5092/api/v1/platform/controller/api/v1/ai/digest?hours=24"
```

Returns counts and a ready-made `markdown` briefing: hosts and machines, open incidents, pending approvals, what Zyra executed,
failed, had rejected and had undone, and budget alerts. `headline` is `ok`, `attention` or `critical`. There is no scheduled
delivery yet; poll it from whatever sends your notifications.

## Migration Order Advisor

```bash
curl -sk -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"vm_names":["web-1","web-2","db-1"],"max_parallel":3}' \
  https://HOST:5092/api/v1/platform/controller/api/v1/ai/migration-waves/advise
```

Batches: powered-off machines first, then running machines smallest memory first, never two running machines of one application
group in a batch, at most `max_parallel` per batch. Unknown names are listed and left out. It is for machines that already exist in Machina (evacuating a host, rebalancing), found by name in the inventory. It does not
know machines that are still being imported through Transiva, so it is not for the Migration waves page. It creates nothing and
does not yet use the convergence oracle or the noisy-neighbour graph.
