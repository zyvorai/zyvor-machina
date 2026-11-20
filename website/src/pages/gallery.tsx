// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import {useEffect, useState} from 'react';
import type {ReactNode} from 'react';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import Reveal from '@site/src/components/Reveal';
import BrowserFrame from '@site/src/components/BrowserFrame';
import {SHOTS} from '@site/src/data/product';
import styles from './subpage.module.css';

const CARDS = [
  {src: '/readme-capabilities.jpg', title: 'Capabilities at a glance'},
  {src: '/readme-architecture.jpg', title: 'How the core services fit together'},
  {src: '/readme-vs-openstack.jpg', title: 'Machina vs OpenStack'},
  {src: '/readme-hero.jpg', title: 'Run your own cloud on your own hardware'},
  {src: '/readme-replace-openstack.jpg', title: 'Replace OpenStack, service by service'},
  {src: '/readme-database.jpg', title: 'SQLite or PostgreSQL'},
  {src: '/readme-ec2.jpg', title: 'EC2-compatible API'},
  {src: '/machina-social-card.jpg', title: 'Run, secure, scale, operate'},
];

type Open = {src: string; title: string} | null;

function Lightbox({open, onClose}: {open: Open; onClose: () => void}) {
  const src = useBaseUrl(open?.src ?? '/');
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  if (!open) return null;
  return (
    <figure className={styles.lightbox} onClick={onClose} role="dialog" aria-label={open.title}>
      <img src={src} alt={open.title} />
      <figcaption>{open.title}</figcaption>
    </figure>
  );
}

function CardImage({src, title, onOpen}: {src: string; title: string; onOpen: () => void}) {
  const resolved = useBaseUrl(src);
  return (
    <button type="button" className={styles.galleryItem} onClick={onOpen}>
      <img src={resolved} alt={title} className={styles.card} style={{marginBottom: 0}} loading="lazy" />
      <h3>{title}</h3>
    </button>
  );
}

export default function Gallery(): ReactNode {
  const [open, setOpen] = useState<Open>(null);
  return (
    <Layout title="Gallery" description="Screens from a live Machina deployment: Mission Control, VMs, live console, HA, Fleet Cloud and Zyra AI.">
      <header className={styles.header}>
        <div className="container">
          <div className="mx-eyebrow">Gallery</div>
          <Heading as="h1" className={styles.title}>
            The product, <span className="mx-gradient">up close.</span>
          </Heading>
          <p className={styles.lede}>
            Every screen below was captured from a real Machina deployment on Ubuntu 26.04 running live KVM guests.
            Click any image to enlarge it.
          </p>
        </div>
      </header>
      <main className="container mx-section">
        <div className={styles.galleryGrid}>
          {SHOTS.map((s, i) => (
            <Reveal key={s.src} delay={(i % 2) * 90}>
              <button type="button" className={styles.galleryItem} onClick={() => setOpen({src: s.src, title: s.title})}>
                <BrowserFrame src={s.src} alt={s.title} url={s.url} />
                <h3>{s.title}</h3>
                <p>{s.caption}</p>
              </button>
            </Reveal>
          ))}
        </div>

        <Reveal className="mx-center text--center" >
          <div style={{marginTop: '5rem'}} className="mx-eyebrow">
            Explainers
          </div>
          <Heading as="h2">The story in four cards.</Heading>
        </Reveal>
        <div className={styles.cards}>
          {CARDS.map((c) => (
            <Reveal key={c.src}>
              <CardImage src={c.src} title={c.title} onOpen={() => setOpen(c)} />
            </Reveal>
          ))}
        </div>
      </main>
      <Lightbox open={open} onClose={() => setOpen(null)} />
    </Layout>
  );
}
