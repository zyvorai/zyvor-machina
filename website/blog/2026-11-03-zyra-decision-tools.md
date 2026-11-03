---
slug: zyra-decision-tools
title: Four small AI tools for the moment you have to decide
authors: [zyvor]
tags: [ai, zyra, operations]
description: Change review, postmortems, fleet digests and migration ordering. Read-only, deterministic, and honest about what is not built.
---

Most AI in operations tools tries to act for you. The hard part of on-call is different: someone has to decide, quickly, with
incomplete information. Zyra already proposes fixes and waits for approval. This release adds four tools for the person doing
the approving.

{/* truncate */}

## What shipped

- **Change Review.** Under every pending approval, **What happens if I approve?** shows the machine, host and application
  groups in reach, whether the action can be undone, and a verdict: approve, approve with care, or hold.
- **Postmortem Writer.** One click on an incident drafts a Markdown postmortem. A section with no data says so instead of
  guessing.
- **Fleet Digest.** A briefing for the last N hours: what is down, what needs a person, what Zyra did and what it undid.
- **Migration Order Advisor.** Suggests batches for moving machines that already run in Machina: powered-off machines first,
  never two running machines of one application group at once. It does not know machines still being imported.

## What they deliberately are not

They are read-only and rule-based. They give the same answer with no LLM configured, so you can read the rules and disagree
with them. A model may phrase things elsewhere in Zyra; none of these depends on one.

## What is not done

Change Review, the Fleet Digest and the Postmortem Writer have run on one demo controller; the Migration Order Advisor is
unit-tested only (claims C67 to C70). The digest can be sent on a schedule to your notification channels (see the guide), but there
are no per-user preferences. The advisor does not yet use the migration oracle or the noisy-neighbour graph. Change Review does not yet draw on the digital
twin. The [guide](/docs/operations/zyra-ai) lists each gap.

Try it from the [Zyra AI page](/ai), or [book a demo](https://zyvor.dev/schedule).
