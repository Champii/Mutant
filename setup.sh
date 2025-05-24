#!/bin/bash

# MutAnt Project Setup Script
# This script sets up all dependencies needed to build and run the MutAnt project

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Detect OS
detect_os() {
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        OS="linux"
        if command -v apt-get &> /dev/null; then
            PACKAGE_MANAGER="apt"
        elif command -v yum &> /dev/null; then
            PACKAGE_MANAGER="yum"
        elif command -v pacman &> /dev/null; then
            PACKAGE_MANAGER="pacman"
        elif command -v zypper &> /dev/null; then
            PACKAGE_MANAGER="zypper"
        else
            log_error "Unsupported Linux distribution. Please install dependencies manually."
            exit 1
        fi
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        OS="macos"
        PACKAGE_MANAGER="brew"
    elif [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "cygwin" ]]; then
        OS="windows"
        PACKAGE_MANAGER="choco"
    else
        log_error "Unsupported operating system: $OSTYPE"
        exit 1
    fi
    
    log_info "Detected OS: $OS with package manager: $PACKAGE_MANAGER"
}

# Install system dependencies
install_system_deps() {
    log_info "Installing system dependencies..."
    
    case $PACKAGE_MANAGER in
        "apt")
            sudo apt-get update -y
            sudo apt-get install -y \
                curl \
                git \
                build-essential \
                pkg-config \
                libssl-dev \
                procps \
                wget \
                unzip
            ;;
        "yum")
            sudo yum update -y
            sudo yum groupinstall -y "Development Tools"
            sudo yum install -y \
                curl \
                git \
                openssl-devel \
                procps-ng \
                wget \
                unzip
            ;;
        "pacman")
            sudo pacman -Syu --noconfirm
            sudo pacman -S --noconfirm \
                curl \
                git \
                base-devel \
                openssl \
                procps-ng \
                wget \
                unzip
            ;;
        "zypper")
            sudo zypper refresh
            sudo zypper install -y \
                curl \
                git \
                gcc \
                make \
                openssl-devel \
                procps \
                wget \
                unzip
            ;;
        "brew")
            if ! command -v brew &> /dev/null; then
                log_info "Installing Homebrew..."
                /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
            fi
            brew update
            brew install \
                curl \
                git \
                openssl \
                wget
            ;;
        "choco")
            if ! command -v choco &> /dev/null; then
                log_error "Chocolatey not found. Please install Chocolatey first: https://chocolatey.org/install"
                exit 1
            fi
            choco install -y \
                curl \
                git \
                wget \
                unzip
            ;;
    esac
    
    log_success "System dependencies installed"
}

# Install Rust toolchain
install_rust() {
    log_info "Installing Rust toolchain..."
    
    if command -v rustc &> /dev/null; then
        log_info "Rust is already installed. Version: $(rustc --version)"
        log_info "Updating Rust toolchain..."
        rustup update
    else
        log_info "Installing Rust via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi
    
    # Ensure we have the latest stable and nightly toolchains
    log_info "Installing Rust stable and nightly toolchains..."
    rustup install stable
    rustup install nightly
    rustup default stable
    
    # Install required components
    log_info "Installing Rust components..."
    rustup component add clippy
    rustup component add rustfmt
    
    # Install wasm-pack for web builds
    log_info "Installing wasm-pack..."
    if ! command -v wasm-pack &> /dev/null; then
        curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
    else
        log_info "wasm-pack is already installed"
    fi
    
    log_success "Rust toolchain installed and configured"
}

# Install Node.js and npm
install_nodejs() {
    log_info "Installing Node.js..."
    
    if command -v node &> /dev/null; then
        log_info "Node.js is already installed. Version: $(node --version)"
    else
        case $PACKAGE_MANAGER in
            "apt")
                curl -fsSL https://deb.nodesource.com/setup_lts.x | sudo -E bash -
                sudo apt-get install -y nodejs
                ;;
            "yum")
                curl -fsSL https://rpm.nodesource.com/setup_lts.x | sudo bash -
                sudo yum install -y nodejs npm
                ;;
            "pacman")
                sudo pacman -S --noconfirm nodejs npm
                ;;
            "zypper")
                sudo zypper install -y nodejs npm
                ;;
            "brew")
                brew install node
                ;;
            "choco")
                choco install -y nodejs
                ;;
        esac
    fi
    
    # Install pnpm if not present
    if ! command -v pnpm &> /dev/null; then
        log_info "Installing pnpm..."
        npm install -g pnpm
    fi
    
    log_success "Node.js and package managers installed"
}

