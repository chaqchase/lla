//! Where things live on each platform, and what is safe to remove.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Apps,
    Developer,
    Caches,
    Logs,
    Downloads,
    Documents,
    Photos,
    Videos,
    Music,
    Trash,
    Other,
    System,
}

impl Category {
    pub const ALL: [Category; 12] = [
        Category::Apps,
        Category::Developer,
        Category::Caches,
        Category::Logs,
        Category::Downloads,
        Category::Documents,
        Category::Photos,
        Category::Videos,
        Category::Music,
        Category::Trash,
        Category::Other,
        Category::System,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Apps => "Apps",
            Category::Developer => "Developer",
            Category::Caches => "Caches",
            Category::Logs => "Logs",
            Category::Downloads => "Downloads",
            Category::Documents => "Documents",
            Category::Photos => "Photos",
            Category::Videos => "Videos",
            Category::Music => "Music",
            Category::Trash => "Trash",
            Category::Other => "Other",
            Category::System => "System & other",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Category::Apps => "apps",
            Category::Developer => "developer",
            Category::Caches => "caches",
            Category::Logs => "logs",
            Category::Downloads => "downloads",
            Category::Documents => "documents",
            Category::Photos => "photos",
            Category::Videos => "videos",
            Category::Music => "music",
            Category::Trash => "trash",
            Category::Other => "other",
            Category::System => "system",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Category::Apps => "Installed apps and the data they keep.",
            Category::Developer => "Build output, dependency folders, package caches and toolchains.",
            Category::Caches => "Data apps keep around to load faster. They rebuild it when needed.",
            Category::Logs => "Diagnostic logs written by apps and the system.",
            Category::Downloads => "Everything you have downloaded.",
            Category::Documents => "Documents, the desktop and cloud drive folders.",
            Category::Photos => "Pictures and photo libraries.",
            Category::Videos => "Movies, recordings and video projects.",
            Category::Music => "Music libraries and audio files.",
            Category::Trash => "Files already in the trash. Emptying it gives the space back.",
            Category::Other => "Everything else in the scanned folders.",
            Category::System => {
                "The operating system, swap, snapshots, other users and anything lla could not read."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Safety {
    /// Regenerated automatically or already thrown away.
    Safe,
    /// Probably fine to remove, but worth a look first.
    Review,
    /// Personal files. lla still lets you remove them, it just never suggests it.
    Keep,
}

impl Safety {
    pub fn label(self) -> &'static str {
        match self {
            Safety::Safe => "safe to clean",
            Safety::Review => "review first",
            Safety::Keep => "your files",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Safety::Safe => "safe",
            Safety::Review => "review",
            Safety::Keep => "keep",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The folder groups things; each child is listed on its own.
    Container,
    /// The folder is one thing and is listed as a whole.
    Item,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub path: PathBuf,
    pub category: Category,
    pub mode: Mode,
    pub safety: Safety,
    pub reason: &'static str,
    /// Contents are already in a trash, so cleaning deletes them for good.
    pub permanent: bool,
}

/// A build or dependency folder recognised by its name and what sits next to it.
pub struct Artifact {
    pub name: &'static str,
    /// At least one sibling must match. `*.ext` matches by extension. Empty means always.
    pub needs: &'static [&'static str],
    pub reason: &'static str,
    pub safety: Safety,
}

const fn artifact(
    name: &'static str,
    needs: &'static [&'static str],
    reason: &'static str,
    safety: Safety,
) -> Artifact {
    Artifact {
        name,
        needs,
        reason,
        safety,
    }
}

const GRADLE: &[&str] = &[
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
];
const DOTNET: &[&str] = &["*.csproj", "*.fsproj", "*.vbproj", "*.sln"];

pub const ARTIFACTS: &[Artifact] = &[
    artifact(
        "node_modules",
        &[],
        "reinstalled by your package manager",
        Safety::Safe,
    ),
    artifact(
        "target",
        &["Cargo.toml"],
        "rebuilt by cargo build",
        Safety::Safe,
    ),
    artifact(
        "__pycache__",
        &[],
        "Python bytecode, rebuilt on import",
        Safety::Safe,
    ),
    artifact(".pytest_cache", &[], "pytest cache", Safety::Safe),
    artifact(".mypy_cache", &[], "mypy cache", Safety::Safe),
    artifact(".ruff_cache", &[], "ruff cache", Safety::Safe),
    artifact(
        ".tox",
        &[],
        "tox environments, recreated on the next run",
        Safety::Safe,
    ),
    artifact(
        ".venv",
        &[],
        "Python virtualenv, recreate it from your requirements",
        Safety::Review,
    ),
    artifact(
        "venv",
        &["requirements.txt", "pyproject.toml", "setup.py", "Pipfile"],
        "Python virtualenv, recreate it from your requirements",
        Safety::Review,
    ),
    artifact(".next", &[], "Next.js build output", Safety::Safe),
    artifact(".nuxt", &[], "Nuxt build output", Safety::Safe),
    artifact(".svelte-kit", &[], "SvelteKit build output", Safety::Safe),
    artifact(".turbo", &[], "Turborepo cache", Safety::Safe),
    artifact(".parcel-cache", &[], "Parcel cache", Safety::Safe),
    artifact(".angular", &[], "Angular build cache", Safety::Safe),
    artifact(".gradle", GRADLE, "Gradle project cache", Safety::Safe),
    artifact("build", GRADLE, "rebuilt by gradle", Safety::Safe),
    artifact(
        "Pods",
        &["Podfile"],
        "reinstalled by pod install",
        Safety::Safe,
    ),
    artifact(
        ".dart_tool",
        &["pubspec.yaml"],
        "rebuilt by dart pub get",
        Safety::Safe,
    ),
    artifact(
        "build",
        &["pubspec.yaml"],
        "rebuilt by flutter build",
        Safety::Safe,
    ),
    artifact(".zig-cache", &[], "Zig build cache", Safety::Safe),
    artifact("zig-cache", &[], "Zig build cache", Safety::Safe),
    artifact(
        "zig-out",
        &["build.zig"],
        "rebuilt by zig build",
        Safety::Safe,
    ),
    artifact(
        ".terraform",
        &[],
        "reinstalled by terraform init",
        Safety::Safe,
    ),
    artifact(".stack-work", &[], "Stack build output", Safety::Safe),
    artifact("dist-newstyle", &[], "Cabal build output", Safety::Safe),
    artifact(
        "_build",
        &["mix.exs"],
        "rebuilt by mix compile",
        Safety::Safe,
    ),
    artifact(
        "deps",
        &["mix.exs"],
        "fetched again by mix deps.get",
        Safety::Safe,
    ),
    artifact("_build", &["dune-project"], "rebuilt by dune", Safety::Safe),
    artifact(
        ".build",
        &["Package.swift"],
        "rebuilt by swift build",
        Safety::Safe,
    ),
    artifact("bin", DOTNET, "rebuilt by dotnet build", Safety::Safe),
    artifact("obj", DOTNET, "rebuilt by dotnet build", Safety::Safe),
    artifact("cmake-build-debug", &[], "CMake build folder", Safety::Safe),
    artifact(
        "cmake-build-release",
        &[],
        "CMake build folder",
        Safety::Safe,
    ),
    artifact(
        "DerivedData",
        &[],
        "Xcode build data, rebuilt on the next build",
        Safety::Safe,
    ),
    artifact(
        "dist",
        &["package.json"],
        "build output, check it is not committed",
        Safety::Review,
    ),
];

/// Returns the 1-based index into [`ARTIFACTS`] for a folder, or 0.
pub fn artifact_tag(name: &OsStr, siblings: &[OsString]) -> u8 {
    let Some(name) = name.to_str() else {
        return 0;
    };
    for (index, artifact) in ARTIFACTS.iter().enumerate() {
        if artifact.name != name {
            continue;
        }
        let matches = artifact.needs.is_empty()
            || siblings.iter().any(|sibling| {
                let Some(sibling) = sibling.to_str() else {
                    return false;
                };
                artifact
                    .needs
                    .iter()
                    .any(|need| match need.strip_prefix('*') {
                        Some(ext) => sibling.ends_with(ext),
                        None => sibling == *need,
                    })
            });
        if matches {
            return (index + 1) as u8;
        }
    }
    0
}

pub fn artifact_info(tag: u8) -> Option<&'static Artifact> {
    ARTIFACTS.get(usize::from(tag).checked_sub(1)?)
}

/// Home-folder entries that must never be removed as a whole.
const PROTECTED_NAMES: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".config",
    ".local",
    ".password-store",
    "Library",
    "AppData",
    "Applications",
];

