# Releasing shimeji2

The first public candidate is **v0.1.0-beta.1**, Apple Silicon, macOS 14+.
The application version remains 0.1.0. Build and run checks were performed on
macOS 15; another Mac's first download/open is a manual acceptance step.

## Local candidate

```sh
./scripts/bundle.sh --offline
python3 scripts/prepare-release.py
```

Python3.11+ is required for prepare-release.py. The first command builds the app and hook with hardened-runtime ad-hoc signatures,
minimum deployment target14.0, the included icon and the unchanged default shime.
It writes target/bundle/shimeji2.app, shimeji2.dmg and shimeji2.zip.
Remove --offline when dependencies need downloading. Only installed Rust targets
are used. With both Apple Silicon and Intel targets installed, the app is universal.
No targets or certificates are installed by the script.

The second command creates versioned archives, a source snapshot, checksums,
release notes and a machine-readable manifest under target/release-candidate/.
It includes current source changes even when they are uncommitted, but excludes
personal configs, output/, corpus packs, vendor checkouts, internal handoff notes,
spike logs and build output. This snapshot is suitable for inspection and building;
it is not a git-history export or a publication operation.

The app contains LICENSE, NOTICE.md, THIRD-PARTY-NOTICES.md (65Rust packages),
READ-ME-FIRST.md, shime's upstream notices,
and shimeji2.icns. The DMG also includes READ-ME-FIRST.md. PNG/XML byte identity
is checked by the bundle builder and by shimeji-packs tests.

## Repository and update links

packaging/repository.txt is configured for https://github.com/AlanJager/shimeji2.
Set a different canonical HTTPS GitHub repository URL there when maintaining a fork. A blank file is supported for local review and
**disables Check for Updates**, rather than opening a placeholder URL.
The builder validates the URL, derives its /releases page and embeds it via
SHIMEJI_RELEASES_URL. The app opens that page through macOS; it does not poll it.
A direct cargo build can set SHIMEJI_RELEASES_URL explicitly when desired.

No git remote is created or changed by these scripts. The Homebrew cask is an
inactive template (packaging/homebrew/shimeji2.rb.in); after a signed release,
fill it with the real repository and DMG digest. No Homebrew install is promised
for the initial ad-hoc beta.

## Runtime and manual acceptance

Ordinary run mode continues until Quit. --seconds N sets a finite positive duration;
benchmark scenarios baseline/s1–s5 default to20seconds when it is omitted.
This policy is tested as pure Rust and does not require a WindowServer.

Test with a throwaway profile, one running application at a time:

```sh
open -W -n target/bundle/shimeji2.app --args \
  --seconds 30 --profile /tmp/shimeji2-review --agent-home /tmp/shimeji2-review/agent-home
```

For release acceptance, also launch without --seconds and confirm it remains open
past20seconds. Verify the relocated extracted application's own pack path,
all46images, the icon, Settings, drag/right-click, hide/show and Quit.
Use a fresh profile on a second Mac for first-open and Accessibility checks;
do not treat same-machine LaunchServices success as second-machine acceptance.
Detailed checklist: docs/testing/public-beta-checklist.md.

Required source checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

scripts/check-entitlements.sh verifies hardened runtime and rejects unexpected
com.apple.security.* entitlements. The application and hook are signed separately.
The candidate scripts verify ZIP inventories, app signatures, architectures,
minimum OS, icons and archive hashes before reporting success.

## Public scope and known limitations

The default white shime is preserved with upstream notices and the documented
art-license ambiguity in assets/packs/shime/ATTRIBUTION.md. Custom Waner/Hanayo art,
third-party corpus packs and their traces are excluded from source and app archives.
The new application icon is AI-assisted original project artwork, separately documented.

Window mischief and agent hooks are experimental and opt-in; sounds are off by default.
Real ToDesk/DingTalk carry acceptance is pending. Some third-party pack extensions
and two recorded compatibility differences are unresolved. Broad multi-display,
sleep/wake and long-duration coverage is incomplete. Latest-runtime energy needs
remeasuring before any universal CPU or energy claim.

## Downloaded first open

Ad-hoc signing is not Developer ID signing or notarization. A quarantined downloaded
app may need an explicit per-app approval in System Settings → Privacy & Security.
See packaging/READ-ME-FIRST.md and [Apple's instructions](https://support.apple.com/en-us/102445).
No instructions require disabling system-wide Gatekeeper.
Accessibility approval belongs to the app signature; rebuilding may require a new grant.

## Developer ID and automation

Developer ID signing and notarization are not enabled for this beta. The local
builder supports --sign-identity once a certificate is available. After signing,
submit the ZIP/DMG using xcrun notarytool, staple the app/DMG, verify with spctl,
and regenerate release checksums from the final artifacts.

The initial public source uses the local fmt/clippy/test and packaging checks
above. GitHub Actions configuration is omitted from this first source submission:
the current publishing credential has repository access without workflow-write
permission. Source publication does not require widening that permission. CI can
be enabled separately when an appropriate credential is available.

No signing credentials belong in source. A future signed workflow would require
APPLE_CERTIFICATE_P12, APPLE_CERTIFICATE_PASSWORD, APPLE_SIGNING_IDENTITY and the
notarization values APPLE_NOTARY_KEY, APPLE_NOTARY_KEY_ID, APPLE_NOTARY_ISSUER.
Homebrew remains an inactive template until a signed release has final URLs/hashes.

## Candidate verification (2026-10-14)

Local fmt/clippy and the complete workspace test suite pass:455passed,0failed,7ignored.
The relocated Apple Silicon app loads all46images from its own resources and remains
alive30seconds without --seconds. Both executable binaries contain no build-host home
path. Signature, hardened-runtime entitlements, system dependencies and ten icon
representations pass. The extracted source snapshot matches its per-file hashes and
resolves locked/offline Cargo metadata; it has not been independently compiled on a
second machine. The repository and updates page are configured for AlanJager/shimeji2. Notarization
and second-Mac first open remain outstanding owner/environment inputs.