# Install Autonomi CLI (ant)
install_ant_cli() {
    log_info "Installing Autonomi CLI (ant)..."
    
    if command -v ant &> /dev/null; then
        log_info "ant CLI is already installed. Version: $(ant --version 2>/dev/null || echo 'unknown')"
    else
        log_info "Installing ant CLI via antup..."
        curl -sSf https://raw.githubusercontent.com/maidsafe/antup/main/install.sh | sh
        
        # Add to PATH if not already there
        if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
            echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
            export PATH="$HOME/.local/bin:$PATH"
        fi
        
        # Install the client
        if command -v antup &> /dev/null; then
            antup client
        else
            log_warning "antup installation may have failed. Please check manually."
        fi
    fi
    
    log_success "Autonomi CLI installation completed"
}

# Build Rust workspace
build_rust_workspace() {
    log_info "Building Rust workspace..."
    
    # Check if we're in the right directory
    if [[ ! -f "Cargo.toml" ]]; then
        log_error "Cargo.toml not found. Please run this script from the project root."
        exit 1
    fi
    
    # Build all workspace members
    log_info "Building all workspace crates..."
    cargo build --workspace
    
    # Run clippy for code quality
    log_info "Running clippy for code quality checks..."
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    
    # Format code
    log_info "Formatting code..."
    cargo fmt --all
    
    log_success "Rust workspace built successfully"
}

# Setup web dependencies
setup_web_dependencies() {
    log_info "Setting up web dependencies..."
    
    if [[ -d "mutant-web" ]]; then
        cd mutant-web
        
        # Install Node.js dependencies
        if [[ -f "package.json" ]]; then
            log_info "Installing Node.js dependencies with pnpm..."
            pnpm install
        fi
        
        # Build WASM module
        log_info "Building WASM module..."
        wasm-pack build --target web --out-dir pkg
        
        cd ..
        log_success "Web dependencies setup completed"
    else
        log_warning "mutant-web directory not found, skipping web setup"
    fi
}

# Setup development environment
setup_dev_environment() {
    log_info "Setting up development environment..."
    
    # Make scripts executable
    if [[ -d "scripts" ]]; then
        chmod +x scripts/*.sh
        log_info "Made scripts executable"
    fi
    
    # Create necessary directories
    mkdir -p ~/.config/mutant
    
    log_success "Development environment setup completed"
}

# Run tests to verify installation
run_tests() {
    log_info "Running tests to verify installation..."
    
    # Build tests first
    log_info "Building tests..."
    cargo test --workspace --no-run
    
    # Run unit tests (skip integration tests that require network)
    log_info "Running unit tests..."
    cargo test --workspace --lib
    
    log_success "Tests completed successfully"
}

# Main setup function
main() {
    echo "=========================================="
    echo "    MutAnt Project Setup Script"
    echo "=========================================="
    echo ""
    
    # Parse command line arguments
    SKIP_TESTS=false
    SKIP_WEB=false
    SKIP_ANT=false
    
    while [[ $# -gt 0 ]]; do
        case $1 in
            --skip-tests)
                SKIP_TESTS=true
                shift
                ;;
            --skip-web)
                SKIP_WEB=true
                shift
                ;;
            --skip-ant)
                SKIP_ANT=true
                shift
                ;;
            --help|-h)
                echo "Usage: $0 [OPTIONS]"
                echo ""
                echo "Options:"
                echo "  --skip-tests    Skip running tests"
                echo "  --skip-web      Skip web dependencies setup"
                echo "  --skip-ant      Skip Autonomi CLI installation"
                echo "  --help, -h      Show this help message"
                echo ""
                exit 0
                ;;
            *)
                log_error "Unknown option: $1"
                echo "Use --help for usage information"
                exit 1
                ;;
        esac
    done
    
    # Detect operating system
    detect_os
    
    # Install dependencies
    install_system_deps
    install_rust
    
    if [[ "$SKIP_WEB" != true ]]; then
        install_nodejs
    fi
    
    if [[ "$SKIP_ANT" != true ]]; then
        install_ant_cli
    fi
    
    # Build project
    build_rust_workspace
    
    if [[ "$SKIP_WEB" != true ]]; then
        setup_web_dependencies
    fi
    
    # Setup development environment
    setup_dev_environment
    
    # Run tests
    if [[ "$SKIP_TESTS" != true ]]; then
        run_tests
    fi
    
    echo ""
    echo "=========================================="
    log_success "Setup completed successfully!"
    echo "=========================================="
    echo ""
    echo "Next steps:"
    echo "1. Source your shell configuration: source ~/.bashrc (or restart terminal)"
    echo "2. Configure ant wallet: ant wallet create (or import existing)"
    echo "3. Build and install CLI tools:"
    echo "   cargo install --path mutant-cli"
    echo "   cargo install --path mutant-daemon"
    echo "4. Start using MutAnt: mutant --help"
    echo ""
    echo "For development:"
    echo "- Run local testnet: ./scripts/manage_local_testnet.sh start"
    echo "- Run integration tests: ./scripts/run_tests_with_env.sh"
    echo "- Build web interface: cd mutant-web && pnpm run build"
    echo ""
}

# Run main function
main "$@"
