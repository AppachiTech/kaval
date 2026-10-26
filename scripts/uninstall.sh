#!/bin/bash
# Removes a kav installed from a release tarball: the binary and its man page.
# (Installed with Homebrew? Use `brew uninstall kaval` instead.)
set -e

BIN_PATH="/usr/local/bin/kav"
MAN_PATH="/usr/local/share/man/man1/kav.1"

remove() {
    # On Intel Macs Homebrew links into /usr/local too; leave its files to brew.
    if [ -L "$1" ] && readlink "$1" | grep -q '/Cellar/'; then
        echo "$1 is managed by Homebrew; run: brew uninstall kaval"
    elif [ -e "$1" ]; then
        echo "Removing $1"
        if [ -w "$(dirname "$1")" ]; then
            rm -f "$1"
        else
            sudo rm -f "$1"
        fi
    else
        echo "$1 not found"
    fi
}

echo "Uninstalling Kaval..."
remove "$BIN_PATH"
remove "$MAN_PATH"

if other=$(command -v kav 2> /dev/null) && [ "$other" != "$BIN_PATH" ]; then
    echo "Another kav is still on your PATH at $other; remove it the way it was installed."
fi

echo "Uninstallation complete."
