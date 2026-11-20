#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Drop the per-test databases the PostgreSQL build's tests leave behind (machina_t_* and machina_tpl_*).
#   TEST_DATABASE_URL=postgres://machina:machina@127.0.0.1:5432/machina_test scripts/db/pg-test-clean.sh
set -euo pipefail
url="${TEST_DATABASE_URL:?set TEST_DATABASE_URL}"
for db in $(psql "$url" -Atc "SELECT datname FROM pg_database WHERE datname LIKE 'machina\\_t\\_%' OR datname LIKE 'machina\\_tpl\\_%'"); do
  psql "$url" -qc "DROP DATABASE IF EXISTS \"$db\" WITH (FORCE)"
done
echo cleaned
