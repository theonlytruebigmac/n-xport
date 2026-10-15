# N-xport — N-Central Data Tool

A cross-platform desktop application for exporting, importing, and migrating data on N-able N-central RMM servers through the N-central REST API (with the SOAP API where N-central offers no REST equivalent). A command-line tool covers exports and profile management.

More detail: [Technical overview](docs/TECHNICAL_OVERVIEW.md) (architecture, migration and import behavior, credential storage, testing) and [Releasing](docs/RELEASING.md).

## Features

### Data Export
- Export data from N-central to CSV and JSON formats
- Supports exporting:
  - Service Organizations
  - Customers
  - Sites
  - Devices
  - Access Groups
  - User Roles
  - Users
  - Organization Custom Properties
  - Device Custom Properties
  - Device hardware/software assets
  - Registration tokens (agent registration token and expiry for every customer and site; off by default, export only, and the file contains live secrets)
- The completion summary reports logical records written (each record counted once, even when both CSV and JSON are selected) and distinguishes complete, partial, failed, and cancelled exports. A partial export lists the items that could not be fetched.

### CSV Import
- Import from CSV templates generated in the app, with a **dry-run** that validates every row and reports what would happen without writing to N-central
- Supported: Customers, Sites, Access Groups, User Roles, Users, Organization property values, Device property values
- Each row gets its own outcome (created, updated, skipped, planned, or error); rows that already exist are skipped, so re-running a file does not create duplicates
- CSV templates can be downloaded from the Setup page, before connecting. Files saved by Excel as "CSV UTF-8" (which adds a byte order mark) are read correctly.
- Not supported: Service Organizations, devices, and device assets

