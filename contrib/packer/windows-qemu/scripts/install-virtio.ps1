$ErrorActionPreference = "Continue"

$drives = Get-PSDrive -PSProvider FileSystem | Select-Object -ExpandProperty Root

foreach ($drive in $drives) {
    $viostor = Join-Path $drive "viostor"
    $netkvm  = Join-Path $drive "NetKVM"

    if (Test-Path $viostor) {
        Write-Host "Installing VirtIO storage drivers from $drive"
        pnputil.exe /add-driver "$drive\viostor\*\*\*.inf" /subdirs /install
        pnputil.exe /add-driver "$drive\vioscsi\*\*\*.inf" /subdirs /install
    }

    if (Test-Path $netkvm) {
        Write-Host "Installing VirtIO network drivers from $drive"
        pnputil.exe /add-driver "$drive\NetKVM\*\*\*.inf" /subdirs /install
    }
}
