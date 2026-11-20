// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import Reveal from '@site/src/components/Reveal';
import {DEMO_URL, REPO} from '@site/src/data/product';
import styles from './subpage.module.css';

type Asset = {
  title: string;
  meta: string;
  blurb: string;
  thumb?: string;
  files: {label: string; href: string}[];
};

const ASSETS: Asset[] = [
  {
    title: 'Customer feature guide',
    meta: 'PDF',
    blurb: 'The full capability map: compute, consoles, fleet, Fleet Cloud, storage, networking, security and AI operations.',
    files: [{label: 'Download PDF', href: 'pathname:///resources/machina-customer-feature-guide.pdf'}],
  },
  {
    title: 'Getting started',
    meta: 'PDF',
    blurb: 'First login and the everyday workflows: create a VM, open a console, take a snapshot.',
    files: [{label: 'Download PDF', href: 'pathname:///resources/Machina-Getting-Started.pdf'}],
  },
  {
    title: 'Admin basics',
    meta: 'PDF',
    blurb: 'Day-one setup and day-two administration for operators.',
    files: [{label: 'Download PDF', href: 'pathname:///resources/Machina-Admin-Basics.pdf'}],
  },
  {
    title: 'Page-by-page guide',
    meta: 'PDF',
    blurb: 'Every primary screen in the UI, what it is for and how to use it.',
    files: [{label: 'Download PDF', href: 'pathname:///resources/Machina-Page-by-Page.pdf'}],
  },
  {
    title: 'Customer README',
    meta: 'PDF',
    blurb: 'The customer documentation package in one printable file.',
    files: [{label: 'Download PDF', href: 'pathname:///resources/Machina-Customer-README.pdf'}],
  },
  {
    title: 'Subscription model',
    meta: 'Markdown',
    blurb: 'Plans, support levels and terms for production use. Non-production use is free under the Zyvor Production License.',
    files: [{label: 'Read on GitHub', href: `${REPO}/blob/main/docs/SUBSCRIPTION-MODEL.md`}],
  },
  {
    title: 'Social card',
    meta: 'JPEG · 1600x900',
    blurb: 'For LinkedIn and X: install, run, console, fleet and operate in five steps.',
    thumb: '/machina-social-card.jpg',
    files: [{label: 'Download JPEG', href: 'pathname:///machina-social-card.jpg'}],
  },
  {
    title: 'Share card',
    meta: 'JPEG · 1200x630',
    blurb: 'The README hero and Open Graph preview, with a live dashboard capture.',
    thumb: '/machina-share-card.jpg',
    files: [{label: 'Download JPEG', href: 'pathname:///machina-share-card.jpg'}],
  },
];

function Thumb({src, alt}: {src: string; alt: string}) {
  return <img src={useBaseUrl(src)} alt={alt} className={styles.assetThumb} loading="lazy" />;
}

export default function Resources(): ReactNode {
  return (
    <Layout title="Resources" description="Machina guides, PDFs and brand assets to download.">
      <header className={styles.header}>
        <div className="container">
          <div className="mx-eyebrow">Resources</div>
          <Heading as="h1" className={styles.title}>
            Guides and <span className="mx-gradient">downloads.</span>
          </Heading>
          <p className={styles.lede}>Printable guides for evaluators and operators, plus the cards for sharing Machina.</p>
        </div>
      </header>
      <main className="container mx-section">
        <div className={styles.assetGrid}>
          {ASSETS.map((a, i) => (
            <Reveal key={a.title} delay={(i % 2) * 80}>
              <div className={clsx('mx-card', styles.asset)}>
                {a.thumb && <Thumb src={a.thumb} alt={a.title} />}
                <div className={styles.assetMeta}>{a.meta}</div>
                <Heading as="h3">{a.title}</Heading>
                <p>{a.blurb}</p>
                <div className={styles.assetLinks}>
                  {a.files.map((f) => (
                    <Link key={f.href} className="mx-btn mx-btn--primary" href={f.href}>
                      {f.label}
                    </Link>
                  ))}
                </div>
              </div>
            </Reveal>
          ))}
        </div>
        <Reveal className="text--center">
          <p className="mx-lede mx-center" style={{marginTop: '4rem'}}>
            Want a walkthrough on your own hardware?
          </p>
          <Link className="mx-btn mx-btn--primary" href={DEMO_URL}>
            Book a demo
          </Link>
        </Reveal>
      </main>
    </Layout>
  );
}
