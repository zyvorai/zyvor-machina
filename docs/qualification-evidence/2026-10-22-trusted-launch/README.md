# Trusted launch (claim C60): live run, 2026-10-22

`scripts/qualification/trusted-launch-live.sh` against the daemon of a real host (Ubuntu 26.04.1, libvirt 12.0.0, QEMU 10.2.1,
edk2-ovmf 2025.11-3ubuntu7.3, swtpm 0.10.1, kernel 7.0.0-38), build `0.3.1-lab2` (the release workflow run from the
`fix/lab-findings` branch, which carries the `--machine q35` fix). Result: PASS.

- `create.json`: the daemon's answer to the create.
- `domain-defined.xml`: the domain right after create (firmware auto-selection, SMM, TPM).
- `domain-running.xml`: after start (resolved `<loader ... secure='yes'>`, per-VM `<nvram>`).
- `swtpm.txt`: the `swtpm` process for the VM on the host.

The first run on this host failed (`virt-install` used a non-q35 machine; libvirt: *Unable to find 'efi' firmware that is
compatible with the current configuration*). That is why #202 passes `--machine q35` for Secure Boot VMs.
