# MutAnt Project Setup Guide

This guide provides comprehensive instructions for setting up the MutAnt project development environment on different platforms.

## Quick Start

The easiest way to set up the project is to use the automated setup scripts:

### Cross-Platform (Recommended)
```bash
./setup
```

### Platform-Specific Scripts

#### Linux/macOS
```bash
./setup.sh
```

#### Windows (PowerShell)
```powershell
.\setup.ps1
```

## Setup Script Options

All setup scripts support the following options:

- `--skip-tests` - Skip running tests during setup
- `--skip-web` - Skip web dependencies setup (Node.js, pnpm, webpack)
- `--skip-ant` - Skip Autonomi CLI installation
- `--help` - Show help message

### Examples

```bash
# Skip tests and web setup
./setup --skip-tests --skip-web

# Only install Rust dependencies
./setup --skip-web --skip-ant
```

## What the Setup Scripts Install

### System Dependencies
- **Build tools**: GCC, Make, or Visual Studio Build Tools (Windows)
- **SSL libraries**: OpenSSL development libraries
- **Process tools**: procps (for pgrep command)
- **Basic tools**: curl, wget, git, unzip

### Rust Toolchain
- **Rust stable and nightly**: Latest versions via rustup
- **Components**: clippy (linting), rustfmt (formatting)
- **WASM tools**: wasm-pack for WebAssembly builds

### Node.js Environment (if not skipped)
- **Node.js**: Latest LTS version
- **Package managers**: npm and pnpm
- **Build tools**: webpack and related dependencies

### Autonomi CLI (if not skipped)
- **antup**: Autonomi installer tool
- **ant**: Autonomi CLI client

### Project Build
- **Workspace build**: All Rust crates in the workspace
- **Code quality**: Runs clippy and rustfmt
- **Web build**: WASM module compilation (if web not skipped)
- **Tests**: Unit tests to verify installation (if not skipped)

## Manual Installation

If you prefer to install dependencies manually or the automated scripts don't work for your system:

### 1. Install Rust

```bash
# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install toolchains and components
rustup install stable nightly
rustup default stable
rustup component add clippy rustfmt

# Install wasm-pack
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
```

### 2. Install System Dependencies

#### Ubuntu/Debian
```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev procps curl git wget unzip
```

#### CentOS/RHEL/Fedora
```bash
sudo yum groupinstall -y "Development Tools"
sudo yum install -y openssl-devel procps-ng curl git wget unzip
```

#### macOS
```bash
# Install Homebrew if not present
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install dependencies
brew install openssl curl git wget
```

#### Windows
```powershell
# Install Chocolatey
Set-ExecutionPolicy Bypass -Scope Process -Force
iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))

# Install dependencies
choco install -y git curl wget 7zip vcredist-all
choco install -y visualstudio2022buildtools visualstudio2022-workload-vctools
```

### 3. Install Node.js (for web interface)

#### Linux/macOS
```bash
# Using Node Version Manager (recommended)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.0/install.sh | bash
nvm install --lts
nvm use --lts

# Install pnpm
npm install -g pnpm
```

#### Windows
```powershell
choco install -y nodejs
npm install -g pnpm
```

### 4. Install Autonomi CLI

```bash
# Install antup
curl -sSf https://raw.githubusercontent.com/maidsafe/antup/main/install.sh | sh

# Install ant client
antup client
```

### 5. Build the Project

```bash
# Build all workspace crates
cargo build --workspace

# Run code quality checks
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all

# Build web interface (optional)
cd mutant-web
wasm-pack build --target web --out-dir pkg
pnpm install
pnpm run build
cd ..

# Run tests
cargo test --workspace --lib
```

## Post-Installation Setup

### 1. Configure Autonomi Wallet

You need an Autonomi wallet to store and retrieve data:

```bash
# Create a new wallet
ant wallet create

# Or import an existing wallet
ant wallet import YOUR_PRIVATE_KEY_HERE
```

### 2. Install CLI Tools

```bash
# Install the CLI and daemon
cargo install --path mutant-cli
cargo install --path mutant-daemon
```

### 3. Verify Installation

```bash
# Check CLI installation
mutant --help

# Check daemon installation
mutant-daemon --help

# Test basic functionality (requires wallet setup)
mutant ls
```

## Development Environment

### Local Testnet

For development and testing, you can run a local Autonomi testnet:

```bash
# Start local testnet
./scripts/manage_local_testnet.sh start

# Check status
./scripts/manage_local_testnet.sh status

# Stop testnet
./scripts/manage_local_testnet.sh stop
```

### Running Tests

```bash
# Unit tests only
cargo test --workspace --lib

# Integration tests with local testnet
./scripts/run_tests_with_env.sh

# Specific test
./scripts/run_tests_with_env.sh test_name
```

### Web Development

```bash
cd mutant-web

# Install dependencies
pnpm install

# Development server
pnpm run serve

# Build for production
pnpm run build
```

## Troubleshooting

### Common Issues

#### Rust Installation Issues
- **Permission denied**: Make sure you have write access to `~/.cargo`
- **PATH not updated**: Restart your terminal or run `source ~/.cargo/env`
- **Old Rust version**: Run `rustup update` to get the latest version

#### Build Failures
- **Missing system dependencies**: Install build tools for your platform
- **SSL errors**: Install OpenSSL development libraries
- **WASM build fails**: Make sure wasm-pack is installed and in PATH

#### Network Issues
- **ant CLI not found**: Make sure `~/.local/bin` is in your PATH
- **Connection failures**: Check your internet connection and firewall settings

#### Windows-Specific Issues
- **PowerShell execution policy**: Run `Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser`
- **Visual Studio Build Tools**: Make sure C++ build tools are installed
- **Long path issues**: Enable long path support in Windows

### Getting Help

If you encounter issues not covered here:

1. Check the [project README](README.md) for additional information
2. Look at existing [GitHub issues](https://github.com/Champii/Mutant/issues)
3. Create a new issue with:
   - Your operating system and version
   - Error messages (full output)
   - Steps you've already tried

## Environment Variables

The following environment variables can be used to customize the setup:

- `CARGO_HOME` - Cargo installation directory (default: `~/.cargo`)
- `RUSTUP_HOME` - Rustup installation directory (default: `~/.rustup`)
- `XDG_DATA_HOME` - Data directory for local testnet (default: `~/.local/share`)

## Security Considerations

- **Private keys**: Never commit private keys to version control
- **Wallet security**: Keep your wallet private key secure and backed up
- **Network access**: The project requires internet access for Autonomi network
- **Local testnet**: Only use test keys with local testnet, never mainnet keys

## Next Steps

After successful setup:

1. Read the [Getting Started Guide](docs/mutant_lib/getting_started.md)
2. Explore the [API Documentation](docs/mutant_lib/api_reference.md)
3. Check out the [Architecture Overview](docs/mutant_lib/architecture.md)
4. Try the [CLI Examples](README.md#command-line-interface-cli)
