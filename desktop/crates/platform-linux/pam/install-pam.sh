#!/bin/bash
# install-pam.sh — Install WristKey PAM module on Linux

set -e

if [ "$EUID" -ne 0 ]; then
    echo "Please run as root (sudo)"
    exit 1
fi

echo "Building pam_wristkey.so..."
cd "$(dirname "$0")"
make clean
make

echo "Installing to /lib/security/..."
cp pam_wristkey.so /lib/security/

echo ""
echo "PAM module installed. Configure your display manager:"
echo ""
echo "  GNOME (GDM):   sudo nano /etc/pam.d/gdm-password"
echo "  KDE (SDDM):    sudo nano /etc/pam.d/sddm"
echo "  LightDM:       sudo nano /etc/pam.d/lightdm"
echo ""
echo "Add this line BEFORE other auth lines:"
echo "    auth sufficient pam_wristkey.so"
echo ""
echo "Example /etc/pam.d/gdm-password:"
echo "    auth sufficient pam_wristkey.so"
echo "    auth required   pam_unix.so"
echo ""
echo "The daemon will write ~/.wristkey/.last_auth after successful BLE unlock."
