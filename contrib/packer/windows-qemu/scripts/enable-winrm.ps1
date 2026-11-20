# SECURITY: this intentionally configures WinRM insecurely (unencrypted
# transport + Basic auth + an open firewall rule) so Packer can connect and
# provision this image unattended. This is only safe because the QEMU build
# VM is an isolated, throwaway build-time host. This must NOT persist into
# the shipped golden image — scripts/harden-on-first-boot.ps1 (run as a
# provisioner in windows.pkr.hcl) schedules a one-time task that reverts all
# of this on the first real boot of any VM cloned from the resulting image.
# Do not remove this script's insecure settings directly (e.g. to "fix" this
# in place) without verifying the Packer build's own WinRM communicator can
# still authenticate afterward — see windows.pkr.hcl's winrm_username/
# winrm_password and communicator settings.
Set-ExecutionPolicy Bypass -Scope LocalMachine -Force
winrm quickconfig -quiet
winrm set winrm/config/service '@{AllowUnencrypted="true"}'
winrm set winrm/config/service/auth '@{Basic="true"}'
Enable-PSRemoting -Force
Set-NetFirewallRule -Name "WINRM-HTTP-In-TCP" -Enabled True
