// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const REPO = 'https://github.com/zyvorai/zyvor-machina';

const config: Config = {
  title: 'Machina',
  tagline: 'Your metal. Your cloud. One control plane.',
  favicon: 'img/favicon.svg',

  future: {
    v4: true,
  },

  url: 'https://zyvorai.github.io',
  baseUrl: '/zyvor-machina/',

  organizationName: 'zyvorai',
  projectName: 'zyvor-machina',

  onBrokenLinks: 'throw',

  markdown: {
    mermaid: true,
    hooks: {
      onBrokenMarkdownLinks: 'warn',
    },
  },
  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  // Screenshots and cards live once in docs/ux and docs/social; README and site share them.
  staticDirectories: ['static', '../docs/ux', '../docs/social'],

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl: `${REPO}/tree/main/website/`,
        },
        blog: {
          path: 'blog',
          routeBasePath: 'blog',
          showReadingTime: true,
          blogSidebarCount: 'ALL',
          feedOptions: {type: ['rss', 'atom'], title: 'Machina blog'},
        },
        theme: {
          customCss: ['./src/css/custom.css', './src/css/machina.css'],
        },
      } satisfies Preset.Options,
    ],
  ],

  themes: [
    '@docusaurus/theme-mermaid',
    [
      '@easyops-cn/docusaurus-search-local',
      {
        hashed: true,
        indexBlog: true,
        docsRouteBasePath: '/docs',
        highlightSearchTermsOnTargetPage: true,
      },
    ],
  ],

  themeConfig: {
    image: 'machina-share-card.jpg',
    metadata: [
      {name: 'keywords', content: 'KVM, libvirt, private cloud, OpenStack alternative, VMware alternative, hypervisor, Rust'},
      {name: 'twitter:card', content: 'summary_large_image'},
    ],
    colorMode: {
      defaultMode: 'dark',
      respectPrefersColorScheme: false,
    },
    navbar: {
      title: 'Machina',
      logo: {
        alt: 'Machina',
        src: 'img/favicon.svg',
      },
      items: [
        {type: 'docSidebar', sidebarId: 'docsSidebar', position: 'left', label: 'Docs'},
        {
          label: 'Compare',
          position: 'left',
          items: [
            {to: '/vs-azure-local', label: 'vs Azure Local'},
            {to: '/vs-vmware', label: 'vs VMware'},
            {to: '/vs-nutanix', label: 'vs Nutanix'},
            {to: '/vs-proxmox', label: 'vs Proxmox VE'},
            {to: '/vs-openstack', label: 'vs OpenStack'},
          ],
        },
        {to: '/blog', label: 'Blog', position: 'left'},
        {to: '/gallery', label: 'Gallery', position: 'left'},
        {to: '/resources', label: 'Resources', position: 'left'},
        {href: REPO, label: 'GitHub', position: 'right'},
        {
          href: 'https://zyvor.dev/schedule?utm_source=pages&utm_medium=machina&utm_campaign=navbar',
          label: 'Book a demo',
          position: 'right',
          className: 'navbar-cta',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            {label: 'Quickstart', to: '/docs/getting-started/quickstart'},
            {label: 'Architecture', to: '/docs/core-concepts/architecture'},
            {label: 'Native eBPF', to: '/docs/networking/ebpf-overview'},
            {label: 'Security', to: '/docs/operations/security'},
            {label: 'Troubleshooting', to: '/docs/operations/troubleshooting'},
            {label: 'Machina vs OpenStack', to: '/vs-openstack'},
            {label: 'Machina vs Azure Local', to: '/vs-azure-local'},
            {label: 'Blog', to: '/blog'},
          ],
        },
        {
          title: 'Project',
          items: [
            {label: 'GitHub', href: REPO},
            {label: 'Changelog', href: `${REPO}/blob/main/CHANGELOG.md`},
            {label: 'License', href: `${REPO}/blob/main/LICENSE`},
            {label: 'Contributing', href: `${REPO}/blob/main/CONTRIBUTING.md`},
          ],
        },
        {
          title: 'Zyvor',
          items: [
            {label: 'zyvor.dev', href: 'https://zyvor.dev'},
            {label: 'sales@zyvor.dev', href: 'mailto:sales@zyvor.dev'},
            {label: 'Subscription model', href: `${REPO}/blob/main/docs/SUBSCRIPTION-MODEL.md`},
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Zyvor AI Labs. Zyvor Production License v1.0. OpenStack is a trademark of the Open Infrastructure Foundation.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['bash', 'toml'],
    },
    mermaid: {
      theme: {light: 'neutral', dark: 'dark'},
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
