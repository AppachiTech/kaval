#!/bin/bash
set -e

INSTALL_PATH="/usr/local/bin"
BIN_NAME="kav"

echo "Uninstalling Kaval..."

if [ -f "$INSTALL_PATH/$BIN_NAME" ]; then
    echo "Removing $INSTALL_PATH/$BIN_NAME"
    sudo rm -f "$INSTALL_PATH/$BIN_NAME"
else
    echo "$BIN_NAME not found in $INSTALL_PATH"
fi

echo "Uninstallation complete."
