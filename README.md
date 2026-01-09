# N-Central Data Export Tool

A cross-platform desktop application for exporting and migrating data from N-able N-Central RMM servers using the REST API.

## Features

### Data Export
- Export data from N-Central to CSV and JSON formats
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

### Data Migration
- Server-to-server migration between N-Central instances
- Migrate customers, users, roles, and access groups
- Automatic ID mapping between source and destination
- Skip existing entities to prevent duplicates

### Connection Management
- Multiple server profiles support
- Secure credential storage using system keychain
- JWT-based API authentication
- Connection testing and validation

### User Interface
- Modern dark-themed interface
- Real-time progress tracking with detailed logs
- Export format selection (CSV, JSON, or both)
- Custom export directory configuration

## System Requirements

- Windows 10/11 (x64)
- macOS 11+ (Intel and Apple Silicon)
- Linux (x64, requires WebKit2GTK 4.1)

## Installation

Download the latest release for your platform from the [Releases](https://github.com/theonlytruebigmac/nc-data-export-tool/releases) page.

### Windows
Download and run the `.msi` installer.

### macOS
Download the `.dmg` file, open it, and drag the application to your Applications folder.

### Linux
Download the `.deb` package (Debian/Ubuntu) or `.AppImage` (universal).

```bash
# Debian/Ubuntu
sudo dpkg -i n-central-data-export_*.deb

# AppImage
chmod +x N-Central-Data-Export_*.AppImage
./N-Central-Data-Export_*.AppImage
```

## Usage

### Connecting to N-Central

1. Enter your N-Central server FQDN (e.g., `ncentral.example.com`)
2. Enter your API JWT token
3. Click "Test Connection" to validate
4. Optionally save the profile for future use

### Exporting Data

1. Connect to your N-Central server
2. Select the data types you want to export
3. Choose export formats (CSV and/or JSON)
4. Click "Start Export"
5. Files will be saved to your configured export directory

### Migrating Data

1. Connect to both source and destination N-Central servers
2. Select the data types to migrate
3. Click "Start Migration"
4. The tool will create missing entities and map IDs automatically

## Development

### Prerequisites

- Node.js 18+
- Rust 1.70+
- Platform-specific dependencies (see below)

### Linux Dependencies

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```

### Setup

```bash
# Clone the repository
git clone https://github.com/theonlytruebigmac/nc-data-export-tool.git
cd nc-data-export-tool

# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

### Project Structure

```
nc-data-export-tool/
├── src/                    # React frontend
│   ├── App.tsx            # Main application component
│   ├── api.ts             # Tauri command invocations
│   ├── types.ts           # TypeScript type definitions
│   └── index.css          # Styles
├── src-tauri/             # Rust backend
│   ├── src/
│   │   ├── api/           # N-Central API client
│   │   ├── commands/      # Tauri commands
│   │   ├── export/        # Export handlers (CSV, JSON)
│   │   ├── models/        # Data models
│   │   └── lib.rs         # Main library
│   ├── Cargo.toml         # Rust dependencies
│   └── tauri.conf.json    # Tauri configuration
└── .github/workflows/     # CI/CD workflows
```

## API Reference

The application uses the N-Central REST API v2. Required API permissions:
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

## Building Releases

Releases are built automatically via GitHub Actions when a version tag is pushed:

```bash
# Create and push a version tag
git tag v0.1.0
git push origin v0.1.0
```

This triggers the release workflow which builds for all platforms and creates signed update artifacts.

## License

MIT License - See LICENSE file for details.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Submit a pull request

## Support

For issues and feature requests, please use the [GitHub Issues](https://github.com/theonlytruebigmac/nc-data-export-tool/issues) page.
