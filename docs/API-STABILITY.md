# API stability and deprecation policy

What you can rely on when you automate Machina (Terraform, SDKs, scripts).

## Versioning
- The HTTP API is versioned in the path: **`/api/v1`**. Everything under `/api/v1` is covered by this policy.
- Machina releases use semantic versions. A change that breaks `/api/v1` ships only with a new path version (`/api/v2`),
  never inside a minor or patch release.

## Stable (compatible changes only)
Within `v1` we may: add endpoints, add optional request fields, add response fields, add enum values you should treat as
"unknown value", and add new error codes. Clients **must ignore unknown response fields** (the SDKs do).
We will not, without a deprecation period: remove or rename an endpoint, field or enum value; change a field's type or
meaning; make an optional field required; or change an HTTP status for an existing outcome.

## Deprecation
1. The endpoint or field is marked deprecated in the changelog and, where possible, answered with a `Deprecation` header.
2. It keeps working for **at least two minor releases and 6 months**, whichever is longer.
3. It is removed only in the next major API version.

## Not covered
- Endpoints documented as experimental, and anything under `/api/v1/ai/*`, `/api/v1/mcp` and `/api/v1/zeus-*` while the
  AI and security-fabric features are still evolving (they follow the compatible-changes rule but may change faster).
- Response *ordering*, the text of human-readable messages, and internal task `operation` names.
- Direct database access and the agent gRPC protocol (agents and controller must be within one minor version).

## Authentication
API keys (Settings → API keys) sent as `Authorization: Bearer <key>` are the supported way to automate. Roles are
`viewer`, `operator` and `admin`; creating and deleting machines needs `operator`.

## Clients
The [Go SDK](../sdk/go), [Python SDK](../sdk/python) and [Terraform provider](../terraform/provider) follow this policy:
they are tested against the current controller in CI and ignore unknown fields.
