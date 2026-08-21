# Installation and first run

This guide covers supported installation methods, upgrading, and the first-run
configuration flow. For everyday commands, continue with
[Views and display](views.md) or the [command reference](command-reference.md).

## Install with the script

```bash
curl -sSL https://raw.githubusercontent.com/chaqchase/lla/main/install.sh | bash
```

The script detects the operating system and architecture, downloads the matching
release binary, verifies it against the release checksum, and installs it in
`/usr/local/bin`.

On Windows PowerShell, use the native installer:

```powershell
irm https://raw.githubusercontent.com/chaqchase/lla/main/install.ps1 | iex
```

It detects AMD64 or ARM64, verifies `SHA256SUMS`, installs to
`%LOCALAPPDATA%\Programs\lla`, and adds that directory to the user `PATH`.
Download `install.ps1` first to use its `-Version`, `-InstallDir`, or
`-NoPathUpdate` options.

## Install with a package manager

| Platform | Command |
| --- | --- |
| Cargo | `cargo install lla` |
| macOS with Homebrew | `brew install lla` |
| Arch Linux with paru | `paru -S lla` |
| NetBSD with pkgin | `pkgin install lla` |
| X-CMD | `x install lla` |

## Install a release manually

Download the release asset that matches the operating system and architecture
from [GitHub Releases](https://github.com/chaqchase/lla/releases), then make it
executable and place it on `PATH`:

```bash
wget -c https://github.com/chaqchase/lla/releases/download/<version>/<asset> -O lla
chmod +x lla
sudo chown root:root lla
sudo mv lla /usr/local/bin/lla
```

Linux releases include GNU binaries for amd64, arm64, and i686. Static musl
binaries are available as `lla-linux-amd64-musl` and
`lla-linux-arm64-musl`; the install script selects them on musl systems. Static
musl builds support core listing, formatting, search, archives, themes,
configuration, and upgrades, but not dynamically loaded plugins. The 32-bit x86
`.apk` is a GNU-based legacy package.

NetBSD releases include `lla-netbsd-amd64`, which the install script selects on
NetBSD amd64. It is built natively with the default feature set: native dynamic
plugins are enabled, but the embedded WASM runtime is omitted because Wasmtime
does not support NetBSD. Official prebuilt plugin archives are not currently
published for NetBSD; native plugins can still be built and installed locally.

Windows releases include `lla-windows-amd64.exe` and
`lla-windows-arm64.exe`, built natively with `wasm-plugins`. Matching
`plugins-windows-amd64.zip` and `plugins-windows-arm64.zip` archives contain
native DLLs and WASM components. Windows reports file size and timestamps
normally, synthesizes Unix-style permission columns from file type and the
read-only attribute, and returns null or `-` for Unix-only ownership, inode,
hard-link, xattr, security-context, and mount metadata. Supported systems are
Windows 10 or Windows Server 2016 and newer on AMD64 or ARM64; 32-bit x86 is
rejected by the installer.

Creating symlinks may require Windows Developer Mode or an elevated terminal.
Existing directory and file symlinks can still be listed normally when the
current account can access them.

## Initialize lla

Run the guided setup after installation:

```bash
lla init
```

The wizard configures appearance, the default view, permission formatting,
listing and sorting defaults, dotfile and `.gitignore` behavior, long-view
columns, plugin directories and enablement, and recursion safety limits.

To write the built-in defaults without prompts, or inspect the active config:

```bash
lla init --default
lla config
```

The configuration file is stored at `~/.config/lla/config.toml`. See
[Configuration](configuration.md) for profiles, themes, shortcuts, and the
supported settings.

## Upgrade

```bash
lla upgrade
lla upgrade --version v0.5.1
lla upgrade --path /usr/local/bin/lla
```

`lla upgrade` uses the official installation logic to detect the platform,
verify `SHA256SUMS`, and atomically replace the executable. It targets the
currently running executable unless `--path` is provided. NetBSD amd64 upgrades
select the `lla-netbsd-amd64` release asset. Windows upgrades select the matching
`.exe` asset and use a Windows-safe self-replacement path.

## Next steps

- Choose a layout in [Views and display](views.md).
- Learn selection tools in [Filtering and search](filtering-and-search.md).
- Set up bookmarks and the interactive finder in [Navigation](navigation.md).
- Add capabilities from the [bundled plugin catalog](plugins/catalog.md).
