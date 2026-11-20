# Project-scoped API keys

## What it is
An API key restricted to named projects, so a CI job or a team can manage its own instances and nothing else.

## Configure
Create the key with `projects`. A scoped key has the operator or viewer role (never admin) and a unique name. The scope is
enforced whether or not `MACHINA_PROJECT_RBAC` is on, and survives rotation.

All calls below use two shell variables:

```bash
export API=https://HOST:5092/api/v1/platform/controller/api/v1   # controller API through the daemon proxy
export TOKEN=...                                                  # a JWT or API key (Settings -> API keys)
alias mc='curl -sk -H "Authorization: Bearer $TOKEN" -H "content-type: application/json"'
```

## Use
```bash
mc -X POST $API/api-keys -d '{"name":"ci-lab","role":"operator","projects":["lab"]}'   # the key is shown once
curl -sk -H "Authorization: Bearer $LAB_KEY" $API/cloud/instance-groups?project=lab       # allowed
curl -sk -H "Authorization: Bearer $LAB_KEY" $API/hosts                                   # 403 key_scope_forbidden
```
The key may call the cloud APIs (`/api/v1/cloud/...`) of its projects and the instance routes (`/api/v1/vms/{id}/...`) of
machines in those projects; the machine list is filtered to them. It launches through launch templates and instance groups.

## Check it works
Use the key against a project it holds and one it does not: the first works, the second answers 403 `key_scope_forbidden`.
Plain `POST /api/v1/vms` with a scoped key is refused. Unit tests:
`cargo test -p machina-controller scoped_key_tests scope_tests a_project_scoped_api_key`.

## Limits
Hosts, storage, security and other projects are out of reach by design. A scope cannot be edited in place: create a new key.
Admin keys cannot be scoped.
