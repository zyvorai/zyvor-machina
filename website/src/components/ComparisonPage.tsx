// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import Reveal from '@site/src/components/Reveal';
import {DEMO_URL, REPO} from '@site/src/data/product';
import type {Comparison} from '@site/src/data/comparisons';
import styles from '@site/src/pages/subpage.module.css';

export default function ComparisonPage({c}: {c: Comparison}): ReactNode {
  return (
    <Layout title={c.title} description={c.description}>
      <header className={styles.header}>
        <div className="container">
          <div className="mx-eyebrow">Comparison</div>
          <Heading as="h1" className={styles.title}>
            Machina vs <span className="mx-gradient">{c.name}</span>
          </Heading>
          <p className={styles.lede}>{c.lede}</p>
        </div>
      </header>
      <main className="container mx-section">
        <Reveal>
          <table className="mx-table">
            <thead>
              <tr>
                <th />
                <th>Machina</th>
                <th>{c.name}</th>
              </tr>
            </thead>
            <tbody>
              {c.rows.map((r) => (
                <tr key={r.topic}>
                  <td>{r.topic}</td>
                  <td className="mx-win">
                    <strong>{r.status}.</strong> {r.machina}
                  </td>
                  <td>{r.other}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className={styles.fine}>
            Status: Yes = shipped; Partial = part of the capability ships; Planned = on the roadmap, not available;
            No = not offered. Each statement traces to the{' '}
            <Link href={`${REPO}/blob/main/docs/claims.md`}>claims ledger</Link>.
          </p>
        </Reveal>

        <div className={styles.twoUp}>
          <Reveal>
            <div className={clsx('mx-card', styles.panel)}>
              <Heading as="h3">Choose Machina when</Heading>
              <ul>{c.machinaWhen.map((t) => <li key={t}>{t}</li>)}</ul>
            </div>
          </Reveal>
          <Reveal delay={90}>
            <div className={clsx('mx-card', styles.panel)}>
              <Heading as="h3">Choose {c.name} when</Heading>
              <ul>{c.otherWhen.map((t) => <li key={t}>{t}</li>)}</ul>
            </div>
          </Reveal>
        </div>

        <Reveal>
          <div className={clsx('mx-card', styles.callout)}>
            <div>
              <Heading as="h3">Test it on your own workload</Heading>
              <p>The 30-day evaluation runs on one host and is the only comparison that counts.</p>
            </div>
            <div className={styles.calloutCtas}>
              <Link className="mx-btn mx-btn--primary" href={DEMO_URL}>Book a demo</Link>
              <Link className="mx-btn mx-btn--ghost-dark" href={REPO}>Read the source</Link>
            </div>
          </div>
        </Reveal>

        <p className={styles.fine}>
          {c.trademark} The {c.name} column reflects public documentation at the time of writing; check the vendor's
          current documentation and pricing before you decide.
        </p>
      </main>
    </Layout>
  );
}
