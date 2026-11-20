// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import {useState} from 'react';
import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import Reveal from '@site/src/components/Reveal';
import Counter from '@site/src/components/Counter';
import BrowserFrame from '@site/src/components/BrowserFrame';
import CopyCommand from '@site/src/components/CopyCommand';
import {DEMO_URL, FEATURES, INSTALL, POC_URL, REPO, SHOTS, WHATS_NEW, WHY} from '@site/src/data/product';

import styles from './index.module.css';

function Hero() {
  return (
    <header className={styles.hero}>
      <div className={styles.heroGrid} aria-hidden />
      <div className={styles.heroGlow} aria-hidden />
      <div className={clsx('container', styles.heroInner)}>
        <div className={styles.heroCopy}>
          <div className={styles.heroBadge}>
            <span className={styles.heroDot} /> Free for non-production · Zyvor Production License
          </div>
          <Heading as="h1" className={styles.wordmark}>
            machina
          </Heading>
          <p className={styles.tagline}>
            Your metal. Your cloud.
            <br />
            <span className="mx-gradient">One control plane.</span>
          </p>
          <p className={styles.heroLede}>
            The private cloud you can install before lunch. VMs, browser consoles, fleet HA/DRS, a self-service
            cloud, a native eBPF datapath and AI operations, from a handful of Rust services on plain Linux + KVM.
          </p>
          <div className={styles.heroCtas}>
            <Link className="mx-btn mx-btn--primary" to="/docs/getting-started/quickstart">
              Get started
            </Link>
            <Link className="mx-btn mx-btn--light" href={DEMO_URL}>
              Book a demo
            </Link>
            <Link className="mx-btn mx-btn--ghost" href={REPO}>
              GitHub
            </Link>
          </div>
          <CopyCommand command={INSTALL} />
        </div>
        <div className={styles.heroMedia}>
          <BrowserFrame src="/machina-dashboard-dark.png" alt="Machina Mission Control dashboard" eager className={styles.heroFrame} />
          <p className={styles.heroCaption}>Live capture from a real Ubuntu 26.04 deployment, not a mockup.</p>
        </div>
      </div>
    </header>
  );
}

function Stats() {
  const stats = [
    {value: 4, suffix: '', label: 'core Rust services'},
    {value: 1000, suffix: '+', label: 'REST routes'},
    {value: 1, suffix: '', label: 'command to install'},
    {value: 4, suffix: '', label: 'console protocols'},
  ];
  return (
    <section className={styles.stats}>
      <div className="container">
        <Reveal className={styles.statsGrid}>
          {stats.map((s) => (
            <div key={s.label} className={styles.stat}>
              <div className={clsx(styles.statValue, 'mx-gradient--light')}>
                <Counter to={s.value} suffix={s.suffix} />
              </div>
              <div className={styles.statLabel}>{s.label}</div>
            </div>
          ))}
        </Reveal>
      </div>
    </section>
  );
}

function WhatsNew() {
  return (
    <section className={clsx('mx-section', styles.news)}>
      <div className="container">
        <Reveal className="mx-center text--center">
          <div className="mx-eyebrow">What's new</div>
          <Heading as="h2">
            Fix it, contain it, <span className="mx-gradient--light">undo it.</span>
          </Heading>
          <p className="mx-lede mx-center">The latest releases, straight from the commit log.</p>
        </Reveal>
        <div className={styles.newsGrid}>
          {WHATS_NEW.map((n, i) => (
            <Reveal key={n.title} delay={i * 60}>
              <div className={clsx('mx-card', styles.newsCard)}>
                <span className={styles.newsTag}>New</span>
                <h3>{n.title}</h3>
                <p>{n.body}</p>
              </div>
            </Reveal>
          ))}
        </div>
      </div>
    </section>
  );
}

