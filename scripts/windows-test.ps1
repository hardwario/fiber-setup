# What only a Windows machine can check, run by CI as administrator (as the app itself runs):
# rpiboot starts with Cygwin's DLLs, Windows takes the USB driver, the installer installs and
# the uninstaller removes. No FIBER is needed, and none is connected.
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $root
$usbboot = Join-Path $root 'src-tauri\resources\usbboot'
$work = Join-Path $root 'target\windows-test'
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $work | Out-Null
function Step([string]$name) { Write-Host "`n== $name" -ForegroundColor Cyan }
function Wait-Exit([string]$file, [string[]]$arguments) {
  $p = if ($arguments) { Start-Process $file -ArgumentList $arguments -Wait -PassThru } else { Start-Process $file -Wait -PassThru }
  return $p.ExitCode
}

Step 'rpiboot.exe starts'
$usage = & (Join-Path $usbboot 'rpiboot.exe') -h 2>&1 | Out-String
if ($usage -notmatch 'Usage: rpiboot') { throw "rpiboot.exe -h said: $usage" }
($usage -split "`n")[0]

Step 'Windows takes the driver package'
# A release signs the catalog as HARDWARIO a.s. Here a certificate made for this run, which
# this machine is told to trust, stands in: the question is whether Windows accepts the
# package (the .inf and a catalog made by scripts/driver-catalog.ps1) once its signer is one
# it trusts.
$bundle = Join-Path $work 'usbboot'
Copy-Item $usbboot $bundle -Recurse
$cat = Join-Path $bundle 'driver\fiber-usbboot.cat'
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=FIBER Setup CI test' -CertStoreLocation Cert:\LocalMachine\My -NotAfter (Get-Date).AddDays(2)
foreach ($name in 'Root', 'TrustedPublisher') {
  $store = [System.Security.Cryptography.X509Certificates.X509Store]::new($name, 'LocalMachine')
  $store.Open('ReadWrite'); $store.Add($cert); $store.Close()
}
$signed = Set-AuthenticodeSignature -FilePath $cat -Certificate $cert -HashAlgorithm SHA256
if ($signed.Status -ne 'Valid') { throw "signing the test catalog: $($signed.Status) $($signed.StatusMessage)" }

cargo build --locked -p fiber-setup -p fiber-setup-cli
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
$cli = Join-Path $root 'target\debug\fiber-setup-cli.exe'
$env:FIBER_SETUP_USBBOOT = $bundle
'Before:'; & $cli access
# The app's own way: pnputil, through fiber-setup-core.
& $cli access --install
if ($LASTEXITCODE -ne 0) { throw 'fiber-setup-cli access --install failed' }
$listed = pnputil /enum-drivers | Out-String
if ($listed -notmatch 'fiber-usbboot\.inf') { throw 'the driver is not in the driver store after installing it' }
$status = & $cli access | ConvertFrom-Json
if (-not $status.rules_installed) { throw 'the app does not see the driver it installed' }
'The driver is in the driver store, and the app sees it.'
Remove-Item Env:FIBER_SETUP_USBBOOT

Step 'The devices and disks the app sees'
& $cli devices

Step 'The installer and the zip are made'
$package = Join-Path $work 'package'
$pack = Join-Path $root 'scripts\windows-package.ps1'
& $pack -Step stage -Exe (Join-Path $root 'target\debug\fiber-setup.exe') -Label test -Work $package
& $pack -Step webview2 -Label test -Work $package
& $pack -Step uninstaller -Label test -Work $package
& $pack -Step installer -Label test -Work $package
& $pack -Step zip -Label test -Work $package

Step 'The installer installs'
$setup = Join-Path $package 'out\fiber-setup-test-windows-x64-setup.exe'
$code = Wait-Exit $setup @('/S')
if ($code -ne 0) { throw "the installer ended with $code" }
$installed = Join-Path $env:ProgramFiles 'FIBER Setup'
foreach ($file in 'fiber-setup.exe', 'uninstall.exe', 'usbboot\rpiboot.exe', 'usbboot\cygwin1.dll', 'usbboot\mass-storage-gadget64\boot.img', 'usbboot\driver\fiber-usbboot.inf') {
  if (-not (Test-Path (Join-Path $installed $file))) { throw "$file was not installed" }
}
$key = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\FIBER Setup'
$entry = Get-ItemProperty $key
"Installed: $($entry.DisplayName) $($entry.DisplayVersion) by $($entry.Publisher), $($entry.EstimatedSize) KB"
$shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\FIBER Setup.lnk'
if (-not (Test-Path $shortcut)) { throw 'no Start menu entry' }
# For the computer or for this user: either is installed.
$webview = 'HKLM:\SOFTWARE\WOW6432Node', 'HKCU:\Software' | ForEach-Object {
  (Get-ItemProperty "$_\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" -ErrorAction SilentlyContinue).pv
} | Where-Object { $_ -and $_ -ne '0.0.0.0' } | Select-Object -First 1
"WebView2: $webview"
if (-not $webview) { throw 'WebView2 is not installed after the installer ran' }

Step 'The uninstaller removes it'
# `_?=` makes it work in place and wait, instead of copying itself away and returning at once.
$code = Wait-Exit (Join-Path $installed 'uninstall.exe') @('/S', "_?=$installed")
if ($code -ne 0) { throw "the uninstaller ended with $code" }
foreach ($gone in (Join-Path $installed 'fiber-setup.exe'), (Join-Path $installed 'usbboot'), $shortcut, $key) {
  if (Test-Path $gone) { throw "$gone is still there after uninstalling" }
}
'Removed.'

Step 'The zip holds the app'
$unzipped = Join-Path $work 'unzipped'
Expand-Archive (Join-Path $package 'out\fiber-setup-test-windows-x64-portable.zip') $unzipped
foreach ($file in 'FIBER Setup\fiber-setup.exe', 'FIBER Setup\usbboot\rpiboot.exe', 'FIBER Setup\usbboot\driver\fiber-usbboot.cat') {
  if (-not (Test-Path (Join-Path $unzipped $file))) { throw "$file is not in the zip" }
}
'All there.'
