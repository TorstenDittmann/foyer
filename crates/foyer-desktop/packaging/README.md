# Packaging and releasing the desktop client

`.github/workflows/desktop-release.yml` builds everything. Pushing a tag
`desktop-vX.Y.Z` publishes a GitHub release; pull requests that touch
packaging, and manual runs, only build and keep the files as workflow
artifacts.

| File | Platform | Updates itself |
| --- | --- | --- |
| `Foyer-Setup-x64.exe` | Windows 10/11, x64. Per-user install into `%LOCALAPPDATA%\Programs\Foyer`, no admin rights | Yes: runs the new setup silently and restarts |
| `Foyer-macos.dmg` | macOS 11+, universal (Apple Silicon and Intel) | Yes: swaps the app bundle, if its folder is writable |
| `Foyer-macos.app.tar.gz` | The same app, downloaded by the client when it updates | |
| `Foyer-x86_64.AppImage` | Linux x86_64, built on Ubuntu 22.04 (glibc 2.35+) | Yes: replaces the AppImage file |
| `Foyer-amd64.deb` | Debian/Ubuntu | No: the client links to the download page |
| `foyer-desktop.json` | Update manifest the client reads | |
| `SHA256SUMS` | Checksums of every file | |

File names carry no version, so
`https://github.com/TorstenDittmann/foyer/releases/latest/download/<file>`
always points at the newest release. The website's `/app` page and the
client's update check both use that.

## Releasing

1. Bump `version` in `crates/foyer-desktop/Cargo.toml` and merge.
2. Tag the merge commit and push the tag:

   ```sh
   git tag desktop-v0.2.0
   git push origin desktop-v0.2.0
   ```

3. The workflow checks the tag matches the version, builds and signs on all
   three platforms, signs the update files with minisign, writes
   `foyer-desktop.json` and publishes the release as latest. Running clients
   pick it up within 30 minutes.

Other GitHub releases in this repository must not be marked "latest", or the
download links and update checks would point at them. (`gh release create
--latest=false` for anything else.)

## How self-update is secured

Every update file is signed with a [minisign](https://jedisct1.github.io/minisign/)
key. The public key is compiled into the client; the signature's trusted
comment is `foyer-desktop <version> <platform>`, so an old, genuinely signed
file can't be served as a newer version. The client also checks size and
SHA-256 from the manifest before installing anything. A client built without
the public key never installs updates; it only links to the download page.

The engine is `crates/foyer-install/src/client_update.rs`; the app wiring is
`crates/foyer-desktop/src/self_update.rs`. `FOYER_UPDATE_URL` points a client
at another manifest for testing.

## Secrets and variables

All of these are optional. Without them the workflow still produces working,
unsigned packages. Set them under **Settings → Secrets and variables →
Actions** in the GitHub repository.

### Update signing (needed for self-update)

Make a key pair once, without a password, and keep the secret key safe:

```sh
minisign -G -W -p foyer-update.pub -s foyer-update.key
```

| Name | Kind | Value |
| --- | --- | --- |
| `FOYER_UPDATE_PUBLIC_KEY` | Variable | The second line of `foyer-update.pub` (starts with `RW`) |
| `MINISIGN_SECRET_KEY` | Secret | The whole contents of `foyer-update.key` |

Changing the key later means clients built with the old key can't verify
new releases, so players would have to download the app once by hand.

### Windows code signing

Without a signature, SmartScreen warns players ("Windows protected your PC",
then More info → Run anyway). Use one of:

**Azure Trusted Signing** (Microsoft's signing service, about $10/month, the
easiest option for individuals and small companies):

| Secret | Value |
| --- | --- |
| `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` | An app registration with the "Trusted Signing Certificate Profile Signer" role on the account |
| `TRUSTED_SIGNING_ENDPOINT` | The account's region endpoint, e.g. `https://weu.codesigning.azure.net` |
| `TRUSTED_SIGNING_ACCOUNT` | The Trusted Signing account name |
| `TRUSTED_SIGNING_PROFILE` | The certificate profile name |

**A code signing certificate as a `.pfx`** (only if your certificate
authority still issues exportable certificates):

| Secret | Value |
| --- | --- |
| `WINDOWS_CERTIFICATE` | `base64 -w0 cert.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | The `.pfx` password |

`packaging/windows/sign.ps1` signs `Foyer.exe`, the installer and the
uninstaller with whichever is set.

### macOS signing and notarization

Needs an Apple Developer Program membership ($99/year). Without it the app is
signed ad hoc and players must allow it once in System Settings → Privacy &
Security → Open Anyway.

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | `base64 -i DeveloperID.p12`: a "Developer ID Application" certificate exported with its private key |
| `APPLE_CERTIFICATE_PASSWORD` | The `.p12` password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | The Apple ID email used for notarization |
| `APPLE_TEAM_ID` | The 10-character team ID |
| `APPLE_APP_PASSWORD` | An app-specific password for that Apple ID (appleid.apple.com) |

With the certificate but no `APPLE_ID` the app is signed but not notarized,
which Gatekeeper still blocks on first launch.

## Building locally

```sh
crates/foyer-desktop/packaging/linux/package.sh   # Linux: AppImage + .deb (needs cargo-deb)
crates/foyer-desktop/packaging/macos/package.sh   # macOS: .dmg + update tarball
```

Windows: build with `cargo build --release -p foyer-desktop`, install
[Inno Setup 6](https://jrsoftware.org/isinfo.php) and run
`iscc /DAppVersion=0.1.0 crates\foyer-desktop\packaging\windows\foyer.iss`.

The icons are drawn by `packaging/icon.py` (`python3 icon.py`, needs Pillow).
