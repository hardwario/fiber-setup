# Releasing

## Steps

1. Set the new version in `package.json` (`npm version 1.2.3 --no-git-tag-version` also updates
   `package-lock.json`), in `Cargo.toml` (the workspace: the core, the CLI and the app take theirs from it,
   and so does Tauri; `cargo check` then updates `Cargo.lock`) and in `src/lib/api.ts` (the simulated
   FIBER). `node scripts/check-version.mjs v1.2.3` checks that they agree.
2. Commit, then tag and push: `git tag v1.2.3 && git push origin v1.2.3`.
3. The Release workflow (`.github/workflows/release.yml`) tests and builds on Linux, macOS and Windows,
   signs the Windows binaries, signs and notarizes the macOS app, and creates a **draft** release with all
   files and their `.sha256` checksums. The two signing jobs wait for an approval: open the run and choose
   **Review deployments**, **signing**, **Approve and deploy** (GitHub also sends a notification). Approve
   only a run of a tag you pushed yourself.
4. Check the draft on GitHub and publish it. Publishing tells the users: from the version after 1.1.0 on,
   FIBER Setup asks GitHub for the newest published release when it starts and offers a newer one (a banner
   on its Welcome page with Download, which opens the release's page). Drafts and pre-releases are never
   offered: keep a release that must not reach users a draft or mark it as a pre-release, and leave "Set as
   the latest release" checked otherwise.

A dry run builds everything without publishing: Actions, Release, Run workflow. Started on a branch it skips
the signing; started on a release tag it signs too, but still publishes nothing.

## Files

| File | Built by |
|---|---|
| `fiber-setup-<version>-windows-x64-setup.exe`, `fiber-setup-<version>-windows-x64-portable.zip` | `build` (windows, the exe), signed and packed by `sign-windows` |
| `fiber-setup-<version>-macos-universal.dmg` | `build` (macos, the app), signed, notarized and packed by `sign-macos` |
| `fiber-setup-<version>-linux-x86_64.AppImage`, `fiber-setup-<version>-linux-x86_64.deb` | `build` (linux), not signed |

Each has a `<file>.sha256` next to it (`shasum -a 256 -c <file>.sha256`), made after signing and packing.
The release holds these files and nothing else; the `publish` job fails otherwise.

Not released yet: `fiber-setup-cli` (it needs the gadget's files beside it, which only the app bundles).

## macOS signing and notarization

The `sign-macos` job signs with the team's Developer ID Application certificate (HARDWARIO a.s., team
C32QFQ68V9) and notarizes with an App Store Connect API key. The app is one build for Apple silicon and
Intel, from macOS 11 on. Order:

1. `FIBER Setup.app`, with the hardened runtime and a secure timestamp. It is then notarized and the ticket
   stapled to it, so it also opens offline;
2. the dmg built from it (the app and a link to Applications): signed, notarized and stapled.

The app holds one program, its own: it starts a FIBER over USB by itself, and the gadget it sends is data
(Raspberry Pi's files, fetched by `scripts/fetch-usbboot.mjs`). Apple refuses an app with any program in it
that is not signed with a Developer ID, so the job first checks that there is no other and stops if it finds
one.

A verification step fails the job unless Gatekeeper (`spctl`) accepts the app and the dmg, both tickets are
stapled, and the app's program is signed with Developer ID, a timestamp and the hardened runtime and is for
both architectures.

The app needs no entitlements: it is not sandboxed, WebKit runs its JIT in Apple's own processes, and USB
and the tools the app starts (`ioreg`, `diskutil`, `authopen`) need none outside the sandbox.

The build job never sees the certificate: it makes an ad-hoc signed app, and only `sign-macos` imports the
certificate, into a temporary keychain it deletes at the end.

When: for a pushed release tag (`v*`) and for a dry run started on one. Nothing else reaches the job.

What it depends on, all in the GitHub environment **`signing`** (deployment limited to the tags `v*`, with
a required reviewer: the person who releases; self-review allowed, no bypass for admins):

- secrets `APPLE_CERTIFICATE` (the `.p12` with certificate and private key, base64),
  `APPLE_CERTIFICATE_PASSWORD` and `APPLE_API_KEY_P8` (the API key's `.p8` file). They are environment
  secrets only: nothing at repository or organization level, so no other job can read them;
- variables `APPLE_API_KEY` (the key ID) and `APPLE_API_ISSUER` (the issuer ID).

To set the secrets, whoever has the files runs (then deletes the files):

```sh
base64 -i <path>/HARDWARIO-Developer-ID-Application.p12 | gh secret set APPLE_CERTIFICATE --env signing --repo hardwario/fiber-setup
gh secret set APPLE_CERTIFICATE_PASSWORD --env signing --repo hardwario/fiber-setup < <path>/password.txt
gh secret set APPLE_API_KEY_P8 --env signing --repo hardwario/fiber-setup < <path>/AuthKey_<key ID>.p8
```

The Developer ID certificate expires on **2027-11-18**. Renewal: the team's Account Holder uploads a new
certificate signing request in developer.apple.com (only the Account Holder can; the API refuses it), then
the new `.p12` goes into `APPLE_CERTIFICATE` and `APPLE_CERTIFICATE_PASSWORD` of every repository that
signs. The API key also serves other build systems: never revoke it for this repository's sake.

## Windows signing

The `sign-windows` job signs with Authenticode, using Microsoft Azure Artifact Signing (formerly Trusted
Signing), account `hardwario`, certificate profile `hardwario` (Public Trust, "CN=HARDWARIO a.s."), endpoint
`https://weu.codesigning.azure.net/`. Nothing is compiled in this job; it signs, and packs with NSIS. Order:

1. `uninstall.exe` is made (unsigned) from `packaging/windows/installer.nsi`: the installer carries it, and
   cannot be changed once it is made, so it has to exist before the first signing.
2. Signed together: `fiber-setup.exe`, the USB driver's catalog `usbboot\driver\fiber-usbboot.cat`, and
   `uninstall.exe`.
3. The installer is made from the signed files, and the portable zip of the same app.
4. The installer is signed.

Not signed by us, because it is Microsoft's file and goes out as it came: `MicrosoftEdgeWebview2Setup.exe`
inside the installer (signed by Microsoft, which the build job and this job both check). The package holds
no other program: the gadget in `usbboot\mass-storage-gadget64` is data the app sends to the FIBER.

Signatures use SHA-256 and an RFC 3161 timestamp from `http://timestamp.acs.microsoft.com`. Microsoft's
certificates are valid for about three days, so only the timestamp keeps a signature valid after that.

Verification steps fail the job unless each of our files has a valid signature by "HARDWARIO a.s." with a
timestamp, the only file of others is the one named above, and **Windows takes the signed
driver package**: its signer is put among the machine's trusted publishers (what "Always trust software from
HARDWARIO a.s." does on a user's computer) and the package is added to the driver store with `pnputil`. A
signature check alone would not show that; a driver package is judged by stricter rules than a program.

CI (`scripts/windows-test.ps1`, every push) checks the rest on a Windows machine without a FIBER: Windows
takes the driver package when a certificate made for the run signs it, the app installs it by itself, the
installer installs (the app, the uninstaller and no other program) and uninstalls.

A new certificate has no reputation with SmartScreen: early downloads may still show "More info, Run anyway".
That is not a signing failure.

When: for a pushed release tag (`v*`) and for a dry run started on one. Nothing else reaches the job.

What it depends on:

- The GitHub environment **`signing`** (see macOS above), with the variables `AZURE_CLIENT_ID`,
  `AZURE_TENANT_ID` and `AZURE_SUBSCRIPTION_ID` (not secrets).
- On the Azure side: the Entra app `github-codesign`, whose client ID is `AZURE_CLIENT_ID`, with the role
  "Artifact Signing Certificate Profile Signer" on the `hardwario` profile only, and a federated credential
  that trusts GitHub's OIDC tokens for the subject
  `repo:hardwario@28539912/fiber-setup@1446010974:environment:signing`. The repository uses GitHub's
  immutable subject (with the owner and repository IDs), so a re-created repository of the same name is
  not trusted. The job logs in with `azure/login` through OIDC (`id-token: write`); there is no client
  secret.
- NSIS 3, which the job installs with Chocolatey (GitHub's Windows machines do not have it).

## Linux

The AppImage and the .deb are not signed; their checksums are what a download is checked against.
