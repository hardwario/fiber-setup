# Makes the catalog (fiber-usbboot.cat) of the USB driver in src-tauri/resources/usbboot/driver:
# the list of what the driver package holds, which a release signs. Windows installs the driver
# only with it. Inf2Cat is part of the Windows Driver Kit; it also checks the .inf.
$ErrorActionPreference = 'Stop'
$driver = Join-Path $PSScriptRoot '..\src-tauri\resources\usbboot\driver' | Resolve-Path
$inf2cat = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x86\Inf2Cat.exe" -ErrorAction SilentlyContinue |
  Sort-Object FullName | Select-Object -Last 1
if (-not $inf2cat) { throw 'Inf2Cat.exe not found: install the Windows Driver Kit' }
"Using $($inf2cat.FullName)"
Remove-Item (Join-Path $driver '*.cat') -ErrorAction SilentlyContinue
# Windows 10 and 11, on Intel/AMD and on ARM (the .inf has both).
& $inf2cat.FullName "/driver:$driver" /os:10_X64,10_RS3_ARM64 /verbose
if ($LASTEXITCODE -ne 0) { throw "Inf2Cat failed ($LASTEXITCODE)" }
$cat = Join-Path $driver 'fiber-usbboot.cat'
if (-not (Test-Path $cat)) { throw 'Inf2Cat made no fiber-usbboot.cat' }
Get-ChildItem $driver | Format-Table Name, Length
