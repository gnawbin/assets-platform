#!/usr/bin/env node

const { execSync, spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

// ANSI color codes for better output
const colors = {
  reset: '\x1b[0m',
  bright: '\x1b[1m',
  red: '\x1b[31m',
  green: '\x1b[32m',
  yellow: '\x1b[33m',
  blue: '\x1b[34m',
  magenta: '\x1b[35m',
  cyan: '\x1b[36m'
};

function log(message, color = 'reset') {
  console.log(`${colors[color]}${message}${colors.reset}`);
}

function logSuccess(message) {
  log(`✅ ${message}`, 'green');
}

function logError(message) {
  log(`❌ ${message}`, 'red');
}

function logWarning(message) {
  log(`⚠️  ${message}`, 'yellow');
}

function logInfo(message) {
  log(`ℹ️  ${message}`, 'blue');
}

function logStep(message) {
  log(`🔧 ${message}`, 'cyan');
}

// Check if a command exists
function commandExists(command) {
  try {
    execSync(`${command} --version`, { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

// Get platform-specific information
function getPlatformInfo() {
  const platform = os.platform();
  const arch = os.arch();
  
  return {
    platform,
    arch,
    isWindows: platform === 'win32',
    isMacOS: platform === 'darwin',
    isLinux: platform === 'linux'
  };
}

// Check Node.js version
function checkNodeVersion() {
  logStep('Checking Node.js version...');
  
  if (!commandExists('node')) {
    logError('Node.js is not installed!');
    logInfo('Please install Node.js from: https://nodejs.org/');
    return false;
  }
  
  try {
    const version = execSync('node --version', { encoding: 'utf8' }).trim();
    const majorVersion = parseInt(version.slice(1).split('.')[0]);
    
    if (majorVersion >= 18) {
      logSuccess(`Node.js ${version} is installed and compatible`);
      return true;
    } else {
      logError(`Node.js ${version} is too old. Required: >= 18.0.0`);
      logInfo('Please update Node.js from: https://nodejs.org/');
      return false;
    }
  } catch (error) {
    logError(`Failed to check Node.js version: ${error.message}`);
    return false;
  }
}

// Install Yarn if not present
function installYarn() {
  logStep('Checking Yarn installation...');
  
  if (commandExists('yarn')) {
    const version = execSync('yarn --version', { encoding: 'utf8' }).trim();
    logSuccess(`Yarn ${version} is already installed`);
    return true;
  }
  
  logWarning('Yarn is not installed. Installing Yarn...');
  
  try {
    execSync('npm install -g yarn', { stdio: 'inherit' });
    logSuccess('Yarn installed successfully');
    return true;
  } catch (error) {
    logError(`Failed to install Yarn: ${error.message}`);
    logInfo('Please install Yarn manually: npm install -g yarn');
    return false;
  }
}

// Install Rust
function installRust() {
  logStep('Checking Rust installation...');
  
  if (commandExists('rustc') && commandExists('cargo')) {
    const rustVersion = execSync('rustc --version', { encoding: 'utf8' }).trim();
    const cargoVersion = execSync('cargo --version', { encoding: 'utf8' }).trim();
    logSuccess(`Rust is already installed: ${rustVersion}`);
    logSuccess(`Cargo is already installed: ${cargoVersion}`);
    return true;
  }
  
  logWarning('Rust/Cargo is not installed. Installing Rust...');
  
  const platformInfo = getPlatformInfo();
  
  try {
    if (platformInfo.isWindows) {
      logInfo('Downloading Rust installer for Windows...');
      execSync('powershell -Command "Invoke-WebRequest -Uri https://win.rustup.rs -OutFile rustup-init.exe; .\\rustup-init.exe -y; Remove-Item rustup-init.exe"', { stdio: 'inherit' });
    } else {
      logInfo('Downloading and installing Rust...');
      execSync('curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y', { stdio: 'inherit' });
    }
    
    // Refresh environment variables
    if (platformInfo.isWindows) {
      process.env.PATH = `${os.homedir()}\\.cargo\\bin;${process.env.PATH}`;
    } else {
      process.env.PATH = `${os.homedir()}/.cargo/bin:${process.env.PATH}`;
    }
    
    logSuccess('Rust installed successfully');
    return true;
  } catch (error) {
    logError(`Failed to install Rust: ${error.message}`);
    logInfo('Please install Rust manually from: https://rustup.rs/');
    return false;
  }
}

// Install Tauri CLI
function installTauriCLI() {
  logStep('Checking Tauri CLI installation...');
  
  try {
    const version = execSync('cargo tauri --version', { encoding: 'utf8' }).trim();
    logSuccess(`Tauri CLI is already installed: ${version}`);
    return true;
  } catch {
    logWarning('Tauri CLI is not installed. Installing Tauri CLI...');
    
    try {
      execSync('cargo install tauri-cli --version "^2.0.0"', { stdio: 'inherit' });
      logSuccess('Tauri CLI installed successfully');
      return true;
    } catch (error) {
      logError(`Failed to install Tauri CLI: ${error.message}`);
      logInfo('Please install Tauri CLI manually: cargo install tauri-cli --version "^2.0.0"');
      return false;
    }
  }
}

// Check CMake (required by wasmtime-c-api-impl, pulled in by the tree-sitter `wasm` feature)
function checkCMake() {
  logStep('Checking CMake installation...');

  const minVersion = [3, 12];

  if (!commandExists('cmake')) {
    logError('CMake is not installed or not available on PATH!');
    logInfo('The Rust build needs it: `crates/ast` enables the tree-sitter `wasm` feature,');
    logInfo('which pulls in `wasmtime-c-api-impl` whose build script spawns `cmake`.');
    logInfo('Without CMake the build panics with: failed to spawn `cmake`: program not found');

    const platformInfo = getPlatformInfo();
    if (platformInfo.isWindows) {
      logInfo('Option 1: The "Desktop development with C++" workload in Visual Studio already bundles CMake, at:');
      logInfo('  C:\\Program Files\\Microsoft Visual Studio\\<version>\\<edition>\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin');
      logInfo('  Add that folder to your user PATH, then fully restart your terminal and VS Code.');
      logInfo('Option 2: winget install --id Kitware.CMake -e');
    } else if (platformInfo.isMacOS) {
      logInfo('Run: brew install cmake');
    } else {
      logInfo('Run: sudo apt-get install cmake (Ubuntu/Debian)');
      logInfo('Or:  sudo yum install cmake (CentOS/RHEL)');
    }
    return false;
  }

  try {
    const output = execSync('cmake --version', { encoding: 'utf8' });
    const match = output.match(/cmake version (\d+)\.(\d+)(?:\.(\d+))?/i);

    if (!match) {
      logWarning('CMake is installed but its version could not be parsed');
      return true;
    }

    const version = [parseInt(match[1], 10), parseInt(match[2], 10), parseInt(match[3] || '0', 10)];
    const versionText = version.join('.');
    const isSupported =
      version[0] > minVersion[0] ||
      (version[0] === minVersion[0] && version[1] >= minVersion[1]);

    if (!isSupported) {
      logError(`CMake ${versionText} is too old. Required: >= ${minVersion.join('.')}`);
      logInfo('Please upgrade CMake from: https://cmake.org/download/');
      return false;
    }

    logSuccess(`CMake ${versionText} is installed and compatible`);
    return true;
  } catch (error) {
    logError(`Failed to check CMake version: ${error.message}`);
    return false;
  }
}

// Install project dependencies
function installProjectDependencies() {
  logStep('Installing project dependencies...');
  
  try {
    execSync('yarn install', { stdio: 'inherit' });
    logSuccess('Project dependencies installed successfully');
    return true;
  } catch (error) {
    logError(`Failed to install project dependencies: ${error.message}`);
    return false;
  }
}

// Check system prerequisites
function checkSystemPrerequisites() {
  logStep('Checking system prerequisites...');
  
  const platformInfo = getPlatformInfo();
  
  if (platformInfo.isWindows) {
    logInfo('Windows detected. Ensure you have Visual Studio Build Tools installed.');
    logInfo('Download from: https://visualstudio.microsoft.com/visual-cpp-build-tools/');
  } else if (platformInfo.isMacOS) {
    logInfo('macOS detected. Ensure you have Xcode Command Line Tools installed.');
    logInfo('Run: xcode-select --install');
  } else if (platformInfo.isLinux) {
    logInfo('Linux detected. Ensure you have build-essential installed.');
    logInfo('Run: sudo apt-get install build-essential (Ubuntu/Debian)');
    logInfo('Or: sudo yum groupinstall "Development Tools" (CentOS/RHEL)');
  }
  
  return true;
}

// Main installation function
async function main() {
  log('🚀 Wallets Tool - Dependency Installation Script', 'bright');
  log('=' .repeat(50), 'cyan');
  
  const platformInfo = getPlatformInfo();
  logInfo(`Platform: ${platformInfo.platform} (${platformInfo.arch})`);
  
  let allSuccess = true;
  
  // Check system prerequisites
  checkSystemPrerequisites();
  
  // Check and install dependencies
  const steps = [
    { name: 'Node.js', fn: checkNodeVersion },
    { name: 'Yarn', fn: installYarn },
    { name: 'Rust', fn: installRust },
    { name: 'CMake', fn: checkCMake },
    { name: 'Tauri CLI', fn: installTauriCLI },
    { name: 'Project Dependencies', fn: installProjectDependencies }
  ];
  
  for (const step of steps) {
    log(`\n${'='.repeat(30)}`, 'cyan');
    const success = step.fn();
    if (!success) {
      allSuccess = false;
      logError(`Failed to install/verify ${step.name}`);
    }
  }
  
  log('\n' + '='.repeat(50), 'cyan');
  
  if (allSuccess) {
    logSuccess('🎉 All dependencies are installed and ready!');
    logInfo('You can now run: yarn tauri-dev');
    return 0;
  } else {
    logError('❌ Some dependencies failed to install.');
    logInfo('Please check the error messages above and install missing dependencies manually.');
    return 1;
  }
}

// Run the script
if (require.main === module) {
  main().then(process.exit).catch((error) => {
    logError(`Unexpected error: ${error.message}`);
    process.exit(1);
  });
}

module.exports = { main, checkNodeVersion, installYarn, installRust, checkCMake, installTauriCLI };