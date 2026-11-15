# Makes the catalog (fiber-usbboot.cat) of the USB driver in src-tauri/resources/usbboot/driver:
# the list of what the driver package holds, with a checksum of each file. A release signs it;
# Windows installs the driver only when the signed list matches the files.
$ErrorActionPreference = 'Stop'
$driver = (Resolve-Path (Join-Path $PSScriptRoot '..\src-tauri\resources\usbboot\driver')).Path
$cat = Join-Path $driver 'fiber-usbboot.cat'
Remove-Item $cat -ErrorAction SilentlyContinue
New-FileCatalog -Path $driver -CatalogFilePath $cat -CatalogVersion 2.0 | Out-Null
$check = Test-FileCatalog -Path $driver -CatalogFilePath $cat -Detailed
if ($check.Status -ne 'Valid') { throw "the catalog does not match the driver's files: $($check.Status)" }
"fiber-usbboot.cat lists: $($check.CatalogItems.Keys -join ', ')"
Get-ChildItem $driver | Format-Table Name, Length
