# Tutorial: your first private cloud

From a bare Linux KVM host to a running VM with a browser console. About 20 minutes. Nothing here changes anything you
cannot undo with `--uninstall`.

## You need
One x86-64 Linux host with hardware virtualization on (`egrep -c '(vmx|svm)' /proc/cpuinfo` is not 0), libvirt and QEMU
installed, root access, and the release packages ([INSTALL](../INSTALL.md) lists the three ways in).

## 1. Install
```bash
sudo apt install ./machina_*_amd64.deb ./machina-controller_*_amd64.deb ./machina-agent_*_amd64.deb   # or dnf for .rpm
sudo cat /etc/machina/INITIAL_ADMIN_PASSWORD
```
A single host needs all three packages. Services start on their own.

## 2. Sign in
Open `https://<host>:5092`, accept the self-signed certificate once, sign in as `admin` with that password, and change it.
The dashboard should show your host online with its CPU, memory and storage.

## 3. Create a VM
In the UI choose **VMs > Create**, or from a shell: `machinactl` and the REST API do the same. Give it a name, a CPU and
memory size, and an OS image (a cloud image such as cirros is the quickest). Start it.

## 4. Open its console
Open the VM and use **Console**: the browser console (noVNC, SPICE, serial or SSH) is served by the daemon itself, so there is
nothing to install on your laptop. You should see the guest boot and be able to log in.

## 5. Check the platform
```bash
./machinactl verify          # API smoke test (from the source tree)
```

## You should see
Host online, one VM running, its console in your browser, and the audit log recording what you did.

## Clean up
Delete the VM from its page. `sudo ./install.sh --uninstall` (offline bundle) removes Machina; data stays unless you add `--purge`.

Next: [EC2 in ten minutes](02-ec2-in-ten-minutes.md).
