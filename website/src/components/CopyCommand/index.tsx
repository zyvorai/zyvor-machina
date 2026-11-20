// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import {useState} from 'react';
import type {ReactNode} from 'react';
import styles from './styles.module.css';

export default function CopyCommand({command}: {command: string}): ReactNode {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div className={styles.term}>
      <span className={styles.prompt}>$</span>
      <code className={styles.cmd}>{command}</code>
      <button type="button" className={styles.copy} onClick={copy} aria-label="Copy command">
        {copied ? 'Copied' : 'Copy'}
      </button>
    </div>
  );
}
