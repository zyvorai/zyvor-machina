---
sidebar_position: 7
title: Zyra AI
description: An AI operator that diagnoses, correlates and proposes, then asks before it changes anything.
---

# Zyra AI

Zyra AI is the operations assistant built into the controller.

- **Diagnostics.** Continuous checks across hosts and guests that surface problems before users report them.
- **Incident correlation.** Related alerts are grouped into one incident with a probable cause.
- **Rightsizing.** Recommendations for over- and under-provisioned VMs based on observed usage.
- **Natural-language operations.** Ask about the fleet or request a change in plain language.
- **Approval queue.** Every change Zyra proposes waits for a human approval before it runs.

![Zyra AI](/machina-zyra-dark.png)

## Bring your own model

Zyra works with the LLM provider you choose. Provider API keys are stored encrypted at rest with AES-256-GCM when
`MACHINA_API_KEY_MASTER_KEY` is set on the controller:

```bash
export MACHINA_API_KEY_MASTER_KEY=$(openssl rand -hex 32)
```