function Why() {
  return (
    <section className="mx-section">
      <div className="container">
        <Reveal className="mx-center text--center">
          <div className="mx-eyebrow">Why Machina</div>
          <Heading as="h2">
            The cloud you own, <span className="mx-gradient--light">without the sprawl.</span>
          </Heading>
        </Reveal>
        <div className={styles.whyGrid}>
          {WHY.map((w, i) => (
            <Reveal key={w.problem} delay={i * 70}>
              <div className={clsx('mx-card', styles.whyCard)}>
                <p className={styles.whyProblem}>{w.problem}</p>
                <p className={styles.whyAnswer}>{w.answer}</p>
              </div>
            </Reveal>
          ))}
        </div>
      </div>
    </section>
  );
}

function Features() {
  return (
    <section className="mx-section mx-section--panel">
      <div className="container">
        <Reveal className="mx-center text--center">
          <div className="mx-eyebrow">The platform</div>
          <Heading as="h2">
            Run it. Reach it. Scale it. Secure it. <span className="mx-gradient--light">Let AI watch it.</span>
          </Heading>
        </Reveal>
        {FEATURES.map((f, i) => (
          <Reveal key={f.title}>
            <div className={clsx(styles.feature, i % 2 === 1 && styles.featureFlip)}>
              <div className={styles.featureCopy}>
                <div className="mx-eyebrow">{f.eyebrow}</div>
                <Heading as="h3" className={styles.featureTitle}>
                  {f.title}
                </Heading>
                <p className={styles.featureBody}>{f.body}</p>
                <ul className={styles.featureList}>
                  {f.bullets.map((b) => (
                    <li key={b}>{b}</li>
                  ))}
                </ul>
                <Link to={f.doc} className={styles.featureLink}>
                  Learn more →
                </Link>
              </div>
              <div className={styles.featureMedia}>
                <BrowserFrame src={f.shot.src} alt={f.shot.title} url={f.shot.url} />
              </div>
            </div>
          </Reveal>
        ))}
      </div>
    </section>
  );
}

function VsOpenStack() {
  const card = useBaseUrl('/readme-vs-openstack.jpg');
  return (
    <section className={clsx('mx-section', styles.dark)}>
      <div className="container">
        <Reveal className="mx-center text--center">
          <div className={clsx('mx-eyebrow', styles.darkEyebrow)}>Machina vs OpenStack</div>
          <Heading as="h2">
            Same private-cloud primitives.
            <br />
            <span className="mx-gradient">A fraction of the moving parts.</span>
          </Heading>
          <p className={clsx('mx-lede mx-center', styles.darkLede)}>
            Four Rust services and embedded SQLite (or PostgreSQL) instead of nine-plus services, a Galera cluster and a message bus. Choose
            OpenStack for thousands of tenants; choose Machina for the fleets you own.
          </p>
        </Reveal>
        <Reveal>
          <img src={card} alt="Machina vs OpenStack" className={styles.wideCard} loading="lazy" />
        </Reveal>
        <div className="text--center">
          <Link className="mx-btn mx-btn--light" to="/vs-openstack">
            See the full comparison
          </Link>{' '}
          <Link className="mx-btn mx-btn--ghost" to="/docs/getting-started/from-openstack">
            Replace OpenStack: the guide
          </Link>
        </div>
      </div>
    </section>
  );
}

const DEPLOYS = [
  {key: 'single', label: 'One host', src: '/anim/deploy-single-host.svg', alt: 'Deploy Machina on one host with ./machinactl deploy', doc: '/docs/getting-started/quickstart'},
  {key: 'fleet', label: 'A fleet', src: '/anim/deploy-fleet.svg', alt: 'Add hosts; the controller fails a VM over to a healthy host', doc: '/docs/core-concepts/fleet-ha'},
  {key: 'pg', label: 'PostgreSQL', src: '/anim/deploy-postgres.svg', alt: 'Move the controller from SQLite to PostgreSQL', doc: '/docs/getting-started/database'},
  {key: 'ec2', label: 'EC2 API', src: '/anim/ec2-launch.svg', alt: 'Launch instances with the aws CLI', doc: '/docs/core-concepts/ec2-api'},
];

