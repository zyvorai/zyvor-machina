# Automation

Every path here ends in the same place as the web wizard: the controller's one-paste join command, which installs
libvirt/QEMU, fetches the agent from the controller, pins its CA and joins over mutual TLS. Prerequisite on the
controller (see [QUICKSTART](../docs/QUICKSTART.md), step 2): `MACHINA_CONTROLLER_TLS_ADDR` set and `machinactl dist publish` run.

| Tool | Where | Use it to |
|------|-------|-----------|
| Ansible | [`ansible/`](ansible) | install the controller from release packages and join any number of nodes (`ansible-playbook -i inventory.ini site.yml`) |
| cloud-init | [`cloud-init/`](cloud-init) | boot a node that joins by itself (`./render.sh --from-controller user@ctl > user-data`) |
| Terraform / OpenTofu | [`terraform/join-nodes/`](terraform/join-nodes) | join existing machines over SSH |
| Docker Compose | [`compose/`](compose) | a controller-only demo from a release binary |
| Helm | [`../contrib/k8s/machina`](../contrib/k8s/machina) | the daemon's identity/config pod only; it does not run libvirt, so it is not a way to add nodes |

Join tokens are single use and expire after one hour; each tool mints a fresh one when it runs. Nothing here stores an admin password or token in a file in this repository.
