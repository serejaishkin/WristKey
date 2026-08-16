#!/bin/bash
# install-pam-macos.sh — Install WristKey PAM module on macOS

set -e

if [ "$EUID" -eq 0 ]; then
    echo "Do NOT run this as root. Run as normal user, it will ask for sudo when needed."
    exit 1
fi

echo "Building pam_wristkey.dylib from Rust sources..."
cd "$(dirname "$0")/../../.."
cargo build --release -p wristkey-platform-macos

echo "Installing PAM module..."
sudo cp target/release/libwristkey_platform_macos.dylib /usr/local/lib/pam_wristkey.so
sudo chmod 644 /usr/local/lib/pam_wristkey.so

echo ""
echo "PAM module installed. Configure /etc/pam.d/authorization:"
echo ""
echo "  sudo nano /etc/pam.d/authorization"
echo ""
echo "Add this line:"
echo "    auth sufficient pam_wristkey.so"
echo ""
echo "The daemon writes ~/.wristkey/.last_auth after successful BLE unlock."