#[derive(Clone)]
pub struct Rules {
    pub home: Option<PathBuf>,
    pub rules: Vec<Rule>,
    by_path: HashMap<PathBuf, usize>,
    protected: HashSet<PathBuf>,
}

impl Rules {
    pub fn detect() -> Self {
        let home = dirs::home_dir();
        let mut rules = Vec::new();
        if let Some(home) = &home {
            home_rules(home, &mut rules);
        }
        system_rules(&mut rules);
        Self::new(home, rules)
    }

    pub fn new(home: Option<PathBuf>, rules: Vec<Rule>) -> Self {
        let mut by_path = HashMap::new();
        let mut protected = HashSet::new();
        protected.insert(PathBuf::from("/"));
        if let Some(home) = &home {
            protected.insert(home.clone());
            for name in PROTECTED_NAMES {
                protected.insert(home.join(name));
            }
            let mut parent = home.parent();
            while let Some(path) = parent {
                protected.insert(path.to_path_buf());
                parent = path.parent();
            }
        }
        for (index, rule) in rules.iter().enumerate() {
            by_path.entry(rule.path.clone()).or_insert(index);
            // Everything above a rule holds things the system expects, and so does a
            // container itself. A single item like DerivedData can go as a whole.
            let mut path = match rule.mode {
                Mode::Container => Some(rule.path.as_path()),
                Mode::Item => rule.path.parent(),
            };
            while let Some(current) = path {
                protected.insert(current.to_path_buf());
                path = current.parent();
            }
        }
        Self {
            home,
            rules,
            by_path,
            protected,
        }
    }

