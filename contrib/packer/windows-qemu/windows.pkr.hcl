packer {
  required_plugins {
    qemu = {
      source  = "github.com/hashicorp/qemu"
      version = ">= 1.1.4"
    }
  }
}

source "qemu" "windows" {
  qemu_binary = "/usr/libexec/qemu-kvm"
  headless    = true

  iso_url      = "WinServer.iso"
  iso_checksum = "none"

  output_directory = "output-windows"
  vm_name          = "windows.qcow2"
  format           = "qcow2"

  disk_size = "51200"
  memory    = 4096
  cpus      = 2

  accelerator    = "kvm"
  disk_interface = "ide"

  communicator   = "winrm"
  winrm_username = "Administrator"
  # NOTE: this password must match Autounattend.xml's AdministratorPassword.
  # It is a build-time-only placeholder, not a secret — every VM cloned from
  # the resulting golden image ships with this well-known local Administrator
  # password baked in until it is rotated downstream (e.g. by first-boot
  # customization tooling). See the note in Autounattend.xml for why it can't
  # simply be changed here without a corresponding template/customization step.
  winrm_password = "Password123!"
  winrm_timeout  = "3h"

  floppy_files = [
    "Autounattend.xml",
    "scripts/enable-winrm.ps1",
    "scripts/install-virtio.ps1"
  ]

  qemuargs = [
    ["-drive", "file=virtio-win.iso,media=cdrom,index=3"]
  ]

  shutdown_command = "shutdown /s /t 10 /f"
}

build {
  sources = ["source.qemu.windows"]

  provisioner "powershell" {
    scripts = [
      "scripts/install-virtio.ps1"
    ]
  }

  # Schedules (but does not itself apply) a one-time hardening pass for the
  # next boot, so the insecure WinRM config and AutoLogon enabled purely for
  # this build do not ship live in every VM cloned from the resulting image.
  # See scripts/harden-on-first-boot.ps1 for details and rationale.
  provisioner "powershell" {
    scripts = [
      "scripts/harden-on-first-boot.ps1"
    ]
  }
}
