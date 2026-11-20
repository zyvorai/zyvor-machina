// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export const ZYVOR_URL = 'https://zyvor.dev';
export const ZYVOR_BRAND = 'Zyvor';
export const ZYVOR_COPY = '© 2026';
export const ZYVOR_LINE = `zyvor.dev · ${ZYVOR_COPY}`;

type FooterProps = {
  className?: string;
  /** Host OS pretty name (e.g. Rocky Linux 9.4) — shown when provided. */
  hostOs?: string;
  /** @deprecated Ignored — footer is zyvor.dev · © 2026 only. */
  product?: string;
};

/** Page footer — transparent; shows the daemon host OS line only, when present. */
function ZyvorFooter({ className = '', hostOs }: FooterProps) {
  if (!hostOs) return null;
  return (
    <footer
      className={`zyvor-footer shrink-0 py-3 text-center bg-transparent border-0 ${className}`.trim()}
      style={{ marginTop: 'auto' }}
      role="contentinfo"
    >
      <div
        className="text-[11px] text-[var(--text-muted)]"
        title="Daemon host operating system"
      >
        {hostOs}
      </div>
    </footer>
  );
}

export default ZyvorFooter;
