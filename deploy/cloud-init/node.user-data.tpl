#cloud-config
# A new Machina compute node: boots, installs libvirt/QEMU, fetches the agent from the controller, joins.
# Render with ./render.sh (it puts a fresh single-use join command in place of @JOIN_COMMAND@).
package_update: true
packages:
  - curl
  - ca-certificates
write_files:
  - path: /root/machina-join.sh
    permissions: "0700"
    content: |
      #!/bin/bash
      set -euo pipefail
      @JOIN_COMMAND@
runcmd:
  - [bash, -c, "/root/machina-join.sh > /var/log/machina-join.log 2>&1; rm -f /root/machina-join.sh"]
