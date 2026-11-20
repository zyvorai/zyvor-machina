#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Build qcow2 Linux guest images with HashiCorp Packer (QEMU builder).
# QEMU builder options follow: https://developer.hashicorp.com/packer/integrations/hashicorp/qemu/latest/components/builder/qemu
# Windows + VirtIO (manual ISO): contrib/packer/windows-qemu/
# Windows 10/11 golden (dockur/Podman): contrib/packer/build-windows-dockur.sh
# Usage: ./build-linux-image.sh {fedora43|ubuntu2204|ubuntu2404|ubuntu2504|ubuntu2510|ubuntu2604|debian12|debian13|almalinux9|rocky9|centos9stream|oraclelinux9} [workdir]
set -euo pipefail

GUEST="${1:-fedora43}"
WORKDIR="${2:-packer-${GUEST}}"

# HashiCorp Packer binary. On RHEL/Alma, `packer` in PATH often resolves to /usr/sbin/packer -> cracklib-packer
# (dictionary), while the real tool is /usr/bin/packer (RPM) or /usr/local/bin/packer (this script / manual).
hashicorp_packer_bin() {
  if [ -x /usr/local/bin/packer ]; then
    echo /usr/local/bin/packer
    return 0
  fi
  if [ -x /usr/bin/packer ]; then
    echo /usr/bin/packer
    return 0
  fi
  local p cand
  p=$(command -v packer 2>/dev/null || true)
  if [ -n "$p" ] && [ -x "$p" ]; then
    cand=$(readlink "$p" 2>/dev/null || true)
    case "$cand" in
      *cracklib-packer* | */cracklib/*) ;;
      *) echo "$p"; return 0 ;;
    esac
  fi
  return 1
}

install_deps() {
  # shellcheck source=/dev/null
  . /etc/os-release

  case "$ID" in
    almalinux|rocky|rhel|centos|fedora)
      sudo dnf install -y qemu-kvm qemu-img libvirt virt-install edk2-ovmf wget curl unzip
      sudo systemctl enable --now libvirtd || true
      ;;
    ubuntu|debian)
      sudo apt-get update
      sudo apt-get install -y qemu-system-x86 qemu-utils libvirt-daemon-system libvirt-clients virtinst wget curl unzip python3
      ;;
    *)
      echo "Unsupported host distro: $ID"
      exit 1
      ;;
  esac

  if ! hashicorp_packer_bin >/dev/null; then
    echo "Installing HashiCorp Packer to /usr/local/bin (RHEL-like hosts ship cracklib-packer as /usr/sbin/packer)..."
    PACKER_VER="1.11.2"
    PT_ARCH=$(uname -m)
    case "$PT_ARCH" in
      x86_64) H_ARCH=amd64 ;;
      aarch64|arm64) H_ARCH=arm64 ;;
      *) echo "Unknown uname -m=$PT_ARCH; trying amd64 zip" >&2; H_ARCH=amd64 ;;
    esac
    curl -fsSL -o /tmp/packer.zip "https://releases.hashicorp.com/packer/${PACKER_VER}/packer_${PACKER_VER}_linux_${H_ARCH}.zip"
    unzip -o /tmp/packer.zip -d /tmp
    sudo mv /tmp/packer /usr/local/bin/packer
    sudo chmod 755 /usr/local/bin/packer
    rm -f /tmp/packer.zip
  fi
}

qemu_binary() {
  if [ -x /usr/libexec/qemu-kvm ]; then
    echo /usr/libexec/qemu-kvm
  elif command -v qemu-system-x86_64 >/dev/null 2>&1; then
    command -v qemu-system-x86_64
  else
    echo "qemu-system-x86_64 not found" >&2
    exit 1
  fi
}

make_files() {
  mkdir -p "$WORKDIR/http"
  cd "$WORKDIR"

  QEMU_BIN="$(qemu_binary)"
  SSH_USER="root"
  # Root guests halt directly; Ubuntu/Debian minimal use `packer` + sudo shutdown.
  SHUTDOWN_CMD='shutdown -P now'
  DISK_SIZE="8192"
  MEMORY=2048
  CPUS=2
  SSH_TIMEOUT="45m"
  PROVISIONER_BLOCK=""
  BOOT_WAIT="5s"

  case "$GUEST" in
    fedora43)
      # Fedora netinst uses GRUB; edit kernel line (e … Ctrl+X) so kickstart is applied; SSH as packer.
      SSH_USER="packer"
      SSH_TIMEOUT="90m"
      BOOT_WAIT="10s"
      SHUTDOWN_CMD='echo password | sudo -S shutdown -P now'
      ISO_URL="https://download.fedoraproject.org/pub/fedora/linux/releases/43/Server/x86_64/iso/Fedora-Server-netinst-x86_64-43-1.6.iso"
      KS_URL="https://download.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os/"
      BOOT_COMMAND="$(cat <<'BOOT'
["e<wait>", "<down><down><end>", " inst.text inst.ks=http://{{ .HTTPIP }}:{{ .HTTPPort }}/ks.cfg", "<leftCtrlOn>x<leftCtrlOff>"]
BOOT
)"
      cat > http/ks.cfg <<KS
lang en_US.UTF-8
keyboard us
timezone UTC --utc

text
reboot

rootpw --plaintext password
user --name=packer --password=password --plaintext --groups=wheel

url --url="${KS_URL}"

bootloader --location=mbr
clearpart --all --initlabel
autopart --type=plain

firewall --enabled --service=ssh
selinux --enforcing

%packages
@core
openssh-server
sudo
%end

%post
systemctl enable sshd
echo 'packer ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/packer
chmod 440 /etc/sudoers.d/packer
%end
KS
      ;;

    almalinux9)
      ISO_URL="https://repo.almalinux.org/almalinux/9/isos/x86_64/AlmaLinux-9-latest-x86_64-boot.iso"
      KS_URL="https://repo.almalinux.org/almalinux/9/BaseOS/x86_64/os/"
      BOOT_COMMAND='["<tab> inst.text inst.ks=http://{{ .HTTPIP }}:{{ .HTTPPort }}/ks.cfg<enter>"]'
      cat > http/ks.cfg <<KS
lang en_US.UTF-8
keyboard us
timezone UTC --utc
text
rootpw password
reboot
url --url="${KS_URL}"
bootloader --location=mbr
clearpart --all --initlabel
autopart --type=plain
%packages
@core
openssh-server
%end
%post
systemctl enable sshd
%end
KS
      ;;

    rocky9)
      ISO_URL="https://download.rockylinux.org/pub/rocky/9/isos/x86_64/Rocky-9-latest-x86_64-boot.iso"
      KS_URL="https://download.rockylinux.org/pub/rocky/9/BaseOS/x86_64/os/"
      BOOT_COMMAND='["<tab> inst.text inst.ks=http://{{ .HTTPIP }}:{{ .HTTPPort }}/ks.cfg<enter>"]'
      cat > http/ks.cfg <<KS
lang en_US.UTF-8
keyboard us
timezone UTC --utc
text
rootpw password
reboot
url --url="${KS_URL}"
bootloader --location=mbr
clearpart --all --initlabel
autopart --type=plain
%packages
@core
openssh-server
%end
%post
systemctl enable sshd
%end
KS
      ;;

    centos9stream)
      ISO_URL="https://mirror.stream.centos.org/9-stream/BaseOS/x86_64/iso/CentOS-Stream-9-latest-x86_64-boot.iso"
      KS_URL="https://mirror.stream.centos.org/9-stream/BaseOS/x86_64/os/"
      BOOT_COMMAND='["<tab> inst.text inst.ks=http://{{ .HTTPIP }}:{{ .HTTPPort }}/ks.cfg<enter>"]'
      cat > http/ks.cfg <<KS
lang en_US.UTF-8
keyboard us
timezone UTC --utc
text
rootpw password
reboot
url --url="${KS_URL}"
bootloader --location=mbr
clearpart --all --initlabel
autopart --type=plain
%packages
@core
openssh-server
%end
%post
systemctl enable sshd
%end
KS
      ;;

    oraclelinux9)
      ISO_URL="https://yum.oracle.com/ISOS/OracleLinux/OL9/u7/x86_64/OracleLinux-R9-U7-x86_64-boot.iso"
      KS_URL="https://yum.oracle.com/repo/OracleLinux/OL9/baseos/latest/x86_64/"
      BOOT_COMMAND='["<tab> inst.text inst.ks=http://{{ .HTTPIP }}:{{ .HTTPPort }}/ks.cfg<enter>"]'
      cat > http/ks.cfg <<KS
lang en_US.UTF-8
keyboard us
timezone UTC --utc
text
rootpw password
reboot
url --url="${KS_URL}"
bootloader --location=mbr
clearpart --all --initlabel
autopart --type=plain
%packages
@core
openssh-server
%end
%post
systemctl enable sshd
%end
KS
      ;;

    ubuntu2204|ubuntu2404|ubuntu2504|ubuntu2510|ubuntu2604)
      SSH_USER="packer"
      SSH_TIMEOUT="60m"
      SHUTDOWN_CMD="echo 'password' | sudo -S shutdown -P now"
      case "$GUEST" in
        ubuntu2204) ISO_URL="https://releases.ubuntu.com/22.04/ubuntu-22.04.5-live-server-amd64.iso" ;;
        ubuntu2404) ISO_URL="https://releases.ubuntu.com/24.04/ubuntu-24.04.4-live-server-amd64.iso" ;;
        ubuntu2504) ISO_URL="https://releases.ubuntu.com/25.04/ubuntu-25.04-live-server-amd64.iso" ;;
        ubuntu2510) ISO_URL="https://releases.ubuntu.com/25.10/ubuntu-25.10-live-server-amd64.iso" ;;
        ubuntu2604) ISO_URL="https://releases.ubuntu.com/26.04/ubuntu-26.04-live-server-amd64.iso" ;;
      esac
      BOOT_COMMAND='["e<wait>","<down><down><down><end> autoinstall ds=nocloud-net\\;s=http://{{ .HTTPIP }}:{{ .HTTPPort }}/ ---<f10>"]'
      cat > http/user-data <<'UD'
#cloud-config
autoinstall:
  version: 1
  identity:
    hostname: ubuntu
    username: packer
    password: "$6$rounds=4096$packer$Jz5VkgXyEKhtWJE9WqfRbxUXs7VbDl3CtQ79loB18bgWyHN6s2HDM41cOTfUEqhYs2EVtebQjWKuM.BBT4ESv."
  ssh:
    install-server: true
    allow-pw: true
  packages:
    - openssh-server
  storage:
    layout:
      name: direct
  late-commands:
    - curtin in-target --target=/target -- systemctl enable ssh
UD
      touch http/meta-data
      ;;

    debian12|debian13)
      DISK_SIZE="2048"
      MEMORY=1024
      CPUS=1
      SSH_USER="packer"
      SSH_TIMEOUT="60m"
      SHUTDOWN_CMD='echo password | sudo -S shutdown -P now'
      case "$GUEST" in
        debian12) ISO_URL="https://cdimage.debian.org/debian-cd/current/amd64/iso-cd/debian-12.11.0-amd64-netinst.iso" ;;
        debian13) ISO_URL="https://cdimage.debian.org/debian-cd/current/amd64/iso-cd/debian-13.4.0-amd64-netinst.iso" ;;
      esac
      BOOT_COMMAND="$(cat <<'BOOT'
[
    "<esc><wait>",
    "auto preseed/url=http://{{ .HTTPIP }}:{{ .HTTPPort }}/preseed.cfg ",
    "debian-installer=en_US locale=en_US ",
    "keyboard-configuration/xkb-keymap=us ",
    "netcfg/get_hostname=debian ",
    "fb=false debconf/frontend=noninteractive ",
    "<enter>"
]
BOOT
)"
      cat > http/preseed.cfg <<'PRE'
d-i debian-installer/locale string en_US
d-i keyboard-configuration/xkb-keymap select us
d-i netcfg/get_hostname string debian
d-i netcfg/get_domain string local

d-i passwd/root-login boolean false
d-i passwd/user-fullname string packer
d-i passwd/username string packer
d-i passwd/user-password password password
d-i passwd/user-password-again password password

d-i clock-setup/utc boolean true
d-i time/zone string UTC

d-i partman-auto/method string regular
d-i partman-auto/choose_recipe select atomic
d-i partman/confirm_write_new_label boolean true
d-i partman/choose_partition select finish
d-i partman/confirm boolean true
d-i partman/confirm_nooverwrite boolean true

tasksel tasksel/first multiselect
d-i pkgsel/include string openssh-server sudo ca-certificates
d-i pkgsel/install-recommends boolean false
popularity-contest popularity-contest/participate boolean false

d-i preseed/late_command string \
  in-target usermod -aG sudo packer; \
  echo 'packer ALL=(ALL) NOPASSWD:ALL' > /target/etc/sudoers.d/packer; \
  chmod 440 /target/etc/sudoers.d/packer

d-i grub-installer/only_debian boolean true
d-i finish-install/reboot_in_progress note
PRE
      PROVISIONER_BLOCK="$(cat <<'PROV'

  provisioner "shell" {
    inline = [
      "sudo apt-get clean",
      "sudo rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*",
      "sudo dd if=/dev/zero of=/EMPTY bs=1M || true",
      "sudo rm -f /EMPTY",
      "sync"
    ]
  }
PROV
)"
      ;;

    *)
      echo "Usage: $0 {fedora43|ubuntu2204|ubuntu2404|ubuntu2504|ubuntu2510|ubuntu2604|debian12|debian13|almalinux9|rocky9|centos9stream|oraclelinux9} [workdir]"
      exit 1
      ;;
  esac

  # Minimal QEMU template (Alma-style /usr/libexec/qemu-kvm + headless + KVM); per-guest files only differ in ISO/boot_command/http payloads.
  cat > "${GUEST}.pkr.hcl" <<PKR
packer {
  required_plugins {
    qemu = {
      source  = "github.com/hashicorp/qemu"
      version = ">= 1.1.4"
    }
  }
}

source "qemu" "${GUEST}" {
  qemu_binary = "${QEMU_BIN}"
  headless    = true

  iso_url      = "${ISO_URL}"
  iso_checksum = "none"

  output_directory = "output-${GUEST}"
  vm_name          = "${GUEST}.qcow2"
  format           = "qcow2"

  disk_size = "${DISK_SIZE}"
  memory    = ${MEMORY}
  cpus      = ${CPUS}

  accelerator = "kvm"

  ssh_username = "${SSH_USER}"
  ssh_password = "password"
  ssh_timeout  = "${SSH_TIMEOUT}"

  boot_wait    = "${BOOT_WAIT}"
  boot_command = ${BOOT_COMMAND}

  http_directory = "http"

  shutdown_command = "${SHUTDOWN_CMD}"
}

build {
  sources = ["source.qemu.${GUEST}"]
${PROVISIONER_BLOCK}
}
PKR
}

# When machina-daemon triggers a build, skip package installs (host must already have packer/qemu deps).
if [ "${MACHINA_SKIP_PACKER_INSTALL_DEPS:-}" != "1" ]; then
  install_deps
fi
make_files

PACKER_BIN="$(hashicorp_packer_bin)" || {
  echo "error: HashiCorp Packer not found after install_deps (expected /usr/local/bin/packer or /usr/bin/packer)" >&2
  exit 1
}
echo "Using HashiCorp Packer: ${PACKER_BIN} (avoid RHEL /usr/sbin/packer -> cracklib-packer shadowing PATH)"
export CHECKPOINT_DISABLE="${CHECKPOINT_DISABLE:-1}"
"$PACKER_BIN" init .
"$PACKER_BIN" build "${GUEST}.pkr.hcl"

echo ""
echo "Done. Example output:"
echo "  $(pwd)/output-${GUEST}/${GUEST}.qcow2"
echo ""
if [ "$GUEST" = "debian12" ] || [ "$GUEST" = "debian13" ]; then
  echo "Optional shrink (sparse compress): qemu-img convert -O qcow2 -c output-${GUEST}/${GUEST}.qcow2 ${GUEST}-compressed.qcow2"
  echo ""
fi
echo "Logins: Fedora 43 packer/password — Alma/Rocky/CentOS Stream/Oracle root/password — Debian/Ubuntu packer/password"
echo "Import this disk in machina (Web: Import VM) or attach as existing disk when creating a VM."
