// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Normalize noisy kubectl / client-go stderr for UI (repeated memcache lines, etc.).
 */
function dedupeKubectlLines(raw: string): string {
  const lines = raw.split(/\r?\n/).map((l) => l.trim()).filter(Boolean)
  const out: string[] = []
  for (const line of lines) {
    if (out.length === 0 || out[out.length - 1] !== line) out.push(line)
  }
  return out.join('\n')
}

type K8sClientErrorSummary = {
  /** Short human-readable summary for banners and compact toasts. */
  headline: string
  /** True when the cluster TLS cert is not trusted for the current kubeconfig. */
  tlsUnknownAuthority: boolean
  /** Deduplicated full text for copy / details panel. */
  dedupedDetail: string
}

/**
 * Interprets API error text from machina (kubectl stderr + code) for Kubernetes pages.
 */
export function summarizeK8sClientError(raw: string): K8sClientErrorSummary {
  const dedupedDetail = dedupeKubectlLines(raw)
  const lower = dedupedDetail.toLowerCase()

  const tlsUnknownAuthority =
    lower.includes('certificate signed by unknown authority') ||
    (lower.includes('tls:') &&
      (lower.includes('failed to verify certificate') || lower.includes('verify certificate')))

  const lines = dedupedDetail.split(/\n/)
  const lastMeaningful =
    [...lines].reverse().find((l) => l.length > 0 && !l.includes('memcache.go')) ?? lines[lines.length - 1] ?? dedupedDetail

  let headline = lastMeaningful.length > 280 ? `${lastMeaningful.slice(0, 277)}…` : lastMeaningful

  if (tlsUnknownAuthority) {
    headline =
      'kubectl cannot verify the API server TLS certificate (unknown authority). The kubeconfig on the machina host must trust the cluster CA, or you must use a kubeconfig that includes the correct certificate-authority-data.'
  }

  return { headline, tlsUnknownAuthority, dedupedDetail }
}

export const TLS_K8S_HINTS: string[] = [
  'machina-daemon probes your default kubectl config first, then auto-selects well-known admin kubeconfigs (e.g. /etc/rancher/k3s/k3s.yaml, RKE2, kubeadm) when the API is reachable with those files.',
  'Point KUBECONFIG (or ~/.kube/config) at a kubeconfig that contains certificate-authority-data (or certificate-authority: path) for this cluster.',
  'For k3s/RKE2, use the server-generated admin config (e.g. /etc/rancher/k3s/k3s.yaml or rke2.yaml) or merge its cluster CA into your user kubeconfig.',
  'Quick check on the machina host: kubectl config view --minify --raw and kubectl cluster-info.',
  'Lab-only (not for production): you can set insecure-skip-tls-verify: true on the cluster entry — understand the security trade-off.',
]
