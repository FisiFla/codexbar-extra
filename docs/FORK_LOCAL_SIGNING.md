# Local signing (CodexBar Extra fork)

## Why

`Scripts/package_app.sh` defaults to **ad-hoc** signing. An ad-hoc signature has no
certificate, so the app's designated requirement is its **CDHash** — a hash of the exact
binary. Every rebuild produces a different CDHash:

```
$ codesign -d -r- CodexBar.app          # ad-hoc
# designated => cdhash H"dc234cefb59a8f529f33e1c765c3469f4a0242a9"
```

macOS keys privacy decisions to that requirement, so each rebuild looks like a brand-new
app and the app loses:

- **Files & Folders → Google Chrome** (what browser-session providers need, e.g. DeepSeek,
  Muse Code, Cursor)
- Keychain "Always Allow" grants for Chromium *Safe Storage* decryption

Symptom: after `make release`, a browser-cookie provider stops updating (no history file,
an error in its menu row) until the prompts are approved again.

## Fix: sign with a stable local certificate

Signing with a self-signed certificate replaces the CDHash with a certificate-based
requirement, which does not change when the binary does:

```
$ codesign -d -r- CodexBar.app          # CODEXBAR_SIGNING=local
designated => identifier "com.steipete.codexbar" and certificate root = H"881cc6…"
```

Grants then survive every future rebuild.

### One-time setup

Create a self-signed code-signing certificate named **CodexBar Local Signing** in the login
keychain (Keychain Access → Certificate Assistant → *Create a Certificate…*, type
*Code Signing*), or non-interactively:

```bash
CN="CodexBar Local Signing"
openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
  -keyout /tmp/k.pem -out /tmp/c.pem \
  -subj "/CN=$CN/OU=CodexBar/O=CodexBar-extra" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning"
openssl pkcs12 -export -legacy -out /tmp/c.p12 -inkey /tmp/k.pem -in /tmp/c.pem \
  -name "$CN" -passout pass:codexbar
security import /tmp/c.p12 -k "$HOME/Library/Keychains/login.keychain-db" \
  -P codexbar -T /usr/bin/codesign -T /usr/bin/security
rm -f /tmp/k.pem /tmp/c.pem /tmp/c.p12
```

Notes:

- `-legacy` is required on OpenSSL 3/4; without it macOS rejects the PKCS#12 with
  *"MAC verification failed during PKCS12 import"*.
- The certificate does **not** need to be trusted, and it will not appear in
  `security find-identity -p codesigning -v` (self-signed identities are never "valid").
  `codesign` signs with it regardless, and the resulting requirement is still stable. So
  `local` mode lists identities **without** `-v`.

### Building

```bash
CODEXBAR_SIGNING=local make release
```

Overrides: `CODEXBAR_LOCAL_IDENTITY` (name or SHA-1 to match, default
`CodexBar Local Signing`) and `APP_TEAM_ID` (default `Y5PE65HELJ`).

The team ID is deliberately kept at `Y5PE65HELJ` so `APP_GROUP_ID`
(`<team>.com.steipete.codexbar`) — and therefore the widget's shared container — does not
change. Because `local` is not `identity`, the iCloud/CloudKit entitlements and the embedded
Developer ID provisioning profile are **not** applied; hardcoded `FEED_URL=""` already keeps
Sparkle self-update off in every mode.

### After switching modes

The first build with the new identity changes the requirement once more, so approve the
Files & Folders / Keychain prompts one final time. From then on they persist.

## Other modes

- `CODEXBAR_SIGNING=adhoc` (default) — no certificate, identity changes per build.
- `CODEXBAR_SIGNING=identity` — a real `Developer ID Application: …(<TEAM>)` certificate;
  also enables iCloud entitlements plus the embedded provisioning profile for upstream's
  bundle ID and team.
