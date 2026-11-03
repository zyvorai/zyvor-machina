// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import Reveal from '@site/src/components/Reveal';
import BrowserFrame from '@site/src/components/BrowserFrame';
import {DEMO_URL, REPO} from '@site/src/data/product';
import styles from './ai.module.css';

type Feature = {name: string; tag: string; text: string; how: string; doc: string};

const NEW: Feature[] = [
  {
    name: 'Change Review',
    tag: 'New',
    text: 'Before you approve anything, ask "what happens if I do?". Zyra shows the machines, hosts and application groups in reach, whether it can be undone, and a plain verdict.',
    how: 'GET /api/v1/ai/actions/{id}/review',
    doc: '/docs/operations/zyra-ai#change-review',
  },
  {
    name: 'Postmortem Writer',
    tag: 'New',
    text: 'One click on an incident drafts the postmortem: summary, impact, timeline, root cause, actions taken and follow-ups. Where the data is missing it says so instead of guessing.',
    how: 'POST /api/v1/ai/incidents/{id}/postmortem',
    doc: '/docs/operations/zyra-ai#postmortem-writer',
  },
  {
    name: 'Fleet Digest',
    tag: 'New',
    text: 'A briefing for the last day or week: what is down, what needs a person, what Zyra did, what it undid, and where the budget stands. Same text with or without an LLM.',
    how: 'GET /api/v1/ai/digest?hours=24',
    doc: '/docs/operations/zyra-ai#fleet-digest',
  },
  {
    name: 'Migration Wave Copilot',
    tag: 'New',
    text: 'Give it the machines you want to move. It proposes batches: powered-off machines first, small running machines next, never two from one application group at once.',
    how: 'POST /api/v1/ai/migration-waves/advise',
    doc: '/docs/operations/zyra-ai#migration-wave-copilot',
  },
];

const EXISTING: Feature[] = [
  {name: 'Incident Commander', tag: 'Ships', text: 'Correlates failures into one incident with a timeline and runbook steps.', how: '/platform/zyra/incidents', doc: '/docs/operations/zyra-ai'},
  {name: 'Black Box root cause', tag: 'Ships', text: 'Freezes a per-VM flight recording on a trigger and ranks likely causes with evidence.', how: '/api/v1/ai/incidents/blackbox', doc: '/docs/operations/zyra-ai'},
  {name: 'Performance autopilot', tag: 'Ships', text: 'One ranked, evidence-backed optimisation plan per VM.', how: '/api/v1/ai/performance-autopilot', doc: '/docs/operations/zyra-ai'},
  {name: 'Verify and undo', tag: 'Ships', text: 'Every executed action is checked afterwards and, when the reverse is exact, undone in one click.', how: '/platform/zyra/approvals', doc: '/docs/operations/zyra-ai'},
];

const STEPS = [
  {n: '1', t: 'Detect', d: 'Zyra correlates events and proposes a fix.'},
  {n: '2', t: 'Review', d: 'See the blast radius and the rollback first.'},
  {n: '3', t: 'Approve', d: 'A person decides. Risky classes can never go automatic.'},
  {n: '4', t: 'Verify', d: 'Zyra checks the result and offers undo.'},
];

function Card({f}: {f: Feature}): ReactNode {
  return (
    <div className={clsx('mx-card', styles.feature)}>
      <span className={clsx(styles.tag, f.tag === 'New' && styles.tagNew)}>{f.tag}</span>
      <Heading as="h3">{f.name}</Heading>
      <p>{f.text}</p>
      <code className={styles.how}>{f.how}</code>
      <Link className={styles.more} to={f.doc}>Read the guide →</Link>
    </div>
  );
}

export default function AiPage(): ReactNode {
  return (
    <Layout
      title="Zyra AI"
      description="AI operations that ask before they act: change review, postmortems, fleet digests and migration planning for your own KVM fleet.">
      <header className={styles.hero}>
        <div className="container">
          <div className="mx-eyebrow">Zyra AI</div>
          <Heading as="h1" className={styles.title}>
            An operator that <span className="mx-gradient">asks first</span>.
          </Heading>
          <p className={styles.lede}>
            Zyra finds the problem, shows you what approving would do, waits for a person, then checks its own work.
            Bring your own model, or run the keyless local one. Nothing leaves your hosts unless you point it somewhere.
          </p>
          <div className={styles.ctas}>
            <Link className="mx-btn mx-btn--primary" href={DEMO_URL}>Book a demo</Link>
            <Link className="mx-btn mx-btn--ghost-dark" to="/docs/operations/zyra-ai">Read the guide</Link>
          </div>
        </div>
      </header>

      <main className="container mx-section">
        <Reveal>
          <ol className={styles.flow} aria-label="How an action moves through Zyra">
            {STEPS.map((s, i) => (
              <li key={s.n} className={styles.step} style={{animationDelay: `${i * 0.35}s`}}>
                <span className={styles.num}>{s.n}</span>
                <strong>{s.t}</strong>
                <span>{s.d}</span>
              </li>
            ))}
          </ol>
        </Reveal>

        <Reveal>
          <Heading as="h2" className={styles.h2}>New decision tools</Heading>
          <p className={styles.sub}>
            Built for the moments an operator actually decides. All four are read-only: they advise, a person acts.
          </p>
        </Reveal>
        <div className={styles.grid}>
          {NEW.map((f, i) => (
            <Reveal key={f.name} delay={i * 70}><Card f={f} /></Reveal>
          ))}
        </div>

        <Reveal>
          <div className={styles.shot}>
            <BrowserFrame
              src="/zyra-change-review.jpg"
              alt="Approval queue with the expanded 'What happens if I approve?' review: a safe low-risk start and a held, irreversible delete"
              url="https://machina:5092/platform/zyra/approvals"
            />
          </div>
        </Reveal>

        <Reveal>
          <Heading as="h2" className={styles.h2}>Already in the box</Heading>
        </Reveal>
        <div className={styles.grid}>
          {EXISTING.map((f, i) => (
            <Reveal key={f.name} delay={i * 70}><Card f={f} /></Reveal>
          ))}
        </div>

        <Reveal>
          <div className={clsx('mx-card', styles.trust)}>
            <Heading as="h3">What "asks first" means here</Heading>
            <ul>
              <li>Only four low-risk, reversible action classes can ever be made automatic, and only by you.</li>
              <li>Network policy, temporary access and anything two-person are never automatic.</li>
              <li>Keys for your model provider are encrypted at rest. A local model needs no key.</li>
              <li>
                Every statement on this page traces to the{' '}
                <Link href={`${REPO}/blob/main/docs/claims.md`}>claims ledger</Link>, which says what has been run on a real host.
              </li>
            </ul>
          </div>
        </Reveal>
      </main>
    </Layout>
  );
}
