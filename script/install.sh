#!/usr/bin/env sh
set -eu

# Downloads a tarball from https://zed.dev/releases and unpacks it
# into ~/.local/. If you'd prefer to do this manually, instructions are at
# https://zed.dev/docs/linux.

main() {
    platform="$(uname -s)"
    arch="$(uname -m)"
    channel="${NEOZED_CHANNEL:-stable}"
    NEOZED_VERSION="${NEOZED_VERSION:-latest}"
    # Use TMPDIR if available (for environments with non-standard temp directories)
    if [ -n "${TMPDIR:-}" ] && [ -d "${TMPDIR}" ]; then
        temp="$(mktemp -d "$TMPDIR/neozed-XXXXXX")"
    else
        temp="$(mktemp -d "/tmp/neozed-XXXXXX")"
    fi

    if [ "$platform" = "Darwin" ]; then
        platform="macos"
    elif [ "$platform" = "Linux" ]; then
        platform="linux"
    else
        echo "Unsupported platform $platform"
        exit 1
    fi

    case "$platform-$arch" in
        macos-arm64* | linux-arm64* | linux-armhf | linux-aarch64)
            arch="aarch64"
            ;;
        macos-x86* | linux-x86* | linux-i686*)
            arch="x86_64"
            ;;
        *)
            echo "Unsupported platform or architecture"
            exit 1
            ;;
    esac

    if command -v curl >/dev/null 2>&1; then
        curl () {
            command curl -fL "$@"
        }
    elif command -v wget >/dev/null 2>&1; then
        curl () {
            wget -O- "$@"
        }
    else
        echo "Could not find 'curl' or 'wget' in your path"
        exit 1
    fi

    "$platform" "$@"

    if [ "$(command -v neozed)" = "$HOME/.local/bin/neozed" ]; then
        echo "Neo Zed has been installed. Run with 'neozed'"
    else
        echo "To run Neo Zed from your terminal, you must add ~/.local/bin to your PATH"
        echo "Run:"

        case "$SHELL" in
            *zsh)
                echo "   echo 'export PATH=\$HOME/.local/bin:\$PATH' >> ~/.zshrc"
                echo "   source ~/.zshrc"
                ;;
            *fish)
                echo "   fish_add_path -U $HOME/.local/bin"
                ;;
            *)
                echo "   echo 'export PATH=\$HOME/.local/bin:\$PATH' >> ~/.bashrc"
                echo "   source ~/.bashrc"
                ;;
        esac

        echo "To run Neo Zed now, '~/.local/bin/neozed'"
    fi
}

linux() {
    if [ -n "${ZED_BUNDLE_PATH:-}" ]; then
        cp "$ZED_BUNDLE_PATH" "$temp/neozed-linux-$arch.tar.gz"
    else
        echo "Downloading Neo Zed version: $NEOZED_VERSION"
        curl "https://cloud.zed.dev/releases/$channel/$NEOZED_VERSION/download?asset=zed&arch=$arch&os=linux&source=install.sh" > "$temp/neozed-linux-$arch.tar.gz"
    fi

    suffix=""
    if [ "$channel" != "stable" ]; then
        suffix="-$channel"
    fi

    appid=""
    case "$channel" in
      stable)
        appid="dev.neozed.NeoZed"
        ;;
      nightly)
        appid="dev.neozed.NeoZed-Nightly"
        ;;
      preview)
        appid="dev.neozed.NeoZed-Preview"
        ;;
      dev)
        appid="dev.neozed.NeoZed-Dev"
        ;;
      *)
        echo "Unknown release channel: ${channel}. Using stable app ID."
        appid="dev.neozed.NeoZed"
        ;;
    esac

    # Unpack
    rm -rf "$HOME/.local/neozed$suffix.app"
    mkdir -p "$HOME/.local/neozed$suffix.app"
    tar -xzf "$temp/neozed-linux-$arch.tar.gz" -C "$HOME/.local/"

    # Setup ~/.local directories
    mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications"

    # Link the binary
    if [ -f "$HOME/.local/neozed$suffix.app/bin/neozed" ]; then
        ln -sf "$HOME/.local/neozed$suffix.app/bin/neozed" "$HOME/.local/bin/neozed"
    elif [ -f "$HOME/.local/neozed$suffix.app/bin/zed" ]; then
        ln -sf "$HOME/.local/neozed$suffix.app/bin/zed" "$HOME/.local/bin/neozed"
    else
        # support for versions before 0.139.x.
        ln -sf "$HOME/.local/neozed$suffix.app/bin/cli" "$HOME/.local/bin/neozed"
    fi

    # Copy .desktop file
    desktop_file_path="$HOME/.local/share/applications/${appid}.desktop"
    src_dir="$HOME/.local/neozed$suffix.app/share/applications"
    if [ -f "$src_dir/${appid}.desktop" ]; then
        cp "$src_dir/${appid}.desktop" "${desktop_file_path}"
    else
        # Fallback for older tarballs
        cp "$src_dir/neozed$suffix.desktop" "${desktop_file_path}"
    fi
    sed -i "s|Icon=neozed|Icon=$HOME/.local/neozed$suffix.app/share/icons/hicolor/512x512/apps/neozed.png|g" "${desktop_file_path}"
    sed -i "s|Exec=neozed|Exec=$HOME/.local/neozed$suffix.app/bin/neozed|g" "${desktop_file_path}"
}

macos() {
    echo "Downloading Neo Zed version: $NEOZED_VERSION"
    curl "https://cloud.zed.dev/releases/$channel/$NEOZED_VERSION/download?asset=zed&os=macos&arch=$arch&source=install.sh" > "$temp/NeoZed-$arch.dmg"
    hdiutil attach -quiet "$temp/NeoZed-$arch.dmg" -mountpoint "$temp/mount"
    app="$(cd "$temp/mount/"; echo *.app)"
    echo "Installing $app"
    if [ -d "/Applications/$app" ]; then
        echo "Removing existing $app"
        rm -rf "/Applications/$app"
    fi
    ditto "$temp/mount/$app" "/Applications/$app"
    hdiutil detach -quiet "$temp/mount"

    mkdir -p "$HOME/.local/bin"
    # Link the binary
    ln -sf "/Applications/$app/Contents/MacOS/cli" "$HOME/.local/bin/neozed"
}

main "$@"