    pub fn exact(&self, path: &Path) -> Option<&Rule> {
        self.by_path.get(path).map(|index| &self.rules[*index])
    }

    /// The closest rule at or above `path`.
    pub fn enclosing(&self, path: &Path) -> Option<&Rule> {
        let mut current = Some(path);
        while let Some(candidate) = current {
            if let Some(rule) = self.exact(candidate) {
                return Some(rule);
            }
            current = candidate.parent();
        }
        None
    }

    pub fn is_protected(&self, path: &Path) -> bool {
        self.protected.contains(path) || path.parent().is_none()
    }

    /// Folders outside the home folder worth scanning by default.
    pub fn system_roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = self
            .rules
            .iter()
            .filter(|rule| {
                self.home
                    .as_ref()
                    .is_none_or(|home| !rule.path.starts_with(home))
            })
            .map(|rule| rule.path.clone())
            .filter(|path| path.is_dir())
            .collect();
        roots.sort();
        roots.dedup();
        // Drop roots nested in another root.
        let snapshot = roots.clone();
        roots.retain(|root| {
            !snapshot
                .iter()
                .any(|other| other != root && root.starts_with(other))
        });
        roots
    }
}

fn push(
    rules: &mut Vec<Rule>,
    path: PathBuf,
    category: Category,
    mode: Mode,
    safety: Safety,
    reason: &'static str,
) {
    rules.push(Rule {
        path,
        category,
        mode,
        safety,
        reason,
        permanent: false,
    });
}

