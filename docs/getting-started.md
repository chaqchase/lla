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
currently running executable unless `--path` is provided.

## Next steps

- Choose a layout in [Views and display](views.md).
- Learn selection tools in [Filtering and search](filtering-and-search.md).
- Set up bookmarks and the interactive finder in [Navigation](navigation.md).
- Add capabilities from the [bundled plugin catalog](plugins/catalog.md).
