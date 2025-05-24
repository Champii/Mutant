# MutAnt Project Setup Script for Windows
# This script sets up all dependencies needed to build and run the MutAnt project on Windows

param(
    [switch]$SkipTests,
    [switch]$SkipWeb,
    [switch]$SkipAnt,
    [switch]$Help
)

# Colors for output
$Red = "Red"
$Green = "Green"
$Yellow = "Yellow"
$Blue = "Blue"

# Logging functions
function Log-Info {
    param([string]$Message)
    Write-Host "[INFO] $Message" -ForegroundColor $Blue
}

function Log-Success {
    param([string]$Message)
    Write-Host "[SUCCESS] $Message" -ForegroundColor $Green
}

function Log-Warning {
    param([string]$Message)
    Write-Host "[WARNING] $Message" -ForegroundColor $Yellow
}

function Log-Error {
    param([string]$Message)
    Write-Host "[ERROR] $Message" -ForegroundColor $Red
}

function Show-Help {
    Write-Host "MutAnt Project Setup Script for Windows"
    Write-Host ""
    Write-Host "Usage: .\setup.ps1 [OPTIONS]"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -SkipTests    Skip running tests"
    Write-Host "  -SkipWeb      Skip web dependencies setup"
    Write-Host "  -SkipAnt      Skip Autonomi CLI installation"
    Write-Host "  -Help         Show this help message"
    Write-Host ""
}

function Test-Administrator {
    $currentUser = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($currentUser)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Install-Chocolatey {
    Log-Info "Checking for Chocolatey..."
    
    if (!(Get-Command choco -ErrorAction SilentlyContinue)) {
        Log-Info "Installing Chocolatey..."
        Set-ExecutionPolicy Bypass -Scope Process -Force
        [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
        iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))
        
        # Refresh environment variables
        $env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")
    } else {
        Log-Info "Chocolatey is already installed"
    }
}

function Install-SystemDependencies {
    Log-Info "Installing system dependencies..."
    
    # Install basic tools
    $packages = @(
        "git",
        "curl",
        "wget",
        "7zip",
        "vcredist-all"
    )
    
    foreach ($package in $packages) {
        Log-Info "Installing $package..."
        choco install $package -y
    }
    
    # Install Visual Studio Build Tools if not present
    if (!(Get-Command cl -ErrorAction SilentlyContinue)) {
        Log-Info "Installing Visual Studio Build Tools..."
        choco install visualstudio2022buildtools -y
        choco install visualstudio2022-workload-vctools -y
    }
    
    Log-Success "System dependencies installed"
}

function Install-Rust {
    Log-Info "Installing Rust toolchain..."
    
    if (Get-Command rustc -ErrorAction SilentlyContinue) {
        Log-Info "Rust is already installed. Version: $(rustc --version)"
        Log-Info "Updating Rust toolchain..."
        rustup update
    } else {
        Log-Info "Installing Rust via rustup..."
        
        # Download and run rustup installer
        $rustupUrl = "https://win.rustup.rs/x86_64"
        $rustupPath = "$env:TEMP\rustup-init.exe"
        
        Invoke-WebRequest -Uri $rustupUrl -OutFile $rustupPath
        Start-Process -FilePath $rustupPath -ArgumentList "-y" -Wait
        
        # Refresh environment variables
        $env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")
    }
    
    # Install required toolchains and components
    Log-Info "Installing Rust stable and nightly toolchains..."
    rustup install stable
    rustup install nightly
    rustup default stable
    
    Log-Info "Installing Rust components..."
    rustup component add clippy
    rustup component add rustfmt
    
    # Install wasm-pack
    Log-Info "Installing wasm-pack..."
    if (!(Get-Command wasm-pack -ErrorAction SilentlyContinue)) {
        choco install wasm-pack -y
    } else {
        Log-Info "wasm-pack is already installed"
    }
    
    Log-Success "Rust toolchain installed and configured"
}

function Install-NodeJS {
    Log-Info "Installing Node.js..."
    
    if (Get-Command node -ErrorAction SilentlyContinue) {
        Log-Info "Node.js is already installed. Version: $(node --version)"
    } else {
        Log-Info "Installing Node.js via Chocolatey..."
        choco install nodejs -y
        
        # Refresh environment variables
        $env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")
    }
    
    # Install pnpm
    if (!(Get-Command pnpm -ErrorAction SilentlyContinue)) {
        Log-Info "Installing pnpm..."
        npm install -g pnpm
    }
    
    Log-Success "Node.js and package managers installed"
}