### Generate Installer Scripts
- Creates a Windows agent installer script for each customer or site you pick, using that customer's or site's existing registration token
- Each script downloads the current agent from your N-central server and installs it silently for that customer or site; run it from an elevated PowerShell session
- Scripts are written to a new, timestamped folder. A customer or site with no token, or an unreadable one, is reported and skipped; an expired token is reported too, and its script is still written
- **Each script contains a registration token in plain text.** Store and share the files securely and delete them after deployment
- Windows agents only. The install logic is adapted from the N-central Customer Installer Generator sample script by Morgan Rourke ([MorganR99](https://github.com/MorganR99)); the generated scripts carry the same disclaimer

### Data Migration
- Server-to-server migration between N-central instances
- Migrates customers and sites, user roles, users, access groups, organization custom-property values, and device custom-property values (for devices already on the destination)
- A **preflight summary** before the run shows the steps in order, a **Before you start** list of what must already exist on the destination, which steps use SOAP, and the manual follow-up already known
- Existing entities (matched by name within the same org unit scope) are skipped, so a migration can be repeated safely
- The result lists failures, skipped items, and manual follow-up separately; a run is only reported as fully successful if nothing needs attention

### Scope and limits
- Service-organization import and device-record creation are not supported. Device record creation is separate from agent deployment; N-xport does not deploy agents.
- Organization custom-property values can be imported and migrated using existing property definitions. Device custom-property values can be imported and migrated for existing devices that can be matched by long name within their org-unit scope. CSV templates are available in the import workflow.
- Property CSV rows identify existing definitions by `propertyName`; organization rows use optional `customerName`/`siteName`, and device rows use `customerName`, optional `siteName`, and `deviceName`. A blank `value` intentionally clears the current value.
- **Custom properties need preparation on the destination.** N-xport copies property *values* only. The definitions (organization and device custom properties) must already exist on the destination, because the N-central API cannot create them; values for a missing definition are reported and skipped. Device property values also need the *devices* to already be on the destination: N-xport does not move or create devices, so install or register each device's agent against the destination server first. The migration preflight lists these under "Before you start", and the import screen shows the same notice for property imports.
- Raw hardware/software asset inventory is export-only. Writing asset lifecycle fields (warranty, purchase, lease, replacement, cost, tag, location, or description) is not currently supported.
- Access groups: N-central cannot change the membership of an existing group. Groups that already exist on the destination are left unchanged (and reported only if a migrated user is missing from them), groups are created with their members only when Users is migrated in the same run, and device-based access groups are not migrated. Groups defined above the source service org (inherited into it from a parent) are not migrated.
- Property values are written only where they differ from what the destination already shows, so a value a customer merely inherits is not copied as an override.
- N-central logins are unique per server. A user whose login already exists elsewhere on the destination server cannot be created there, and is reported with that reason.
- Supported N-central versions are not certified. Reads, export, import (dry-run and real writes for customers, sites, roles, access groups, users, and property values), the SOAP user and role calls, and a full migration between two servers have been checked against N-central 2026.4.0.25 (source) and 2026.4.0.27 (destination) demo servers. Other versions are untested.

### Connection Management
- Multiple server profiles, with separate source and destination connections for migration
- JWT-based API authentication (the server address is always contacted over HTTPS)
- Selecting a profile or reconnecting drops the previous connections first, so nothing from the old profile stays live. Connecting to a different server than the selected profile's never overwrites that profile's saved token, and profile names that end in `_dest` are refused (they would collide with a destination credential).
- Long runs keep working: if the session's refresh token expires partway through, the app signs in again with the same API token, and a read that fails on a dropped connection or timeout is retried
- Credentials are stored in the operating-system keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service). If there is no working keychain, **nothing is written to disk by default**: the credential is kept for the session only and the app warns you. You can opt in to a file encrypted with a passphrase you choose. See [Credential storage](#credential-storage).

### User Interface
- Dark-themed interface
- Real-time progress tracking with a detailed activity log
- Cancel at any time; work already completed is kept and the result is reported as cancelled
- Export format selection (CSV, JSON, or both) and custom export directory
- A **Settings** page (the gear icon on the home screen) holds credential storage
- Every workflow page has a title and a Setup, Configure, Run step indicator; while a run is going the app stays on its page and the connection cannot be dropped until it finishes or is cancelled
- The window opens at 1280×960 so the pages fit without scrolling (the minimum is 1200×700; on smaller screens the longest pages scroll inside the window)

## System Requirements

- Windows 10/11 (x64)
- macOS 11+ (Intel and Apple Silicon)
- Linux (x64, requires WebKit2GTK 4.1)

These are the intended targets. The packaged installers and the auto-updater have not yet been smoke-tested on each platform.

## Installation

Download the latest release for your platform from the [Releases](https://github.com/theonlytruebigmac/n-xport/releases) page. The release workflow builds Windows, macOS (Apple Silicon and Intel separately), and Linux installers; the exact set of files is listed on each release.

On Debian/Ubuntu, install a `.deb` with `sudo dpkg -i <file>.deb`. An `.AppImage` needs `chmod +x <file>` and can then be run directly.

Note: releases v1.0.0 through v1.2.0 have no Linux files (their Linux build never ran), and the in-app updater has no Linux entry for them. Linux installers and updates return with the first release built by the corrected workflow; the last release with Linux files is v0.1.12.

## Usage

### Connecting to N-central

1. Choose Export, Import, or Migrate on the home screen
2. Enter your N-central server FQDN (e.g. `ncentral.example.com`). Connections always use HTTPS. A pasted `https://` or `http://` prefix, a trailing slash, a page path and surrounding spaces are all removed.
3. Enter your API JWT token and your **API username**. The username is required for the SOAP API, which user creation and some other operations use (see [SOAP API](#soap-api)).
4. Click **Connect**. For a migration, connect the destination server as well.
5. Optionally save the connection as a profile. Selecting another profile disconnects the current one first.

### Exporting data

1. Connect, then pick the service organization and the data types to export
2. Choose export formats (CSV and/or JSON) and an output folder. It starts as `nc_export` inside your Downloads folder, and the folder you pick is remembered for next time.
3. Click **Start export**. Nothing is selected to begin with, so you can tick just one type; **Select all** ticks everything.
4. Files are written to the output folder.
5. Check the completion summary: a *partial* export means some items could not be fetched, and the activity log lists them.

### Importing data from CSV

1. Connect, then choose the resource type and download its CSV template
2. Fill in the template and select the file
3. Leave **Dry-run** on and run it first: every row is validated against the server and reported as *planned*, *skipped*, or *error* without any change
4. Turn dry-run off and apply. Rows whose entity already exists are skipped, so applying the same file twice does not create duplicates.

### Generating installer scripts

1. Choose **Generate Installer Scripts** on the home screen and connect
2. Pick the service organization and the folder to save into, then tick the customers and sites you want (use the filter to narrow the list)
3. Generate. The scripts go in a new folder inside the one you chose, named with the server and the time.

### Migrating data

1. Connect the source and destination servers and pick the service organization on each
2. Select the data to migrate. The **preflight summary** shows the steps in the order they run, a **Before you start** list of what must already exist on the destination (property definitions, and devices for device properties), which steps use SOAP, and manual follow-up that is already known
3. Click **Start migration**
4. Review the result. Failures, skipped (already existing) items, and manual follow-up are listed separately.

Steps run in this order: customers and sites, user roles, users, access groups, organization properties, device properties. Users run before access groups because group membership can only be set when a group is created. If Customers is not selected, destination customers and sites are matched to the source by unique name; unmatched or ambiguous ones are reported and skipped along with everything beneath them.

### SOAP API

N-central's SOAP API is disabled by default. Enable it on the server before running operations that need it:

- **Required:** creating users (import and migration)
- **Preferred:** customer- and site-level user roles use SOAP when it is available and REST otherwise; on N-central 2026.4 REST places them correctly (checked on a live server), but older servers were reported to place them at the service-org level, so check where they landed if SOAP is unavailable
- **Fallback only:** customers, access groups, and organization properties are written over REST first; SOAP is tried only if the REST call fails
- **Never used:** exports, sites, device properties, and dry runs

When SOAP is disabled or the API username is wrong, SOAP-dependent rows or items fail and are reported as failures. On servers older than N-central 2026.4, a customer- or site-level role created over REST because SOAP was unavailable may land at the service-org level while still being reported as created.

### Credential storage

JWTs (and an optional API password) are saved to the OS keychain. If the keychain fails or does not exist, the credential is **not saved**: it is kept in memory until you close the app, and the app says so. To remember credentials without a keychain (for example on headless Linux), opt in to **encrypted file storage**:

- In the app: the gear icon on the home screen → Settings → *Credential storage* → *Turn on encrypted file storage*. From the command line: `nc-export profile enable-encrypted-storage`.
- You choose a passphrase of at least 12 characters. The file is encrypted with a key derived from it (Argon2id) and each credential is sealed with ChaCha20-Poly1305; nothing in it is readable without the passphrase. **There is no recovery**: if you forget the passphrase, delete the file (*Forgot it? Delete the file*, or `nc-export profile disable-encrypted-storage --delete-file`) and enter your credentials again.
- The file is *locked* each time the app starts and must be unlocked with the passphrase before saved credentials in it can be used. The CLI prompts for it, or reads `NC_CREDENTIALS_PASSPHRASE` for scripts (other processes of your user can read environment variables, so prefer the prompt).
- Turning the option off keeps the file but makes it unreadable; you can delete it at the same time. `nc-export profile storage-status` shows the current state.

Earlier versions wrote credentials to a file that only lightly obfuscated them. Those entries are moved into your keychain automatically when it works; otherwise they stay readable, the app warns you, and turning on encrypted file storage re-encrypts them and removes the old file. Deleting a profile removes its source and destination credentials from every store and reports any it could not remove.

## Command line

The same crate builds a command-line tool, `nc-export`, for scripted exports and profile management. It is not included in the installers; build it from source:

```bash
cd src-tauri
cargo build --release --bin nc-export     # binary: target/release/nc-export
```

```bash
# Check a connection (JWT from --jwt or the NC_JWT environment variable)
NC_JWT=... nc-export test --server ncentral.example.com

# Export one service organization (add -f json for JSON as well; --all selects every type the CLI supports)
NC_JWT=... nc-export export --server ncentral.example.com --service-org 50 --all -f csv -o ./nc_export

# Or use a saved profile and its stored credentials
nc-export profile add Prod --server ncentral.example.com --service-org 50
nc-export profile set-credentials Prod          # reads the JWT from stdin
nc-export export --profile Prod --service-org 50 --customers --sites --devices
```

Export types: `--service-orgs`, `--customers`, `--sites`, `--devices`, `--access-groups`, `--user-roles`, `--org-properties` (with none selected, all of these are exported). Profile commands: `list`, `add`, `delete`, `use`, `set-credentials`, and the encrypted-storage commands described under [Credential storage](#credential-storage).

What the CLI does **not** do, compared with the app:
- Import and migration are app-only.
- It exports no users, device assets, registration tokens, or device custom properties (`--device-properties` prints a notice instead).
- Access groups, user roles, and organization properties are exported for the service org itself, not every customer and site beneath it. Sites and devices are limited to the service org you name, but `--service-orgs` lists every service org the token can see.

## Development

### Prerequisites

- Node.js 22.22+ or 24.15+ (the frontend test tooling, jsdom and Vitest, sets this floor; building alone only needs Vite 7's 20.19+). `.nvmrc` selects Node 24 for `nvm use`, and `package.json` declares the range in `engines`.
- Current stable Rust (CI uses the stable toolchain)
- Platform-specific dependencies (see below)

### Linux Dependencies

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```

### Setup

```bash
# Clone the repository
git clone https://github.com/theonlytruebigmac/n-xport.git
cd n-xport

# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

### Checks

CI (`.github/workflows/ci.yml`) runs these on every push to `main` and every pull request; run them locally before pushing:

```bash
npx tsc --noEmit && npm test && npm run build   # frontend typecheck, tests, production build

cd src-tauri
cargo fmt --check                                              # formatting is enforced
cargo clippy --all-targets --all-features -- -D warnings       # no warnings allowed
cargo test --all-targets                                       # tests use a local fake N-central server
```

Frontend tests (Vitest with Testing Library, in `src/**/*.test.tsx`) render the real app with the Tauri backend mocked, and cover connecting, export selection and Select all, import dry-run and apply, import templates, the SOAP notice, cancellation, profile switching and deletion, the credential-overwrite guard, the output folder default, the lock while a run is going, installer script selection and generation, destination service orgs, and the migration preflight panel. The Rust tests do not need a live N-central server. They run the API client, import handlers, and migration steps against a stateful fake server (`src-tauri/src/test_support.rs`). That complements, but does not replace, validation against a real N-central instance.

### Project Structure

```
n-xport/
├── src/                    # React frontend
│   ├── App.tsx            # Main application component and workflow state
│   ├── api.ts             # Tauri command invocations
│   ├── types.ts           # TypeScript type definitions
│   ├── components/        # Home, settings, setup, configure, import, installer, progress panels; page head; credential storage card
│   └── test/              # Tauri-backend mock and setup for the frontend tests
├── src-tauri/             # Rust backend
│   ├── src/
│   │   ├── api/           # N-central REST client, auth, rate limiter, SOAP client
│   │   ├── cli/           # Command-line runner for nc-export
│   │   ├── bin/           # nc-export entry point
│   │   ├── commands/      # Tauri commands (connection, export, import, installers, migration/, preflight)
│   │   ├── import/        # CSV row models, templates, handlers/ (one module per resource)
│   │   ├── credentials/   # Keychain storage with an opt-in passphrase-encrypted file
│   │   ├── config/        # Settings and profiles
│   │   ├── export/        # Export writers (CSV, JSON)
│   │   ├── models/        # Data models
│   │   └── lib.rs         # Main library
│   ├── data/              # Bundled data: the role permission list (rolePermissionIds.csv) and the installer script template
│   ├── icons/             # App icons; icons/source/ holds the images they are generated from
│   ├── tests/             # Opt-in live-server tests
│   ├── Cargo.toml         # Rust dependencies
│   └── tauri.conf.json    # Tauri configuration
├── docs/                  # Technical overview and release process
│   └── api/               # Checked-in N-central API-Service OpenAPI spec
├── scripts/               # bump-version.sh: sets the version in package.json, Cargo.toml, and tauri.conf.json
├── LICENSE                # MIT
├── .nvmrc, .editorconfig  # Node version for nvm; editor basics (UTF-8, LF, final newline)
└── .github/workflows/     # CI and release workflows
```

## API Reference

The application uses the N-central API-Service REST API as described by the OpenAPI spec checked in as `docs/api/n-central-openapi-spec.json` (spec version 1.0). SOAP is used only as described under [SOAP API](#soap-api). Required API permissions:
- Read access to all data types you want to export
- Write access for migration operations

### Generating an API Token

1. Log into N-Central as an administrator
2. Navigate to Administration > User Management
3. Select your user and go to the API tab
4. Generate a new JWT token
5. Copy the token and use it in the application

## Auto-Updates

The application checks for updates automatically on startup and can download and install updates from GitHub Releases.

## Releasing

Releases are built by GitHub Actions when a `v*` tag is pushed. See [docs/RELEASING.md](docs/RELEASING.md) for the steps (version bump, tagging, publishing) and for what to do if something goes wrong.

## License

MIT License - See LICENSE file for details.

The installer script template is adapted from a sample script by Morgan Rourke ([MorganR99](https://github.com/MorganR99)); see the header of `src-tauri/data/install_agent.ps1.template`.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes, and run the [checks](#checks) (CI enforces them)
4. Submit a pull request

## Support

For issues and feature requests, please use the [GitHub Issues](https://github.com/theonlytruebigmac/n-xport/issues) page.