function Deploy() {
  const [active, setActive] = useState('single');
  const d = DEPLOYS.find((x) => x.key === active) ?? DEPLOYS[0];
  const src = useBaseUrl(d.src);
  return (
    <section className="mx-section mx-section--panel">
      <div className="container">
        <Reveal className="mx-center text--center">
          <div className="mx-eyebrow">Deploy</div>
          <Heading as="h2">
            From one command <span className="mx-gradient--light">to a fleet.</span>
          </Heading>
          <p className="mx-lede mx-center">Start on a single host with embedded SQLite, add hypervisors, move to PostgreSQL when the fleet grows.</p>
        </Reveal>
        <div className="text--center" role="tablist" aria-label="Deployment stories">
          {DEPLOYS.map((x) => (
            <button
              key={x.key}
              role="tab"
              aria-selected={x.key === active}
              className={clsx('mx-btn', x.key === active ? 'mx-btn--primary' : 'mx-btn--ghost-dark')}
              style={{margin: '0 6px 8px'}}
              onClick={() => setActive(x.key)}>
              {x.label}
            </button>
          ))}
        </div>
        <Reveal>
          <img src={src} alt={d.alt} className={styles.wideCard} />
        </Reveal>
        <div className="text--center">
          <Link to={d.doc}>Read the guide →</Link>
        </div>
      </div>
    </section>
  );
}

function PlatformFacts() {
  const base = useBaseUrl('/');
  const items = [
    {img: '/readme-database.jpg', alt: 'SQLite or PostgreSQL', title: 'SQLite or PostgreSQL', body: 'Embedded SQLite by default. PostgreSQL for hundreds of machines or several controllers, with a one-command migration and a way back.', to: '/docs/getting-started/database'},
    {img: '/readme-ec2.jpg', alt: 'EC2-compatible API', title: 'EC2-compatible API', body: 'awscli, boto3 and Terraform work against the controller with an endpoint override. Every action is applied, recorded or refused by name.', to: '/docs/core-concepts/ec2-api'},
  ];
  return (
    <section className="mx-section">
      <div className="container">
        <Reveal className="mx-center text--center">
          <div className="mx-eyebrow">Under the hood</div>
          <Heading as="h2">Use the tools you already know.</Heading>
        </Reveal>
        <div style={{display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))', gap: 24, marginTop: 32}}>
          {items.map((it, i) => (
            <Reveal key={it.title} delay={i * 90}>
              <div className={clsx('mx-card', styles.port)}>
                <img src={base + it.img.slice(1)} alt={it.alt} loading="lazy" style={{width: '100%', borderRadius: 12}} />
                <div className={styles.portHead}><code>{it.title}</code></div>
                <p>{it.body}</p>
                <Link to={it.to}>Read more →</Link>
              </div>
            </Reveal>
          ))}
        </div>
      </div>
    </section>
  );
}

function Architecture() {
  const card = useBaseUrl('/readme-architecture.jpg');
  const parts = [
    {name: 'machina-daemon', port: ':5092', body: 'Single-host REST + WebSocket API, auth, RBAC and console proxies. Serves the web UI.'},
    {name: 'machina-controller', port: ':5093', body: 'Fleet, HA, DRS, Fleet Cloud and Zyra AI. Embedded SQLite or PostgreSQL, optional NATS.'},
    {name: 'machina-agent', port: ':50051', body: 'gRPC over TLS on every hypervisor. Executes libvirt and eBPF operations for the fleet.'},
    {name: 'machina-bpfd', port: 'unix socket', body: 'Root eBPF service on every host: load balancing, CNI datapath, shield, isolation and telemetry.'},
  ];
  return (
    <section className="mx-section">
      <div className="container">
        <Reveal className="mx-center text--center">
          <div className="mx-eyebrow">How it fits together</div>
          <Heading as="h2">
            Four services. <span className="mx-gradient--light">One private cloud.</span>
          </Heading>
        </Reveal>
        <Reveal>
          <img src={card} alt="Machina architecture" className={styles.wideCard} loading="lazy" />
        </Reveal>
        <div className={styles.ports}>
          {parts.map((p, i) => (
            <Reveal key={p.name} delay={i * 90}>
              <div className={clsx('mx-card', styles.port)}>
                <div className={styles.portHead}>
                  <code>{p.name}</code>
                  <span>{p.port}</span>
                </div>
                <p>{p.body}</p>
              </div>
            </Reveal>
          ))}
        </div>
        <div className="text--center">
          <Link to="/docs/core-concepts/architecture">Read the architecture guide →</Link>
        </div>
      </div>
    </section>
  );
}

