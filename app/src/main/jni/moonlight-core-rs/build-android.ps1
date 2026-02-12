# Build script for moonlight-core-rs (Windows PowerShell)
# This script builds the Rust library for Android arm64-v8a

$ErrorActionPreference = "Stop"

# Check if cargo-ndk is installed
$cargoNdk = Get-Command cargo-ndk -ErrorAction SilentlyContinue
if (-not $cargoNdk) {
    Write-Host "Installing cargo-ndk..."
    cargo install cargo-ndk
}

# Check if ninja is installed
$ninja = Get-Command ninja -ErrorAction SilentlyContinue
if (-not $ninja) {
    Write-Error "Error: ninja is required but not installed."
    Write-Host "Install it with:"
    Write-Host "  scoop install ninja"
    Write-Host "  or: choco install ninja"
    exit 1
}

# Add Android targets if not already added
Write-Host "Adding Android targets..."
rustup target add aarch64-linux-android

# Check NDK path
if (-not $env:ANDROID_NDK_HOME) {
    Write-Host "Warning: ANDROID_NDK_HOME not set, trying to find NDK..."
    $localProps = Get-Content -Path "..\..\..\..\..\..\local.properties" -ErrorAction SilentlyContinue
    if ($localProps) {
        $ndkLine = $localProps | Where-Object { $_ -match "ndk.dir" }
        if ($ndkLine) {
            $env:ANDROID_NDK_HOME = $ndkLine -replace "ndk.dir=", "" -replace "\\\\", "\"
        }
    }

    if (-not $env:ANDROID_NDK_HOME) {
        $sdkPath = "$env:LOCALAPPDATA\Android\Sdk\ndk"
        if (Test-Path $sdkPath) {
            $ndkDirs = Get-ChildItem -Path $sdkPath -Directory | Sort-Object Name -Descending
            if ($ndkDirs) {
                $env:ANDROID_NDK_HOME = $ndkDirs[0].FullName
            }
        }
    }
}

Write-Host "Using NDK: $env:ANDROID_NDK_HOME"

if (-not $env:ANDROID_NDK_HOME -or -not (Test-Path $env:ANDROID_NDK_HOME)) {
    Write-Error "Android NDK not found. Please set ANDROID_NDK_HOME environment variable."
    exit 1
}

# Set up cross-compilation environment variables
$env:TARGET_CC = "$env:ANDROID_NDK_HOME\toolchains\llvm\prebuilt\windows-x86_64\bin\aarch64-linux-android34-clang.cmd"
$env:TARGET_AR = "$env:ANDROID_NDK_HOME\toolchains\llvm\prebuilt\windows-x86_64\bin\llvm-ar.exe"
$env:CC_aarch64_linux_android = $env:TARGET_CC
$env:AR_aarch64_linux_android = $env:TARGET_AR

# Force opus-sys to build from source (disable pkg-config for cross-compilation)
$env:OPUS_NO_PKG = "1"
$env:OPUS_STATIC = "1"

# Set up CMake environment for audiopus_sys cross-compilation
$env:CMAKE_TOOLCHAIN_FILE = "$env:ANDROID_NDK_HOME\build\cmake\android.toolchain.cmake"
$env:ANDROID_ABI = "arm64-v8a"
$env:ANDROID_PLATFORM = "android-34"
$env:CMAKE_GENERATOR = "Ninja"
$env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"

# Create output directory
$outDir = ".\jniLibs"
if (-not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Path $outDir | Out-Null
}

# Build for arm64-v8a only
Write-Host "`nBuilding for arm64-v8a..."
cargo ndk -t arm64-v8a -o $outDir build --release


Write-Host "`nBuild complete!"
Write-Host "Libraries are in $outDir\"

