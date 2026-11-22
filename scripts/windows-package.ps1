# Packs FIBER Setup for Windows: the installer and the portable zip, from an app that is already
# built. In steps, because a release signs files in between (docs/release.md):
#
#   stage        the app as it will be installed: fiber-setup.exe and the usbboot folder; with
#                -Cli, the command line's fiber-setup-cli.exe beside it, in cli
#   webview2     Microsoft's WebView2 installer, which the app's installer carries along
#   uninstaller  uninstall.exe (a release signs it, with the app and the driver's catalog)
#   installer    the installer, which carries the staged app and that uninstall.exe
#   zip          the portable zip of the staged app
#   cli          the command line's zip: fiber-setup-cli.exe and the staged app's usbboot folder
#
# -Work is the folder they share. -Label is the version in the file names (a tag's version in a
# release; anything else in a test).
param(
  [Parameter(Mandatory)][ValidateSet('stage', 'webview2', 'uninstaller', 'installer', 'zip', 'cli')][string]$Step,
  [string]$Exe,
  [string]$Cli,
  [string]$Label,
  [string]$Work = 'target\windows-package'
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
New-Item -ItemType Directory -Force $Work | Out-Null
$Work = (Resolve-Path $Work).Path
$app = Join-Path $Work 'app'
$out = Join-Path $Work 'out'
$version = (Get-Content (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
if (-not $Label) { $Label = $version }

# NSIS makes the installer. GitHub's Windows machines do not have it, so there it is installed
# with Chocolatey, which they do have; on a developer's PC it is asked for.
function Find-MakeNsis {
  $usual = Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe'
  $found = Get-Command makensis.exe -ErrorAction SilentlyContinue
  if ($found) { return $found.Source }
  if (Test-Path $usual) { return $usual }
  if ($env:GITHUB_ACTIONS -eq 'true') {
    choco install nsis --yes --no-progress | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "choco install nsis failed ($LASTEXITCODE)" }
    if (Test-Path $usual) { return $usual }
  }
  throw 'makensis.exe not found: install NSIS 3 (https://nsis.sourceforge.io, or: choco install nsis)'
}

function Invoke-MakeNsis([string[]]$Defines) {
  $makensis = Find-MakeNsis
  "NSIS $(& $makensis /VERSION) ($makensis)"
  # Windows wants four numbers for a file's version: 1.2.3.0, also for 1.2.3-rc.1.
  $numeric = ($version -split '[-+]')[0] + '.0'
  $arguments = @('/V2', '/INPUTCHARSET', 'UTF8', "/DVERSION=$version", "/DVERSION_NUMERIC=$numeric") + $Defines + (Join-Path $root 'packaging\windows\installer.nsi')
  & $makensis @arguments
  if ($LASTEXITCODE -ne 0) { throw "makensis failed ($LASTEXITCODE)" }
}

switch ($Step) {
  'stage' {
    if (-not $Exe) { throw '-Exe: the built fiber-setup.exe' }
    $usbboot = Join-Path $root 'src-tauri\resources\usbboot'
    foreach ($needed in 'mass-storage-gadget64\boot.img', 'mass-storage-gadget64\bootfiles.bin', 'driver\fiber-usbboot.inf', 'driver\fiber-usbboot.cat') {
      if (-not (Test-Path (Join-Path $usbboot $needed))) { throw "$needed is missing from $usbboot" }
    }
    Remove-Item $app -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $app | Out-Null
    Copy-Item $Exe (Join-Path $app 'fiber-setup.exe')
    Copy-Item $usbboot (Join-Path $app 'usbboot') -Recurse
    Get-ChildItem $app -Recurse -File | ForEach-Object { $_.FullName.Substring($app.Length + 1) }
    if ($Cli) {
      $cliDir = Join-Path $Work 'cli'
      Remove-Item $cliDir -Recurse -Force -ErrorAction SilentlyContinue
      New-Item -ItemType Directory -Force $cliDir | Out-Null
      Copy-Item $Cli (Join-Path $cliDir 'fiber-setup-cli.exe')
      'cli\fiber-setup-cli.exe'
    }
  }
  'webview2' {
    # The small one, which downloads the rest: https://developer.microsoft.com/microsoft-edge/webview2
    $setup = Join-Path $Work 'MicrosoftEdgeWebview2Setup.exe'
    Invoke-WebRequest 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $setup
    $signature = Get-AuthenticodeSignature $setup
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike '*O=Microsoft Corporation*') {
      throw "MicrosoftEdgeWebview2Setup.exe is not signed by Microsoft: $($signature.Status), $($signature.SignerCertificate.Subject)"
    }
    "MicrosoftEdgeWebview2Setup.exe: $((Get-Item $setup).Length) bytes, signed by $($signature.SignerCertificate.Subject)"
  }
  'uninstaller' {
    $maker = Join-Path $Work 'make-uninstaller.exe'
    $uninstaller = Join-Path $Work 'uninstall.exe'
    Remove-Item $maker, $uninstaller -ErrorAction SilentlyContinue
    Invoke-MakeNsis @('/DUNINSTALLER_ONLY', "/DAPP_DIR=$app", "/DOUT_FILE=$maker")
    # It installs nothing: it writes uninstall.exe next to itself and quits.
    Start-Process $maker -Wait
    if (-not (Test-Path $uninstaller)) { throw 'make-uninstaller.exe wrote no uninstall.exe' }
    Remove-Item $maker
    "uninstall.exe: $((Get-Item $uninstaller).Length) bytes"
  }
  'installer' {
    $uninstaller = Join-Path $Work 'uninstall.exe'
    if (-not (Test-Path $uninstaller)) { throw 'uninstall.exe is missing: run the uninstaller step first' }
    New-Item -ItemType Directory -Force $out | Out-Null
    $setup = Join-Path $out "fiber-setup-$Label-windows-x64-setup.exe"
    $defines = @("/DAPP_DIR=$app", "/DOUT_FILE=$setup", "/DUNINSTALLER=$uninstaller")
    $webview2 = Join-Path $Work 'MicrosoftEdgeWebview2Setup.exe'
    if (Test-Path $webview2) { $defines += "/DWEBVIEW2=$webview2" } else { 'Without WebView2: MicrosoftEdgeWebview2Setup.exe is not in the work folder.' }
    Invoke-MakeNsis $defines
    "$(Split-Path $setup -Leaf): $((Get-Item $setup).Length) bytes"
  }
  'zip' {
    New-Item -ItemType Directory -Force $out | Out-Null
    $zip = Join-Path $out "fiber-setup-$Label-windows-x64-portable.zip"
    Remove-Item $zip -ErrorAction SilentlyContinue
    # One folder in the zip, so that unpacking it anywhere keeps the app and usbboot together.
    $top = Join-Path $Work 'zip\FIBER Setup'
    Remove-Item (Join-Path $Work 'zip') -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force (Split-Path $top) | Out-Null
    Copy-Item $app $top -Recurse
    Compress-Archive -Path $top -DestinationPath $zip
    Remove-Item (Join-Path $Work 'zip') -Recurse -Force
    "$(Split-Path $zip -Leaf): $((Get-Item $zip).Length) bytes"
  }
  'cli' {
    $exe = Join-Path $Work 'cli\fiber-setup-cli.exe'
    if (-not (Test-Path $exe)) { throw 'cli\fiber-setup-cli.exe is missing: stage it with -Cli first' }
    New-Item -ItemType Directory -Force $out | Out-Null
    $zip = Join-Path $out "fiber-setup-cli-$Label-windows-x64.zip"
    Remove-Item $zip -ErrorAction SilentlyContinue
    # One folder in the zip: the program finds the gadget's files and the USB driver in the
    # usbboot folder beside it (the staged app's, whose driver catalog a release has signed).
    $top = Join-Path $Work "zip\fiber-setup-cli-$Label"
    Remove-Item (Join-Path $Work 'zip') -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $top | Out-Null
    Copy-Item $exe $top
    Copy-Item (Join-Path $app 'usbboot') (Join-Path $top 'usbboot') -Recurse
    Copy-Item (Join-Path $root 'LICENSE') $top
    Copy-Item (Join-Path $root 'LICENSES') (Join-Path $top 'LICENSES') -Recurse
    Compress-Archive -Path $top -DestinationPath $zip
    Remove-Item (Join-Path $Work 'zip') -Recurse -Force
    "$(Split-Path $zip -Leaf): $((Get-Item $zip).Length) bytes"
  }
}
