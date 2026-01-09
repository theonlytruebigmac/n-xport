# macOS Code Signing and Notarization Guide

This guide explains how to set up Apple code signing and notarization for the N-xport Data Tool.

## Prerequisites

- **Paid Apple Developer Account** ($99/year) - Required for notarization
- **macOS device** - Required to generate certificates
- **Xcode 14+** installed

## Step 1: Create a Developer ID Application Certificate

1. Open **Keychain Access** on your Mac
2. Go to **Keychain Access → Certificate Assistant → Request a Certificate From a Certificate Authority**
3. Fill in your email and select **Saved to disk**
4. Go to [Apple Developer - Certificates](https://developer.apple.com/account/resources/certificates/list)
5. Click **+** to create a new certificate
6. Select **Developer ID Application**
7. Upload your Certificate Signing Request (CSR)
8. Download and install the certificate to your Keychain

## Step 2: Export Certificate for CI/CD

1. In **Keychain Access**, find your "Developer ID Application" certificate
2. Right-click → **Export** as `.p12` file with a password
3. Convert to base64:
   ```bash
   base64 -i Certificates.p12 | pbcopy
   ```

## Step 3: Create App Store Connect API Key

1. Go to [App Store Connect - API Keys](https://appstoreconnect.apple.com/access/integrations/api)
2. Click **Generate API Key**
3. Name: "Notarization" (or similar)
4. Access: **Developer**
5. Download the private key (`.p8` file) - you can only download once!
6. Note the **Key ID** and **Issuer ID**

## Step 4: Configure GitHub Secrets

Add these secrets to your repository at **Settings → Secrets and variables → Actions**:

| Secret Name | Description |
|-------------|-------------|
| `APPLE_CERTIFICATE` | Base64-encoded `.p12` certificate |
| `APPLE_CERTIFICATE_PASSWORD` | Password for the `.p12` file |
| `APPLE_SIGNING_IDENTITY` | Full name of cert (e.g., "Developer ID Application: Your Name (TEAMID)") |
| `APPLE_API_KEY` | Key ID from App Store Connect |
| `APPLE_API_ISSUER` | Issuer ID from App Store Connect |
| `APPLE_API_KEY_PATH` | Path to private key (usually `/tmp/AuthKey.p8`) |

For the API key, you'll also need to write the key content to `APPLE_API_KEY_PATH` in the workflow.

## Step 5: Update GitHub Workflow

The workflow at `.github/workflows/release.yml` is already configured to use these secrets. Just ensure all secrets are properly set.

## Temporary Workaround (Without Signing)

Until code signing is set up, Mac users can install the app by:

1. **Control-click** (or right-click) on the app
2. Select **Open** from the context menu
3. Click **Open** again in the security dialog

Or:

1. Go to **System Settings → Privacy & Security**
2. Scroll down to the **Security** section
3. Click **Open Anyway** next to the app warning

## Verification

After setting up, push a new version tag to trigger a release build:

```bash
git tag v0.1.2
git push origin v0.1.2
```

Check the GitHub Actions log to verify signing and notarization succeeded.
