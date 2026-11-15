# The checks of a release's Windows signing job (docs/release.md), in the work folder of
# scripts/windows-package.ps1:
#
#   -Files a, b, ..  each is signed by HARDWARIO a.s., with a timestamp; the files of others
#                    in the package are as they came (Cygwin's unsigned, Microsoft's signed)
#   -Driver          Windows takes the signed driver package into its driver store
#   -Installer       the installer in out\ is signed the same way
param(
  [Parameter(Mandatory)][string]$Work,
  [string[]]$Files,
  [switch]$Driver,
  [switch]$Installer
)
$ErrorActionPreference = 'Stop'
$Work = (Resolve-Path $Work).Path

function Assert-Ours([string]$file) {
  $sig = Get-AuthenticodeSignature $file
  "{0}: {1}, signed by {2}, timestamped by {3}" -f (Split-Path $file -Leaf), $sig.Status, $sig.SignerCertificate.Subject, $sig.TimeStamperCertificate.Subject
  if ($sig.Status -ne 'Valid') { throw "$file: signature $($sig.Status): $($sig.StatusMessage)" }
  if ($sig.SignerCertificate.Subject -notlike '*HARDWARIO a.s.*') { throw "$file: signed by $($sig.SignerCertificate.Subject)" }
  if (-not $sig.TimeStamperCertificate) { throw "$file: no timestamp" }
}

if ($Files) {
  foreach ($file in $Files) { Assert-Ours (Join-Path $Work $file) }
  # Everything else that Windows can run must be one of the files of others we know.
  $ours = $Files | ForEach-Object { (Join-Path $Work $_).ToLowerInvariant() }
  $others = Get-ChildItem $Work -Recurse -File -Include *.exe, *.dll, *.cat, *.sys |
    Where-Object { $ours -notcontains $_.FullName.ToLowerInvariant() }
  foreach ($file in $others) {
    $sig = Get-AuthenticodeSignature $file.FullName
    "{0}: {1} {2}" -f $file.Name, $sig.Status, $sig.SignerCertificate.Subject
    switch -Wildcard ($file.Name) {
      'cyg*.dll' { if ($sig.Status -ne 'NotSigned') { throw "$($file.Name) is Cygwin's and should be unsigned, as it came" } }
      'MicrosoftEdgeWebview2Setup.exe' {
        if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notlike '*O=Microsoft Corporation*') { throw "$($file.Name) is not Microsoft's" }
      }
      default { throw "unexpected file in the package: $($file.FullName)" }
    }
  }
}

if ($Driver) {
  $inf = Join-Path $Work 'app\usbboot\driver\fiber-usbboot.inf'
  $cat = Join-Path $Work 'app\usbboot\driver\fiber-usbboot.cat'
  $check = Test-FileCatalog -Path (Split-Path $inf) -CatalogFilePath $cat -Detailed
  if ($check.Status -ne 'Valid') { throw "the signed catalog does not match the driver's files: $($check.Status)" }
  $signer = (Get-AuthenticodeSignature $cat).SignerCertificate
  $store = [System.Security.Cryptography.X509Certificates.X509Store]::new('TrustedPublisher', 'LocalMachine')
  $store.Open('ReadWrite'); $store.Add($signer); $store.Close()
  $said = pnputil /add-driver $inf | Out-String
  $said
  if ($LASTEXITCODE -ne 0) { throw "Windows does not take the driver package (pnputil $LASTEXITCODE)" }
  if ((pnputil /enum-drivers | Out-String) -notmatch 'fiber-usbboot\.inf') { throw 'the driver is not in the driver store' }
  'Windows takes the driver package signed by HARDWARIO a.s.'
}

if ($Installer) {
  $setup = @(Get-ChildItem (Join-Path $Work 'out') -Filter *-setup.exe)
  if ($setup.Count -ne 1) { throw "expected one installer in out\, found $($setup.Count)" }
  Assert-Ours $setup[0].FullName
  $zip = @(Get-ChildItem (Join-Path $Work 'out') -Filter *-portable.zip)
  if ($zip.Count -ne 1) { throw "expected one zip in out\, found $($zip.Count)" }
}