function Strip() {
  return (
    <section className="mx-section mx-section--panel">
      <div className="container">
        <Reveal className="mx-center text--center">
          <div className="mx-eyebrow">See it live</div>
          <Heading as="h2">A real product, not a mockup.</Heading>
          <p className="mx-lede mx-center">
            Captured against a live deployment. <Link to="/gallery">Open the full gallery →</Link>
          </p>
        </Reveal>
      </div>
      <div className={styles.strip}>
        {SHOTS.map((s) => (
          <Link key={s.src} to="/gallery" className={styles.stripItem}>
            <BrowserFrame src={s.src} alt={s.title} url={s.url} />
            <span>{s.title}</span>
          </Link>
        ))}
      </div>
    </section>
  );
}

function Trust() {
  return (
    <section className="mx-section">
      <div className="container">
        <Reveal className={styles.trust}>
          <div>
            <div className="mx-eyebrow">Open by default</div>
            <Heading as="h2" className={styles.trustTitle}>
              Source you can read. A license you can understand.
            </Heading>
            <p className="mx-lede">
              Every source file carries an SPDX header checked in CI, the full git history is scanned for secrets,
              and the whole stack builds from this repository. Evaluate, develop, test and run your lab for free.
            </p>
            <Link to="/docs/license">How licensing works →</Link>
          </div>
          <div className={styles.badges}>
            <img src={`${REPO}/actions/workflows/ci.yml/badge.svg`} alt="CI status" />
            <img src="https://img.shields.io/badge/License-Zyvor%20Production%20v1.0-0071e3.svg" alt="Zyvor Production License v1.0" />
            <img src="https://img.shields.io/github/stars/zyvorai/zyvor-machina?style=social" alt="GitHub stars" />
          </div>
        </Reveal>
      </div>
    </section>
  );
}

function EnterpriseCta() {
  return (
    <section className={clsx(styles.cta)}>
      <div className={styles.heroGlow} aria-hidden />
      <div className="container text--center">
        <Reveal>
          <Heading as="h2" className={styles.ctaTitle}>
            Ready to run <span className="mx-gradient">your own cloud?</span>
          </Heading>
          <p className={styles.ctaLede}>
            Non-production use is free. Production runs on an annual enterprise subscription with updates, upgrades and
            the support level you choose.
          </p>
          <div className={styles.heroCtas} style={{justifyContent: 'center'}}>
            <Link className="mx-btn mx-btn--primary" href={DEMO_URL}>
              Book a demo
            </Link>
            <Link className="mx-btn mx-btn--light" href={POC_URL}>
              Start a 30-day PoC
            </Link>
            <Link className="mx-btn mx-btn--ghost" href="mailto:sales@zyvor.dev">
              sales@zyvor.dev
            </Link>
          </div>
        </Reveal>
      </div>
    </section>
  );
}

export default function Home(): ReactNode {
  return (
    <Layout
      title="Machina — private cloud on KVM"
      description="VMs, browser consoles, fleet HA/DRS, Fleet Cloud, a native eBPF datapath and Zyra AI operations from a handful of Rust services. An OpenStack alternative you can install in one command.">
      <Hero />
      <main>
        <Stats />
        <WhatsNew />
        <Why />
        <Features />
        <VsOpenStack />
        <Deploy />
        <PlatformFacts />
        <Architecture />
        <Strip />
        <Trust />
        <EnterpriseCta />
      </main>
    </Layout>
  );
}
