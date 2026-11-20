// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type {ReactNode} from 'react';
import clsx from 'clsx';
import useBaseUrl from '@docusaurus/useBaseUrl';

type BrowserFrameProps = {
  src: string;
  alt: string;
  url?: string;
  className?: string;
  eager?: boolean;
};

export default function BrowserFrame({src, alt, url = 'https://machina:5092', className, eager}: BrowserFrameProps): ReactNode {
  const resolved = useBaseUrl(src);
  return (
    <div className={clsx('mx-frame', className)}>
      <div className="mx-frame__bar">
        <i />
        <i />
        <i />
        <em>{url}</em>
      </div>
      <img src={resolved} alt={alt} loading={eager ? 'eager' : 'lazy'} />
    </div>
  );
}