fn home_rules(home: &Path, rules: &mut Vec<Rule>) {
    use Category::*;
    use Mode::*;
    use Safety::*;
    let h = |rel: &str| home.join(rel);

    // The home folder itself catches whatever nothing else claims.
    push(
        rules,
        home.to_path_buf(),
        Other,
        Container,
        Review,
        "in your home folder",
    );

    for (rel, category) in [
        ("Downloads", Downloads),
        ("Documents", Documents),
        ("Desktop", Documents),
        ("Music", Music),
    ] {
        let (safety, reason) = if category == Downloads {
            (Review, "downloaded file")
        } else {
            (Keep, "your files")
        };
        push(rules, h(rel), category, Container, safety, reason);
    }

    // Developer tooling that lives in the same place on every platform.
    for (rel, safety, reason) in [
        (
            ".cargo/registry",
            Safe,
            "cargo downloads crates again when needed",
        ),
        (".cargo/git", Safe, "cargo downloads git dependencies again"),
        (".rustup", Review, "Rust toolchains, reinstall with rustup"),
        (".npm", Safe, "npm cache"),
        (
            ".pnpm-store",
            Safe,
            "pnpm store, prune with pnpm store prune",
        ),
        (".yarn/berry/cache", Safe, "Yarn cache"),
        (".bun/install/cache", Safe, "Bun cache"),
        (".gradle/caches", Safe, "Gradle downloads these again"),
        (".gradle/wrapper", Safe, "Gradle wrapper distributions"),
        (".m2/repository", Safe, "Maven downloads these again"),
        (".ivy2/cache", Safe, "Ivy cache"),
        (".sbt", Review, "sbt boot files and caches"),
        (
            "go/pkg/mod",
            Safe,
            "Go module cache, same as go clean -modcache",
        ),
        (".nuget/packages", Safe, "NuGet restores these again"),
        (".pub-cache", Safe, "Dart and Flutter package cache"),
        (".android/avd", Review, "Android emulator images"),
        (".android/cache", Safe, "Android tooling cache"),
        (".docker", Review, "Docker configuration and buildx cache"),
        (
            ".local/share/containers",
            Review,
            "Podman images and containers",
        ),
        (".ollama/models", Review, "downloaded models"),
        (
            ".cache/huggingface",
            Review,
            "downloaded models and datasets",
        ),
        (".conda", Review, "conda environments and packages"),
        ("miniconda3", Review, "conda installation"),
        ("anaconda3", Review, "conda installation"),
        (".pyenv", Review, "Python versions installed by pyenv"),
        (".nvm", Review, "Node versions installed by nvm"),
        (".deno", Review, "Deno cache and installs"),
        (".stack", Review, "Stack snapshots and GHC installs"),
        (".ghcup", Review, "GHC toolchains"),
        (".cabal/packages", Safe, "Cabal package cache"),
        (".vscode/extensions", Review, "VS Code extensions"),
        (".cursor/extensions", Review, "Cursor extensions"),
        (".expo", Safe, "Expo cache"),
        (".gem", Review, "Ruby gems"),
        (".bundle/cache", Safe, "Bundler cache"),
        (".swiftpm", Safe, "Swift package cache"),
        (".platformio", Review, "PlatformIO toolchains"),
        (
            ".colima",
            Review,
            "Colima VM disks, docker system prune frees space inside them",
        ),
        (".lima", Review, "Lima VM disks"),
    ] {
        push(rules, h(rel), Developer, Item, safety, reason);
    }

    #[cfg(target_os = "macos")]
    {
        push(rules, h("Applications"), Apps, Container, Review, "app");
        for (rel, reason) in [
            (
                "Library/Application Support",
                "app data, the app may need it",
            ),
            ("Library/Containers", "sandboxed app data"),
            ("Library/Group Containers", "data shared between apps"),
        ] {
            push(rules, h(rel), Apps, Container, Review, reason);
        }
        push(
            rules,
            h("Library/Caches"),
            Caches,
            Container,
            Safe,
            "apps rebuild their caches",
        );
        push(rules, h("Library/Logs"), Logs, Container, Safe, "log files");
        push(rules, h("Movies"), Videos, Container, Keep, "your files");
        push(rules, h("Pictures"), Photos, Container, Keep, "your files");
        push(
            rules,
            h("Library/Mobile Documents"),
            Documents,
            Container,
            Keep,
            "in iCloud Drive",
        );
        push(
            rules,
            h("Library/CloudStorage"),
            Documents,
            Container,
            Keep,
            "cloud drive",
        );
        for (rel, safety, reason) in [
            (
                "Library/Developer/Xcode/DerivedData",
                Safe,
                "Xcode build data, rebuilt on the next build",
            ),
            (
                "Library/Developer/Xcode/Archives",
                Review,
                "app archives you built",
            ),
            (
                "Library/Developer/Xcode/iOS DeviceSupport",
                Safe,
                "Xcode downloads it again when a device connects",
            ),
            (
                "Library/Developer/Xcode/watchOS DeviceSupport",
                Safe,
                "Xcode downloads it again when a device connects",
            ),
            (
                "Library/Developer/Xcode/tvOS DeviceSupport",
                Safe,
                "Xcode downloads it again when a device connects",
            ),
            (
                "Library/Developer/Xcode/visionOS DeviceSupport",
                Safe,
                "Xcode downloads it again when a device connects",
            ),
            (
                "Library/Developer/Xcode/macOS DeviceSupport",
                Safe,
                "Xcode downloads it again when a device connects",
            ),
            (
                "Library/Developer/Xcode/UserData/Previews",
                Safe,
                "SwiftUI preview data, rebuilt when you open a preview",
            ),
            (
                "Library/Developer/CoreSimulator/Devices",
                Review,
                "simulators, xcrun simctl delete unavailable removes old ones",
            ),
            (
                "Library/Developer/CoreSimulator/Caches",
                Safe,
                "simulator caches, rebuilt when needed",
            ),
            ("Library/Android/sdk", Review, "Android SDK"),
            (
                "Library/Containers/com.docker.docker",
                Review,
                "Docker Desktop disk image, docker system prune frees space inside it",
            ),
            (
                "Library/Group Containers/HUAQ24HBR6.dev.orbstack",
                Review,
                "OrbStack disk image, docker system prune frees space inside it",
            ),
            (
                "Library/pnpm/store",
                Safe,
                "pnpm store, prune with pnpm store prune",
            ),
            (
                "Library/Caches/Homebrew",
                Safe,
                "Homebrew downloads, same as brew cleanup",
            ),
        ] {
            push(rules, h(rel), Developer, Item, safety, reason);
        }
        push(
            rules,
            h("Library/Developer/CoreSimulator"),
            Developer,
            Container,
            Review,
            "simulator data",
        );
        push(
            rules,
            h("Library/Application Support/MobileSync/Backup"),
            Apps,
            Container,
            Review,
            IOS_BACKUP,
        );
        rules.push(Rule {
            path: h(".Trash"),
            category: Trash,
            mode: Container,
            safety: Safe,
            reason: "already in the Trash",
            permanent: true,
        });
    }

    #[cfg(windows)]
    {
        for (rel, reason) in [
            ("AppData/Local/Programs", "app"),
            ("AppData/Local", "app data, the app may need it"),
            ("AppData/Roaming", "app data, the app may need it"),
            ("AppData/LocalLow", "app data, the app may need it"),
        ] {
            push(rules, h(rel), Apps, Container, Review, reason);
        }
        push(
            rules,
            h("AppData/Local/Temp"),
            Caches,
            Container,
            Safe,
            "temporary file",
        );
        push(
            rules,
            h("AppData/Local/Microsoft/Windows/INetCache"),
            Caches,
            Item,
            Safe,
            "browser cache",
        );
        push(
            rules,
            h("AppData/Local/CrashDumps"),
            Logs,
            Container,
            Safe,
            "crash dump",
        );
        push(rules, h("Videos"), Videos, Container, Keep, "your files");
        push(rules, h("Pictures"), Photos, Container, Keep, "your files");
        push(
            rules,
            h("OneDrive"),
            Documents,
            Container,
            Keep,
            "in OneDrive",
        );
        for (rel, safety, reason) in [
            ("AppData/Local/npm-cache", Safe, "npm cache"),
            ("AppData/Roaming/npm-cache", Safe, "npm cache"),
            ("AppData/Local/pnpm", Safe, "pnpm store"),
            ("AppData/Local/Yarn/Cache", Safe, "Yarn cache"),
            ("AppData/Local/pip/Cache", Safe, "pip cache"),
            ("AppData/Local/go-build", Safe, "Go build cache"),
            ("AppData/Local/Android/Sdk", Review, "Android SDK"),
            (
                "AppData/Local/Docker",
                Review,
                "Docker Desktop disk image, docker system prune frees space inside it",
            ),
            ("AppData/Local/NuGet", Safe, "NuGet cache"),
            (
                "AppData/Local/uv/cache",
                Safe,
                "uv cache, same as uv cache clean",
            ),
        ] {
            push(rules, h(rel), Developer, Item, safety, reason);
        }
        for rel in [
            "AppData/Roaming/Apple Computer/MobileSync/Backup",
            // iTunes from the Microsoft Store keeps them here.
            "Apple/MobileSync/Backup",
        ] {
            push(rules, h(rel), Apps, Container, Review, IOS_BACKUP);
        }
    }

    // Command-line tools use ~/.cache on macOS too (uv, pre-commit, Hugging Face).
    #[cfg(unix)]
    push(
        rules,
        h(".cache"),
        Caches,
        Container,
        Safe,
        "apps rebuild their caches",
    );

    // Homebrew's download cache, wherever HOMEBREW_CACHE points.
    #[cfg(unix)]
    if let Some(cache) = std::env::var_os("HOMEBREW_CACHE").filter(|value| !value.is_empty()) {
        push(
            rules,
            PathBuf::from(cache),
            Developer,
            Item,
            Safe,
            "Homebrew downloads, same as brew cleanup",
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        push(
            rules,
            h(".local/share/flatpak"),
            Apps,
            Container,
            Review,
            "Flatpak app",
        );
        push(
            rules,
            h(".var/app"),
            Apps,
            Container,
            Review,
            "Flatpak app data",
        );
        push(rules, h("snap"), Apps, Container, Review, "Snap app data");
        push(
            rules,
            h(".local/share/Steam"),
            Apps,
            Item,
            Review,
            "Steam and its games",
        );
        push(rules, h(".mozilla"), Apps, Item, Review, "Firefox profiles");
        push(rules, h("Videos"), Videos, Container, Keep, "your files");
        push(rules, h("Pictures"), Photos, Container, Keep, "your files");
        push(
            rules,
            h("Android/Sdk"),
            Developer,
            Item,
            Review,
            "Android SDK",
        );
        push(
            rules,
            h(".cache/Homebrew"),
            Developer,
            Item,
            Safe,
            "Homebrew downloads, same as brew cleanup",
        );
        push(
            rules,
            h(".local/share/pnpm/store"),
            Developer,
            Item,
            Safe,
            "pnpm store, prune with pnpm store prune",
        );
        rules.push(Rule {
            path: h(".local/share/Trash/files"),
            category: Trash,
            mode: Container,
            safety: Safe,
            reason: "already in the trash",
            permanent: true,
        });
    }
}

fn system_rules(rules: &mut Vec<Rule>) {
    use Category::*;
    use Mode::*;
    use Safety::*;

    #[cfg(target_os = "macos")]
    push(
        rules,
        PathBuf::from("/Applications"),
        Apps,
        Container,
        Review,
        "app",
    );

    #[cfg(windows)]
    for bin in windows::recycle_bins() {
        rules.push(Rule {
            path: bin,
            category: Trash,
            mode: Container,
            safety: Safe,
            reason: "already in the Recycle Bin",
            permanent: true,
        });
    }

    #[cfg(windows)]
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            push(
                rules,
                PathBuf::from(dir),
                Apps,
                Container,
                Review,
                "installed program",
            );
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        push(
            rules,
            PathBuf::from("/opt"),
            Apps,
            Container,
            Review,
            "installed program",
        );
        push(
            rules,
            PathBuf::from("/var/lib/flatpak"),
            Apps,
            Item,
            Review,
            "Flatpak apps and runtimes",
        );
        push(
            rules,
            PathBuf::from("/var/lib/snapd/snaps"),
            Apps,
            Container,
            Review,
            "Snap package",
        );
        push(
            rules,
            PathBuf::from("/var/cache"),
            Caches,
            Container,
            Safe,
            "system cache",
        );
        push(
            rules,
            PathBuf::from("/var/log"),
            Logs,
            Container,
            Review,
            "system log",
        );
        push(
            rules,
            PathBuf::from("/var/lib/docker"),
            Developer,
            Item,
            Review,
            "Docker images and volumes, docker system prune frees space",
        );
    }

    let _ = (Other, Keep, Item);
}

