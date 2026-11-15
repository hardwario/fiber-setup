# Releasing

## Steps

1. Set the new version in `package.json` and in `Cargo.toml` (the workspace: the core, the CLI and the app
   take theirs from it, and so does Tauri). `node scripts/check-version.mjs v1.2.3` checks that they agree.
2. Commit, then tag and push: `git tag v1.2.3 && git push origin v1.2.3`.
3. The Release workflow (`.github/workflows/release.yml`) tests on Linux and macOS, builds both, signs and
   notarizes the macOS app, and creates a **draft** release with all files and their `.sha256` checksums.
   The signing job waits for an approval: open the run and choose **Review deployments**, **signing**,
   **Approve and deploy** (GitHub also sends a notification). Approve only a run of a tag you pushed
   yourself.
4. Check the draft on GitHub and publish it.

A dry run builds everything without publishing: Actions, Release, Run workflow. Started on a branch it skips
the signing; started on a release tag it signs too, but still publishes nothing.

## Files

| File | Built by |
|---|---|
| `fiber-setup-<version>-macos-universal.dmg` | `build` (macos, the app), signed, notarized and packed by `sign-macos` |
| `fiber-setup-<version>-linux-x86_64.AppImage`, `fiber-setup-<version>-linux-x86_64.deb` | `build` (linux), not signed |

Each has a `<file>.sha256` next to it (`shasum -a 256 -c <file>.sha256`), made after signing and packing.
The release holds these files and nothing else; the `publish` job fails otherwise.

Not released yet: `fiber-setup-cli` (it needs rpiboot and the gadget beside it, which only the app
bundles), and anything for Windows.

## macOS signing and notarization

The `sign-macos` job signs with the team's Developer ID Application certificate (HARDWARIO a.s., team
C32QFQ68V9) and notarizes with an App Store Connect API key. The app is one build for Apple silicon and
Intel, from macOS 11 on. Order:

1. `FIBER Setup.app/Contents/Resources/usbboot/libusb-1.0.0.dylib`, the library rpiboot loads;
2. `FIBER Setup.app/Contents/Resources/usbboot/rpiboot`;
3. `FIBER Setup.app` itself, whose signature seals the two. All three are signed with the hardened runtime
   and a secure timestamp. The app is then notarized and the ticket stapled to it, so it also opens offline;
4. the dmg built from it (the app and a link to Applications): signed, notarized and stapled.

rpiboot and libusb are not ours (Raspberry Pi's usbboot and libusb.info, built from their pinned sources by
`scripts/fetch-usbboot.sh`), but they must carry our signature all the same: Apple refuses an app with any
program in it that is not signed with a Developer ID, and with the hardened runtime rpiboot loads only a
library signed by the same team. The job first checks that the app holds exactly these three programs and
stops if it finds another.

A verification step fails the job unless Gatekeeper (`spctl`) accepts the app and the dmg, both tickets are
stapled, each of the three programs is signed with Developer ID, a timestamp and the hardened runtime and is
for both architectures, and the signed rpiboot still starts with the libusb next to it.

The app needs no entitlements: it is not sandboxed, WebKit runs its JIT in Apple's own processes, and USB
(rpiboot) and the tools the app starts (`ioreg`, `diskutil`, `authopen`) need none outside the sandbox.

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

## Windows

FIBER Setup does not run on Windows yet (README, "Not in this version"), so the workflow builds and signs
nothing for it. When it does, the Windows binaries are signed the way TOWER Bridge's are: a `sign-windows`
job in the same `signing` environment, Authenticode through Microsoft Azure Artifact Signing (account
`hardwario`, certificate profile `hardwario`, endpoint `https://weu.codesigning.azure.net/`), every signature
with an RFC 3161 timestamp from `http://timestamp.acs.microsoft.com`. That needs, and does not have yet:

- the variables `AZURE_CLIENT_ID`, `AZURE_TENANT_ID` and `AZURE_SUBSCRIPTION_ID` in the environment;
- a federated credential on the Entra app `github-codesign` for this repository's subject,
  `repo:hardwario@28539912/fiber-setup@1446010974:environment:signing` (GitHub's immutable form, with the
  owner and repository IDs). The app takes at most 20 of them, so this one waits until there is something
  to sign.

## Linux

The AppImage and the .deb are not signed; their checksums are what a download is checked against.
