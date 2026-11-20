#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Generate new Machina feature decks (07–10) as PDFs and email stakeholders.
#
# HTML source: docs/client-presentations/ (hyper2kvm slide-deck format)
# SMTP: scripts/deploy-mailer.env or ../hypersdk-web/contact-mailer.env
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

export FEATURE_PDF_TO="${FEATURE_PDF_TO:-sibu@zyvor.dev}"
export FEATURE_PDF_CC="${FEATURE_PDF_CC:-ssahani@zyvor.dev}"

echo "══ Generate client-presentation PDFs (decks 07–11) ══"
chmod +x "${SCRIPT_DIR}/generate-client-presentation-pdfs.sh"
"${SCRIPT_DIR}/generate-feature-pdfs.sh"

echo ""
echo "══ Mail PDFs to ${FEATURE_PDF_TO} (cc ${FEATURE_PDF_CC}) ══"
python3 "${SCRIPT_DIR}/send-feature-pdfs.py"