#[cfg_attr(all(unix, not(target_os = "macos")), allow(dead_code))]
const IOS_BACKUP: &str = "device backup, delete old ones from Finder or iTunes";

/// A friendlier name for items whose own name says nothing, like iPhone
/// backups (a long hex id) or Recycle Bin entries (`$R1A2B3C.txt`).
pub fn item_label(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    let parent_name = parent.file_name()?.to_str()?;
    let grandparent = parent.parent()?.file_name()?.to_str()?;
    if parent_name == "Backup" && grandparent == "MobileSync" {
        return backup_device_name(path);
    }
    if grandparent.eq_ignore_ascii_case("$Recycle.Bin") {
        return recycled_name(path);
    }
    None
}

/// Reads "Device Name" from a backup's Info.plist, which Finder and iTunes
/// write as XML.
fn backup_device_name(backup: &Path) -> Option<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(backup.join("Info.plist"))
        .ok()?
        .take(256 * 1024)
        .read_to_string(&mut text)
        .ok()?;
    let after = &text[text.find("<key>Device Name</key>")?..];
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")?;
    let name = after[start..start + end].trim();
    (!name.is_empty()).then(|| format!("{name} backup"))
}

/// Windows keeps the original path of each `$R…` entry in the matching `$I…`
/// file next to it.
fn recycled_name(entry: &Path) -> Option<String> {
    let name = entry.file_name()?.to_str()?;
    let rest = name.strip_prefix("$R")?;
    let info = std::fs::read(entry.with_file_name(format!("$I{rest}"))).ok()?;
    let original = parse_recycle_info(&info)?;
    let file = original.rsplit(['\\', '/']).next().unwrap_or(&original);
    (!file.is_empty()).then(|| file.to_string())
}