function Install-AntCLI {
    Log-Info "Installing Autonomi CLI (ant)..."
    
    if (Get-Command ant -ErrorAction SilentlyContinue) {
        Log-Info "ant CLI is already installed"
    } else {
        Log-Info "Installing ant CLI via antup..."
        
        # Download and run antup installer
        $antupUrl = "https://raw.githubusercontent.com/maidsafe/antup/main/install.ps1"
        $antupScript = "$env:TEMP\antup-install.ps1"
        
        try {
            Invoke-WebRequest -Uri $antupUrl -OutFile $antupScript
            & $antupScript
            
            # Install the client
            if (Get-Command antup -ErrorAction SilentlyContinue) {
                antup client
            }
        } catch {
            Log-Warning "Failed to install ant CLI automatically. Please install manually from https://github.com/maidsafe/antup"
        }
    }
    
    Log-Success "Autonomi CLI installation completed"
}

function Build-RustWorkspace {
    Log-Info "Building Rust workspace..."
    
    # Check if we're in the right directory
    if (!(Test-Path "Cargo.toml")) {
        Log-Error "Cargo.toml not found. Please run this script from the project root."
        exit 1
    }
    
    # Build all workspace members
    Log-Info "Building all workspace crates..."
    cargo build --workspace
    
    # Run clippy for code quality
    Log-Info "Running clippy for code quality checks..."
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    
    # Format code
    Log-Info "Formatting code..."
    cargo fmt --all
    
    Log-Success "Rust workspace built successfully"
}

function Setup-WebDependencies {
    Log-Info "Setting up web dependencies..."
    
    if (Test-Path "mutant-web") {
        Set-Location "mutant-web"
        
        # Install Node.js dependencies
        if (Test-Path "package.json") {
            Log-Info "Installing Node.js dependencies with pnpm..."
            pnpm install
        }
        
        # Build WASM module
        Log-Info "Building WASM module..."
        wasm-pack build --target web --out-dir pkg
        
        Set-Location ".."
        Log-Success "Web dependencies setup completed"
    } else {
        Log-Warning "mutant-web directory not found, skipping web setup"
    }
}

function Setup-DevEnvironment {
    Log-Info "Setting up development environment..."
    
    # Create necessary directories
    $configDir = "$env:USERPROFILE\.config\mutant"
    if (!(Test-Path $configDir)) {
        New-Item -ItemType Directory -Path $configDir -Force | Out-Null
    }
    
    Log-Success "Development environment setup completed"
}

function Run-Tests {
    Log-Info "Running tests to verify installation..."
    
    # Build tests first
    Log-Info "Building tests..."
    cargo test --workspace --no-run
    
    # Run unit tests (skip integration tests that require network)
    Log-Info "Running unit tests..."
    cargo test --workspace --lib
    
    Log-Success "Tests completed successfully"
}

function Main {
    Write-Host "==========================================" -ForegroundColor $Blue
    Write-Host "    MutAnt Project Setup Script (Windows)" -ForegroundColor $Blue
    Write-Host "==========================================" -ForegroundColor $Blue
    Write-Host ""
    
    if ($Help) {
        Show-Help
        return
    }
    
    # Check if running as administrator
    if (!(Test-Administrator)) {
        Log-Warning "This script should be run as Administrator for best results."
        Log-Info "Some installations may fail without administrator privileges."
        $continue = Read-Host "Continue anyway? (y/N)"
        if ($continue -ne "y" -and $continue -ne "Y") {
            Log-Info "Exiting. Please run as Administrator."
            return
        }
    }
    
    try {
        # Install Chocolatey first
        Install-Chocolatey
        
        # Install dependencies
        Install-SystemDependencies
        Install-Rust
        
        if (!$SkipWeb) {
            Install-NodeJS
        }
        
        if (!$SkipAnt) {
            Install-AntCLI
        }
        
        # Build project
        Build-RustWorkspace
        
        if (!$SkipWeb) {
            Setup-WebDependencies
        }
        
        # Setup development environment
        Setup-DevEnvironment
        
        # Run tests
        if (!$SkipTests) {
            Run-Tests
        }
        
        Write-Host ""
        Write-Host "==========================================" -ForegroundColor $Green
        Log-Success "Setup completed successfully!"
        Write-Host "==========================================" -ForegroundColor $Green
        Write-Host ""
        Write-Host "Next steps:"
        Write-Host "1. Restart your PowerShell/Command Prompt to refresh environment variables"
        Write-Host "2. Configure ant wallet: ant wallet create (or import existing)"
        Write-Host "3. Build and install CLI tools:"
        Write-Host "   cargo install --path mutant-cli"
        Write-Host "   cargo install --path mutant-daemon"
        Write-Host "4. Start using MutAnt: mutant --help"
        Write-Host ""
        Write-Host "For development:"
        Write-Host "- Build web interface: cd mutant-web && pnpm run build"
        Write-Host "- Run tests: cargo test --workspace"
        Write-Host ""
        
    } catch {
        Log-Error "Setup failed with error: $($_.Exception.Message)"
        Log-Error "Please check the error messages above and try again."
        exit 1
    }
}

# Run main function
Main
