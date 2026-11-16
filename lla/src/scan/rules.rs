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
                "Library/Developer/CoreSimulator",
                Review,
                "simulators, xcrun simctl delete unavailable removes old ones",
            ),
            ("Library/Android/sdk", Review, "Android SDK"),
            (
                "Library/Containers/com.docker.docker",
                Review,
                "Docker disk image, docker system prune frees space inside it",
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
            ("AppData/Local/Docker", Review, "Docker data"),
            ("AppData/Local/NuGet", Safe, "NuGet cache"),
        ] {
            push(rules, h(rel), Developer, Item, safety, reason);
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        push(
            rules,
            h(".cache"),
            Caches,
            Container,
            Safe,
            "apps rebuild their caches",
        );
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

#[cfg(test)]
mod tests {
    use super::*;

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
