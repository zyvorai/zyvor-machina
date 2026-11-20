// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type {ReactNode} from 'react';
import ComparisonPage from '@site/src/components/ComparisonPage';
import {PROXMOX} from '@site/src/data/comparisons';

export default function VsProxmox(): ReactNode {
  return <ComparisonPage c={PROXMOX} />;
}
