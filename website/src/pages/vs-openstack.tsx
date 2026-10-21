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

const ROWS: {topic: string; machina: string; openstack: string}[] = [
  {topic: 'Services to run', machina: '4 Rust services: daemon, controller, agent, machina-bpfd', openstack: '9+ services: Keystone, Nova, Neutron, Glance, Cinder, Placement, Horizon, Heat, Octavia'},
  {topic: 'Backing infrastructure', machina: 'Embedded SQLite, or PostgreSQL for large fleets; NATS only if you want it', openstack: 'MariaDB/Galera, RabbitMQ, Memcached'},
  {topic: 'Install', machina: './machinactl deploy on one host; deploy-remote.sh for the next', openstack: 'Kolla-Ansible or OpenStack-Ansible deployment project'},
  {topic: 'Smallest useful footprint', machina: 'A single KVM host', openstack: 'A multi-node control plane'},
  {topic: 'Self-service primitives', machina: 'Flavors, images, volumes, security groups, keypairs, stacks, load balancers in Fleet Cloud', openstack: 'Nova, Glance, Cinder, Neutron, Heat, Octavia'},
  {topic: 'Load balancer data plane', machina: 'Maglev XDP service LB + iptables member rules; no amphora VM', openstack: 'Amphora VMs (Octavia)'},
  {topic: 'Network datapath and security', machina: 'Native eBPF (XDP, TC, cgroup, BPF-LSM): CNI, DDoS shield, VM isolation, lease-gated enforcement', openstack: 'Neutron agents with OVS/OVN; security groups via iptables or OVS flows'},
  {topic: 'Browser consoles', machina: 'noVNC, SPICE, serial and SSH built into the daemon', openstack: 'noVNC/SPICE proxy services'},
  {topic: 'HA failover and DRS', machina: 'Built into the controller', openstack: 'Separate projects: Masakari (instance HA), Watcher (rebalancing)'},
  {topic: 'AI operations', machina: 'Zyra AI: diagnostics, incidents, rightsizing, approvals', openstack: 'Not included'},
  {topic: 'Containers and Kubernetes', machina: 'Podman containers and pods; KubeVirt inventory and migration', openstack: 'Zun / Magnum (separate projects)'},
  {topic: 'Implementation language', machina: 'Rust (backend), React (UI)', openstack: 'Python'},
];

export default function VsOpenStack(): ReactNode {
  const card = useBaseUrl('/readme-vs-openstack.jpg');
  return (
    <Layout title="Machina vs OpenStack" description="How Machina compares with a typical OpenStack IaaS deployment: services, install, consoles, HA/DRS, AI operations.">
      <header className={styles.header}>
        <div className="container">
          <div className="mx-eyebrow">Comparison</div>
          <Heading as="h1" className={styles.title}>
            Machina vs <span className="mx-gradient">OpenStack</span>
          </Heading>
          <p className={styles.lede}>
            Same private-cloud primitives, a fraction of the moving parts. Here is an honest side-by-side, including
            where OpenStack is still the better choice.
          </p>
        </div>
      </header>
      <main className="container mx-section">
        <Reveal>
          <img src={card} alt="Machina vs OpenStack" className={styles.card} />
        </Reveal>

        <Reveal>
          <Heading as="h2">Service by service</Heading>
          <img src={useBaseUrl('/readme-replace-openstack.jpg')} alt="Each OpenStack service and what replaces it in Machina" className={styles.card} loading="lazy" />
          <p>
            Machina is not a drop-in: there is no Nova or Neutron API, so tools move to Machina's REST API or its
            EC2-compatible API, and guests move as disk images. There is no OpenStack importer.{' '}
            <Link to="/docs/getting-started/from-openstack">Read the migration guide →</Link>
          </p>
        </Reveal>

        <Reveal>
          <table className="mx-table">
            <thead>
              <tr>
                <th />
                <th>Machina</th>
                <th>OpenStack (typical IaaS)</th>
              </tr>
            </thead>
            <tbody>
              {ROWS.map((r) => (
                <tr key={r.topic}>
                  <td>{r.topic}</td>
                  <td className="mx-win">{r.machina}</td>
                  <td>{r.openstack}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Reveal>

        <div className={styles.twoUp}>
          <Reveal>
            <div className={clsx('mx-card', styles.panel)}>
              <Heading as="h3">Choose Machina when</Heading>
              <ul>
                <li>You own the fleet: a lab, a branch, a sovereign region, an edge site.</li>
                <li>You are leaving VMware and want HA, DRS and live migration on open KVM.</li>
                <li>One or two people need to install, understand and upgrade the whole stack.</li>
                <li>You want AI-assisted operations with a human approval in the loop.</li>
              </ul>
            </div>
          </Reveal>
          <Reveal delay={90}>
            <div className={clsx('mx-card', styles.panel)}>
              <Heading as="h3">Choose OpenStack when</Heading>
              <ul>
                <li>You run thousands of tenants with hard multi-tenant isolation at hyperscale.</li>
                <li>You need Neutron-grade SDN breadth (many ML2 drivers, complex overlays).</li>
                <li>You depend on its ecosystem of distributions, vendors and integrations.</li>
                <li>You already staff a team that operates it well.</li>
              </ul>
            </div>
          </Reveal>
        </div>

        <Reveal>
          <div className={clsx('mx-card', styles.callout)}>
            <div>
              <Heading as="h3">Moving off VMware or OpenStack?</Heading>
              <p>
                h2kvm and Transiva convert guests into KVM, and GuestKit checks them offline before cutover. Machina
                imports the result as ordinary libvirt domains.
              </p>
            </div>
            <div className={styles.calloutCtas}>
              <Link className="mx-btn mx-btn--primary" href={DEMO_URL}>
                Plan a migration
              </Link>
              <Link className="mx-btn mx-btn--ghost-dark" href={REPO}>
                Read the source
              </Link>
            </div>
          </div>
        </Reveal>

        <p className={styles.fine}>
          OpenStack is a trademark of the Open Infrastructure Foundation. The OpenStack column describes a typical
          IaaS deployment; individual deployments vary.
        </p>
      </main>
    </Layout>
  );
}