/// The original path from a `$I` file: version 1 (Vista to 8) stores it in a
/// fixed 520-byte field, version 2 (Windows 10 and later) after its length.
fn parse_recycle_info(info: &[u8]) -> Option<String> {
    let version = u64::from_le_bytes(info.get(0..8)?.try_into().ok()?);
    let utf16 = match version {
        1 => info.get(24..24 + 520)?,
        2 => {
            let chars = u32::from_le_bytes(info.get(24..28)?.try_into().ok()?) as usize;
            info.get(28..28 + chars.checked_mul(2)?)?
        }
        _ => return None,
    };
    let units: Vec<u16> = utf16
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    let text = String::from_utf16_lossy(&units);
    (!text.is_empty()).then_some(text)
}

#[cfg(windows)]
mod windows {
    use std::path::PathBuf;

    /// `X:\$Recycle.Bin\<SID>` for the current user on every fixed drive.
    pub fn recycle_bins() -> Vec<PathBuf> {
        let drives = fixed_drives();
        match current_user_sid() {
            Some(sid) => drives
                .into_iter()
                .map(|drive| drive.join("$Recycle.Bin").join(&sid))
                .filter(|path| path.is_dir())
                .collect(),
            // Without the SID, other users' bins are the ones we cannot read.
            None => drives
                .into_iter()
                .filter_map(|drive| std::fs::read_dir(drive.join("$Recycle.Bin")).ok())
                .flat_map(|entries| entries.flatten())
                .map(|entry| entry.path())
                .filter(|path| path.is_dir() && std::fs::read_dir(path).is_ok())
                .collect(),
        }
    }

