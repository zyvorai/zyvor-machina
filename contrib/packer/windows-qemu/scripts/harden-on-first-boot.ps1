# Runs once, DURING the Packer build (over the still-open build-time WinRM
# connection), and does not itself touch the live WinRM/AutoLogon config —
# it only stages a one-time hardening pass and schedules it to run at the
# NEXT boot of this machine, i.e. the first real boot of a VM cloned from
# this golden image.
#
# Why: scripts/enable-winrm.ps1 intentionally configures WinRM insecurely
# (AllowUnencrypted, Basic auth, an open firewall rule) and Autounattend.xml
# intentionally enables AutoLogon, both ONLY so Packer can provision this
# image unattended over WinRM during the build. Neither of those is safe to
# leave enabled in the shipped image: every VM cloned from it would expose
# unauthenticated-looking, unencrypted WinRM and/or auto-login as
# Administrator. Reverting them mid-build would risk breaking the very
# WinRM connection Packer uses to run this and the final shutdown_command,
# so the revert is deferred to first real boot instead.
$ErrorActionPreference = "Continue"

$destDir = Join-Path $env:ProgramData "Machina"
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
$hardenScript = Join-Path $destDir "harden-on-first-boot.ps1"

@'
$ErrorActionPreference = "Continue"

# Revert WinRM to secure defaults (no unencrypted transport, no Basic auth).
winrm set winrm/config/service '@{AllowUnencrypted="false"}' | Out-Null
winrm set winrm/config/service/auth '@{Basic="false"}' | Out-Null

# The WinRM firewall rule was opened for build-time provisioning only.
Set-NetFirewallRule -Name "WINRM-HTTP-In-TCP" -Enabled False -ErrorAction SilentlyContinue

# Clear AutoLogon remnants. Windows normally resets AutoAdminLogon to 0 once
# LogonCount is exhausted, but it does NOT reliably clear DefaultPassword,
# which otherwise sits in the registry in plaintext indefinitely.
$winlogon = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon"
Set-ItemProperty -Path $winlogon -Name AutoAdminLogon -Value "0" -ErrorAction SilentlyContinue
Remove-ItemProperty -Path $winlogon -Name DefaultPassword -ErrorAction SilentlyContinue
Remove-ItemProperty -Path $winlogon -Name DefaultUserName -ErrorAction SilentlyContinue

# One-time pass: unregister the scheduled task and remove this copy of the
# script so nothing lingers on disk referencing the above.
Unregister-ScheduledTask -TaskName "MachinaHardenOnFirstBoot" -Confirm:$false -ErrorAction SilentlyContinue
Remove-Item -Path $PSCommandPath -Force -ErrorAction SilentlyContinue
'@ | Set-Content -Path $hardenScript -Encoding UTF8

$action  = New-ScheduledTaskAction -Execute "powershell.exe" `
    -Argument "-ExecutionPolicy Bypass -NoProfile -File `"$hardenScript`""
$trigger = New-ScheduledTaskTrigger -AtStartup
Register-ScheduledTask -TaskName "MachinaHardenOnFirstBoot" -Action $action -Trigger $trigger `
    -RunLevel Highest -User "SYSTEM" -Force | Out-Null

Write-Host "Scheduled first-real-boot hardening pass ($hardenScript)."