    fn fixed_drives() -> Vec<PathBuf> {
        use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
        const DRIVE_FIXED: u32 = 3;
        // SAFETY: no arguments.
        let mask = unsafe { GetLogicalDrives() };
        (0..26u8)
            .filter(|bit| mask & (1 << bit) != 0)
            .map(|bit| char::from(b'A' + bit))
            .filter(|letter| {
                let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain([0]).collect();
                // SAFETY: `root` is a NUL-terminated wide string that outlives the call.
                unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_FIXED }
            })
            .map(|letter| PathBuf::from(format!("{letter}:\\")))
            .collect()
    }

    fn current_user_sid() -> Option<String> {
        use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
        use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
        use windows_sys::Win32::Security::{
            GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        // SAFETY: the token is closed before returning, the buffer is sized by
        // the first call and 8-byte aligned, and the string that
        // ConvertSidToStringSidW allocates is released with LocalFree.
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let mut needed = 0u32;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
            let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
            let ok = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            );
            CloseHandle(token);
            if ok == 0 {
                return None;
            }
            let user = &*(buffer.as_ptr() as *const TOKEN_USER);
            let mut text: *mut u16 = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 || text.is_null() {
                return None;
            }
            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            Some(sid)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(rules: &Rules, path: &Path) -> Option<(Category, Safety)> {
        rules
            .enclosing(path)
            .map(|rule| (rule.category, rule.safety))
    }

    #[test]
    fn package_caches_and_tool_data_are_classified() {
        let home = PathBuf::from("/home/me");
        let mut list = Vec::new();
        home_rules(&home, &mut list);
        let rules = Rules::new(Some(home.clone()), list);
        let dev_safe = Some((Category::Developer, Safety::Safe));
        for rel in [
            ".npm/_cacache/content-v2/sha512/ab",
            ".pnpm-store/v3/files",
            ".yarn/berry/cache/react.zip",
            ".cargo/registry/cache",
            ".gradle/caches/modules-2",
            ".m2/repository/org",
            "go/pkg/mod/github.com",
            ".bun/install/cache/react",
        ] {
            assert_eq!(classify(&rules, &home.join(rel)), dev_safe, "{rel}");
        }
        assert_eq!(
            classify(&rules, &home.join(".colima/default")),
            Some((Category::Developer, Safety::Review))
        );
        #[cfg(unix)]
        assert_eq!(
            classify(&rules, &home.join(".cache/uv/archive-v0")),
            Some((Category::Caches, Safety::Safe))
        );
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(
            classify(&rules, &home.join(".cache/Homebrew/downloads")),
            dev_safe
        );
        #[cfg(target_os = "macos")]
        {
            for (rel, category, safety) in [
                (
                    "Library/Developer/Xcode/tvOS DeviceSupport/17.0",
                    Category::Developer,
                    Safety::Safe,
                ),
                (
                    "Library/Developer/CoreSimulator/Devices/ABC",
                    Category::Developer,
                    Safety::Review,
                ),
                (
                    "Library/Developer/CoreSimulator/Caches/dyld",
                    Category::Developer,
                    Safety::Safe,
                ),
                (
                    "Library/Application Support/MobileSync/Backup/00008030-001",
                    Category::Apps,
                    Safety::Review,
                ),
                (
                    "Library/Caches/Homebrew/downloads",
                    Category::Developer,
                    Safety::Safe,
                ),
                (
                    "Library/Group Containers/HUAQ24HBR6.dev.orbstack/data",
                    Category::Developer,
                    Safety::Review,
                ),
            ] {
                assert_eq!(
                    classify(&rules, &home.join(rel)),
                    Some((category, safety)),
                    "{rel}"
                );
            }
            // Each backup is its own item, so only the folder holding them is kept.
            let backups = home.join("Library/Application Support/MobileSync/Backup");
            assert!(rules.is_protected(&backups));
            assert!(!rules.is_protected(&backups.join("00008030-001")));
        }
    }

    #[test]
    fn labels_backups_and_recycled_files() {
        let root = tempfile::TempDir::new().unwrap();
        let backup = root.path().join("MobileSync/Backup/00008030-001A");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(
            backup.join("Info.plist"),
            "<plist><dict><key>Build Version</key><string>21A</string>\n\
             <key>Device Name</key>\n\t<string>Sam's iPhone</string></dict></plist>",
        )
        .unwrap();
        assert_eq!(item_label(&backup).as_deref(), Some("Sam's iPhone backup"));

        let bin = root.path().join("$Recycle.Bin/S-1-5-21-1");
        std::fs::create_dir_all(&bin).unwrap();
        let original = r"C:\Users\me\Videos\trip.mp4";
        let mut info = 2u64.to_le_bytes().to_vec();
        info.extend(5_000_000u64.to_le_bytes());
        info.extend(0u64.to_le_bytes());
        let units: Vec<u16> = original.encode_utf16().chain([0]).collect();
        info.extend((units.len() as u32).to_le_bytes());
        info.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
        std::fs::write(bin.join("$IAB12CD.mp4"), &info).unwrap();
        std::fs::write(bin.join("$RAB12CD.mp4"), "x").unwrap();
        assert_eq!(
            item_label(&bin.join("$RAB12CD.mp4")).as_deref(),
            Some("trip.mp4")
        );
        assert_eq!(item_label(&root.path().join("plain/file")), None);
    }

    fn names(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn artifacts_need_their_project_file() {
        let rust = names(&["Cargo.toml", "src", "target"]);
        let plain = names(&["notes", "target"]);
        assert_ne!(artifact_tag(OsStr::new("target"), &rust), 0);
        assert_eq!(artifact_tag(OsStr::new("target"), &plain), 0);
        assert_ne!(artifact_tag(OsStr::new("node_modules"), &plain), 0);
        let dotnet = names(&["App.csproj", "bin"]);
        assert_ne!(artifact_tag(OsStr::new("bin"), &dotnet), 0);
        assert_eq!(artifact_tag(OsStr::new("bin"), &plain), 0);
    }

    #[test]
    fn rule_folders_and_their_parents_are_protected() {
        let home = PathBuf::from("/home/me");
        let mut rules = Vec::new();
        home_rules(&home, &mut rules);
        let rules = Rules::new(Some(home.clone()), rules);
        assert!(rules.is_protected(&home));
        assert!(rules.is_protected(&home.join(".ssh")));
        assert!(rules.is_protected(&home.join(".cargo")));
        assert!(rules.is_protected(&home.join("Downloads")));
        assert!(!rules.is_protected(&home.join("Downloads/big.iso")));
        assert!(!rules.is_protected(&home.join("code/app/node_modules")));
        assert!(!rules.is_protected(&home.join(".rustup")));
        assert_eq!(
            rules
                .enclosing(&home.join("Downloads/x/y"))
                .map(|r| r.category),
            Some(Category::Downloads)
        );
    }
}
