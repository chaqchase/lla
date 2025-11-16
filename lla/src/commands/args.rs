use crate::config::{Config, ShortcutCommand};
use crate::error::{LlaError, Result};
use crate::filter::{parse_size_range, parse_time_range, NumericRange, TimeRange};
use clap::{App, Arg, ArgGroup, ArgMatches, SubCommand};
use clap_complete::Shell;
use std::path::PathBuf;
use std::time::SystemTime;

pub struct Args {
    pub directory: String,
    pub depth: Option<usize>,
    pub long_format: bool,
    pub tree_format: bool,
    pub table_format: bool,
    pub grid_format: bool,
    pub grid_ignore: bool,
    pub sizemap_format: bool,
    pub timeline_format: bool,
    pub git_format: bool,
    pub fuzzy_format: bool,
    pub recursive_format: bool,
    pub show_icons: bool,
    pub no_color: bool,
    pub sort_by: String,
    pub sort_reverse: bool,
    pub sort_dirs_first: bool,
    pub sort_case_sensitive: bool,
    pub sort_natural: bool,
    pub filter: Option<String>,
    pub presets: Vec<String>,
    pub size_filter: Option<NumericRange>,
    pub size_filter_raw: Option<String>,
    pub modified_filter: Option<TimeRange>,
    pub modified_filter_raw: Option<String>,
    pub created_filter: Option<TimeRange>,
    pub created_filter_raw: Option<String>,
    pub case_sensitive: bool,
    pub refine_filters: Vec<String>,
    pub enable_plugin: Vec<String>,
    pub disable_plugin: Vec<String>,
    pub plugins_dir: PathBuf,
    pub include_dirs: bool,
    pub dirs_only: bool,
    pub files_only: bool,
    pub symlinks_only: bool,
    pub no_dirs: bool,
    pub no_files: bool,
    pub no_symlinks: bool,
    pub no_dotfiles: bool,
    pub almost_all: bool,
    pub dotfiles_only: bool,
    pub permission_format: String,
    pub hide_group: bool,
    pub relative_dates: bool,
    pub output_mode: OutputMode,
    pub command: Option<Command>,
    pub search: Option<String>,
    pub search_context: usize,
    pub search_pipelines: Vec<SearchPipelineSpec>,
}

#[derive(Clone, Debug)]
pub struct SearchPipelineSpec {
    pub plugin: String,
    pub action: String,
    pub args: Vec<String>,
}

pub enum Command {
    Install(InstallSource),
    ListPlugins,
    Use,
    Diff(DiffCommand),
    InitConfig { defaults_only: bool },
    Config(Option<ConfigAction>),
    PluginAction(String, String, Vec<String>),
    Update(Option<String>),
    Clean,
    Shortcut(ShortcutAction),
    Jump(JumpAction),
    GenerateCompletion(Shell, Option<String>, Option<String>),
    Theme,
    ThemePull,
    ThemeInstall(String),
    ThemePreview(String),
}

pub enum InstallSource {
    Prebuilt,
    GitHub(String),
    LocalDir(String),
}

pub enum ShortcutAction {
    Add(String, ShortcutCommand),
    Remove(String),
    List,
    Create,
    Export(Option<String>),
    Import(String, bool),
    Run(String, Vec<String>),
}

#[derive(Clone)]
pub struct DiffCommand {
    pub left: String,
    pub target: DiffTarget,
}

#[derive(Clone)]
pub enum DiffTarget {
    Directory(String),
    Git { reference: String },
}

#[derive(Clone, Debug)]
pub enum JumpAction {
    Prompt,
    Add(String),
    Remove(String),
    List,
    ClearHistory,
    Setup(Option<String>),
}

#[derive(Clone)]
pub enum ConfigAction {
    View,
    Set(String, String),
    ShowEffective,
    DiffDefault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputMode {
    Human,
    Json { pretty: bool },
    Ndjson,
    Csv,
}

impl Args {
    fn build_cli(config: &Config) -> App<'_> {
        App::new(env!("CARGO_PKG_NAME"))
            .version(env!("CARGO_PKG_VERSION"))
            .author(env!("CARGO_PKG_AUTHORS"))
            .about(env!("CARGO_PKG_DESCRIPTION"))
            .arg(
                Arg::with_name("directory")
                    .help("The directory to list")
                    .index(1)
                    .default_value("."),
            )
            .subcommand(
                SubCommand::with_name("diff")
                    .about("Compare two directories or a directory against a git reference")
                    .arg(
                        Arg::with_name("left")
                            .help("Base directory to compare")
                            .required(true)
                            .index(1),
                    )
                    .arg(
                        Arg::with_name("right")
                            .help("Directory to compare against")
                            .index(2),
                    )
                    .arg(
                        Arg::with_name("git")
                            .long("git")
                            .help("Compare the directory against a git reference instead of another directory"),
                    )
                    .arg(
                        Arg::with_name("git-ref")
                            .long("git-ref")
                            .takes_value(true)
                            .requires("git")
                            .help("Git reference to compare against (default: HEAD)"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("jump")
                    .about("Jump to a bookmarked or recent directory")
                    .arg(
                        Arg::with_name("add")
                            .long("add")
                            .takes_value(true)
                            .help("Add a directory to bookmarks"),
                    )
                    .arg(
                        Arg::with_name("remove")
                            .long("remove")
                            .takes_value(true)
                            .help("Remove a directory from bookmarks"),
                    )
                    .arg(
                        Arg::with_name("list")
                            .long("list")
                            .help("List bookmarks and history"),
                    )
                    .arg(
                        Arg::with_name("clear-history")
                            .long("clear-history")
                            .help("Clear directory history"),
                    )
                    .arg(
                        Arg::with_name("setup")
                            .long("setup")
                            .help("Setup shell integration for seamless directory jumping"),
                    )
                    .arg(
                        Arg::with_name("shell")
                            .long("shell")
                            .takes_value(true)
                            .possible_values(&["bash", "zsh", "fish"]) 
                            .help("Override shell detection for setup (bash|zsh|fish)"),
                    ),
            )
            .arg(
                Arg::with_name("json")
                    .long("json")
                    .help("Output a single JSON array"),
            )
            .arg(
                Arg::with_name("ndjson")
                    .long("ndjson")
                    .help("Output newline-delimited JSON (one object per line)"),
            )
            .arg(
                Arg::with_name("csv")
                    .long("csv")
                    .help("Output CSV with header row"),
            )
            .arg(
                Arg::with_name("pretty")
                    .long("pretty")
                    .help("Pretty print JSON (only applies to --json)"),
            )
            .group(
                ArgGroup::new("machine_output")
                    .args(&["json", "ndjson", "csv"]) // mutually exclusive
                    .multiple(false),
            )
            .arg(
                Arg::with_name("search")
                    .long("search")
                    .takes_value(true)
                    .help("Search file contents with ripgrep for the given pattern"),
            )
            .arg(
                Arg::with_name("search-context")
                    .long("search-context")
                    .takes_value(true)
                    .help("Number of context lines to show before and after matches (default: 2)"),
            )
            .arg(
                Arg::with_name("depth")
                    .short('d')
                    .long("depth")
                    .takes_value(true)
                    .help("Set the depth for tree listing (default from config)"),
            )
            .arg(
                Arg::with_name("long")
                    .short('l')
                    .long("long")
                    .help("Use long listing format (overrides config format)"),
            )
            .arg(
                Arg::with_name("tree")
                    .short('t')
                    .long("tree")
                    .help("Use tree listing format (overrides config format)"),
            )
            .arg(
                Arg::with_name("table")
                    .short('T')
                    .long("table")
                    .help("Use table listing format (overrides config format)"),
            )
            .arg(
                Arg::with_name("grid")
                    .short('g')
                    .long("grid")
                    .help("Use grid listing format (overrides config format)"),
            )
            .arg(
                Arg::with_name("grid-ignore")
                    .long("grid-ignore")
                    .help("Use grid view ignoring terminal width (Warning: output may extend beyond screen width)"),
            )
            .arg(
                Arg::with_name("sizemap")
                    .short('S')
                    .long("sizemap")
                    .help("Show visual representation of file sizes (overrides config format)"),
            )
            .arg(
                Arg::with_name("timeline")
                    .long("timeline")
                    .help("Group files by time periods (overrides config format)"),
            )
            .arg(
                Arg::with_name("git")
                    .short('G')
                    .long("git")
                    .help("Show git status and information (overrides config format)"),
            )
            .arg(
                Arg::with_name("fuzzy")
                    .short('F')
                    .long("fuzzy")
                    .help("Use interactive fuzzy finder"),
            )
            .arg(
                Arg::with_name("icons")
                    .long("icons")
                    .help("Show icons for files and directories (overrides config setting)"),
            )
            .arg(
                Arg::with_name("no-icons")
                    .long("no-icons")
                    .help("Hide icons for files and directories (overrides config setting)"),
            )
            .arg(
                Arg::with_name("no-color")
                    .long("no-color")
                    .help("Disable all colors in the output"),
            )
            .arg(
                Arg::with_name("sort")
                    .short('s')
                    .long("sort")
                    .help("Sort files by name, size, or date")
                    .takes_value(true)
                    .possible_values(["name", "size", "date"])
                    .default_value(&config.default_sort),
            )
            .arg(
                Arg::with_name("sort-reverse")
                    .short('r')
                    .long("sort-reverse")
                    .help("Reverse the sort order"),
            )
            .arg(
                Arg::with_name("sort-dirs-first")
                    .long("sort-dirs-first")
                    .help("List directories before files (overrides config setting)"),
            )
            .arg(
                Arg::with_name("sort-case-sensitive")
                    .long("sort-case-sensitive")
                    .help("Enable case-sensitive sorting (overrides config setting)"),
            )
            .arg(
                Arg::with_name("sort-natural")
                    .long("sort-natural")
                    .help("Use natural sorting for numbers (overrides config setting)"),
            )
            .arg(
                Arg::with_name("filter")
                    .short('f')
                    .long("filter")
                    .takes_value(true)
                    .help("Filter files by name or extension"),
            )
            .arg(
                Arg::with_name("preset")
                    .long("preset")
                    .takes_value(true)
                    .multiple(true)
                    .help("Apply a named filter preset defined in your config"),
            )
            .arg(
                Arg::with_name("size")
                    .long("size")
                    .takes_value(true)
                    .help("Filter by file size (e.g., '>10M', '5K..2G')"),
            )
            .arg(
                Arg::with_name("modified")
                    .long("modified")
                    .takes_value(true)
                    .help("Filter by modified time (e.g., '<7d', '2023-01-01..2023-12-31')"),
            )
            .arg(
                Arg::with_name("created")
                    .long("created")
                    .takes_value(true)
                    .help("Filter by creation time using the same syntax as --modified"),
            )
            .arg(
                Arg::with_name("case-sensitive")
                    .short('c')
                    .long("case-sensitive")
                    .help("Enable case-sensitive filtering (overrides config setting)"),
            )
            .arg(
                Arg::with_name("refine")
                    .long("refine")
                    .takes_value(true)
                    .multiple(true)
                    .help("Refine a previous listing (or cache) without re-walking the filesystem using additional filters"),
            )
            .arg(
                Arg::with_name("enable-plugin")
                    .long("enable-plugin")
                    .takes_value(true)
                    .multiple(true)
                    .help("Enable specific plugins"),
            )
            .arg(
                Arg::with_name("search-pipe")
                    .long("search-pipe")
                    .takes_value(true)
                    .multiple(true)
                    .help("After --search finishes, run plugin action(s) on matching files (syntax: plugin:action[:arg...])"),
            )
            .arg(
                Arg::with_name("disable-plugin")
                    .long("disable-plugin")
                    .takes_value(true)
                    .multiple(true)
                    .help("Disable specific plugins"),
            )
            .arg(
                Arg::with_name("plugins-dir")
                    .long("plugins-dir")
                    .takes_value(true)
                    .help("Specify the plugins directory"),
            )
            .arg(
                Arg::with_name("recursive")
                    .short('R')
                    .long("recursive")
                    .help("Use recursive listing format"),
            )
            .arg(
                Arg::with_name("include-dirs")
                    .long("include-dirs")
                    .help("Include directory sizes in the metadata"),
            )
            .arg(
                Arg::with_name("dirs-only")
                    .long("dirs-only")
                    .help("Show only directories"),
            )
            .arg(
                Arg::with_name("files-only")
                    .long("files-only")
                    .help("Show only regular files"),
            )
            .arg(
                Arg::with_name("symlinks-only")
                    .long("symlinks-only")
                    .help("Show only symbolic links"),
            )
            .arg(
                Arg::with_name("no-dirs")
                    .long("no-dirs")
                    .help("Hide directories"),
            )
            .arg(
                Arg::with_name("no-files")
                    .long("no-files")
                    .help("Hide regular files"),
            )
            .arg(
                Arg::with_name("no-symlinks")
                    .long("no-symlinks")
                    .help("Hide symbolic links"),
            )
            .arg(
                Arg::with_name("no-dotfiles")
                    .long("no-dotfiles")
                    .help("Hide files starting with a dot (overrides config setting)"),
            )
            .arg(
                Arg::with_name("all")
                    .short('a')
                    .long("all")
                    .help("Show all files including dotfiles (overrides no_dotfiles config)"),
            )
            .arg(
                Arg::with_name("almost-all")
                    .short('A')
                    .long("almost-all")
                    .help("Show all files including dotfiles except . and .. (overrides no_dotfiles config)"),
            )
            .arg(
                Arg::with_name("dotfiles-only")
                    .long("dotfiles-only")
                    .help("Show only dot files and directories (those starting with a dot)"),
            )
            .arg(
                Arg::with_name("permission-format")
                    .long("permission-format")
                    .help("Format for displaying permissions (symbolic, octal, binary, verbose, compact)")
                    .takes_value(true)
                    .possible_values(&["symbolic", "octal", "binary",  "verbose", "compact"])
                    .default_value(&config.permission_format),
            )
            .arg(
                Arg::with_name("hide-group")
                    .long("hide-group")
                    .help("Hide group column in long format"),
            )
            .arg(
                Arg::with_name("relative-dates")
                    .long("relative-dates")
                    .help("Show relative dates (e.g., '2h ago') in long format"),
            )
            .subcommand(
                SubCommand::with_name("install")
                    .about("Install a plugin")
                    .arg(
                        Arg::with_name("prebuilt")
                            .long("prebuilt")
                            .help("Install plugins from the latest prebuilt release (default)"),
                    )
                    .arg(
                        Arg::with_name("git")
                            .long("git")
                            .takes_value(true)
                            .help("Install a plugin from a GitHub repository URL"),
                    )
                    .arg(
                        Arg::with_name("dir")
                            .long("dir")
                            .takes_value(true)
                            .help("Install a plugin from a local directory"),
                    )
                    .group(
                        ArgGroup::with_name("install-source")
                            .args(&["prebuilt", "git", "dir"])
                            .multiple(false),
                    ),
            )
            .subcommand(
                SubCommand::with_name("plugin")
                    .about("Run a plugin action")
                    .arg(
                        Arg::with_name("plugin_name")
                            .help("Name of the plugin")
                            .index(1),
                    )
                    .arg(
                        Arg::with_name("plugin_action")
                            .help("Action to perform")
                            .index(2),
                    )
                    .arg(
                        Arg::with_name("plugin_args")
                            .help("Arguments for the plugin action")
                            .index(3)
                            .multiple(true),
                    )
                    .arg(
                        Arg::with_name("name")
                            .long("name")
                            .short('n')
                            .takes_value(true)
                            .help("Name of the plugin (alternative to positional)"),
                    )
                    .arg(
                        Arg::with_name("action")
                            .long("action")
                            .short('a')
                            .takes_value(true)
                            .help("Action to perform (alternative to positional)"),
                    )
                    .arg(
                        Arg::with_name("args")
                            .long("args")
                            .short('r')
                            .takes_value(true)
                            .multiple(true)
                            .help("Arguments for the plugin action (alternative to positional)"),
                    ),
            )
            .subcommand(SubCommand::with_name("list-plugins").about("List all available plugins"))
            .subcommand(SubCommand::with_name("use").about("Interactive plugin manager"))
            .subcommand(
                SubCommand::with_name("init")
                    .about("Initialize the configuration file")
                    .arg(
                    Arg::with_name("default")
                        .long("default")
                        .help("Write the default config without launching the wizard"),
                    ),
            )
            .subcommand(
                SubCommand::with_name("config")
                    .about("View or modify configuration")
                    .arg(
                        Arg::with_name("set")
                            .long("set")
                            .takes_value(true)
                            .number_of_values(2)
                            .value_names(&["KEY", "VALUE"])
                            .help("Set a configuration value (e.g., --set plugins_dir /new/path)"),
                    )
                    .subcommand(
                        SubCommand::with_name("show-effective")
                            .about("Show the merged config (global + nearest .lla.toml)"),
                    )
                    .subcommand(
                        SubCommand::with_name("diff")
                            .about("Compare config overrides against defaults")
                            .arg(
                                Arg::with_name("default")
                                    .long("default")
                                    .help("Diff against the built-in defaults")
                                    .required(true),
                            ),
                    ),
            )
            .subcommand(
                SubCommand::with_name("update")
                    .about("Update installed plugins")
                    .arg(
                        Arg::with_name("name")
                            .help("Name of the plugin to update (updates all if not specified)")
                            .index(1),
                    ),
            )
            .subcommand(
                SubCommand::with_name("clean").about("This command will clean up invalid plugins"),
            )
            .subcommand(
                SubCommand::with_name("shortcut")
                    .about("Manage command shortcuts")
                    .subcommand(
                        SubCommand::with_name("add")
                            .about("Add a new shortcut")
                            .arg(
                                Arg::with_name("name")
                                    .help("Name of the shortcut")
                                    .required(true)
                                    .index(1),
                            )
                            .arg(
                                Arg::with_name("plugin")
                                    .help("Plugin name")
                                    .required(true)
                                    .index(2),
                            )
                            .arg(
                                Arg::with_name("action")
                                    .help("Plugin action")
                                    .required(true)
                                    .index(3),
                            )
                            .arg(
                                Arg::with_name("description")
                                    .help("Optional description of the shortcut")
                                    .long("description")
                                    .short('d')
                                    .takes_value(true),
                            ),
                    )
                    .subcommand(
                        SubCommand::with_name("create")
                            .about("Interactively create a new shortcut"),
                    )
                    .subcommand(
                        SubCommand::with_name("remove")
                            .about("Remove a shortcut")
                            .arg(
                                Arg::with_name("name")
                                    .help("Name of the shortcut to remove")
                                    .required(true)
                                    .index(1),
                            ),
                    )
                    .subcommand(
                        SubCommand::with_name("export")
                            .about("Export shortcuts to a file")
                            .arg(
                                Arg::with_name("output")
                                    .help("Output file path (defaults to stdout)")
                                    .index(1),
                            ),
                    )
                    .subcommand(
                        SubCommand::with_name("import")
                            .about("Import shortcuts from a file")
                            .arg(
                                Arg::with_name("file")
                                    .help("File path to import from")
                                    .required(true)
                                    .index(1),
                            )
                            .arg(
                                Arg::with_name("merge")
                                    .long("merge")
                                    .help("Merge with existing shortcuts (skip conflicts)"),
                            ),
                    )
                    .subcommand(SubCommand::with_name("list").about("List all shortcuts")),
            )
            .subcommand(
                SubCommand::with_name("completion")
                    .about("Generate shell completion scripts")
                    .arg(
                        Arg::with_name("shell")
                            .help("Target shell")
                            .required(true)
                            .possible_values(["bash", "fish", "zsh", "powershell", "elvish"])
                            .index(1),
                    )
                    .arg(
                        Arg::with_name("path")
                            .long("path")
                            .short('p')
                            .help("Custom installation path for the completion script")
                            .takes_value(true),
                    )
                    .arg(
                        Arg::with_name("output")
                            .long("output")
                            .short('o')
                            .help("Output path for the completion script (prints to stdout if not specified)")
                            .takes_value(true),
                    ),
            )
            .subcommand(
                SubCommand::with_name("theme")
                    .about("Interactive theme manager")
                    .subcommand(
                        SubCommand::with_name("pull")
                            .about("Pull and install themes from the official repository")
                    )
                    .subcommand(
                        SubCommand::with_name("install")
                            .about("Install theme(s) from a file or directory")
                            .arg(
                                Arg::with_name("path")
                                    .help("Path to theme file or directory containing themes")
                                    .required(true)
                                    .index(1),
                            )
                    )
                    .subcommand(
                        SubCommand::with_name("preview")
                            .about("Preview a theme using sample output")
                            .arg(
                                Arg::with_name("name")
                                    .help("Name of the theme to preview")
                                    .required(true)
                                    .index(1),
                            ),
                    ),
            )
    }

    pub fn parse(config: &Config) -> Result<Self> {
        let args: Vec<String> = std::env::args().collect();
        if args.len() > 1 {
            let potential_shortcut = &args[1];
            if config.get_shortcut(potential_shortcut).is_some() {
                return Ok(Self {
                    directory: ".".to_string(),
                    depth: config.default_depth,
                    long_format: config.default_format == "long",
                    tree_format: config.default_format == "tree",
                    table_format: config.default_format == "table",
                    grid_format: config.default_format == "grid",
                    grid_ignore: false,
                    sizemap_format: config.default_format == "sizemap",
                    timeline_format: config.default_format == "timeline",
                    git_format: config.default_format == "git",
                    fuzzy_format: false,
                    recursive_format: false,
                    show_icons: config.show_icons,
                    no_color: false,
                    sort_by: config.default_sort.clone(),
                    sort_reverse: false,
                    sort_dirs_first: config.sort.dirs_first,
                    sort_case_sensitive: config.sort.case_sensitive,
                    sort_natural: config.sort.natural,
                    filter: None,
                    presets: Vec::new(),
                    size_filter: None,
                    size_filter_raw: None,
                    modified_filter: None,
                    modified_filter_raw: None,
                    created_filter: None,
                    created_filter_raw: None,
                    case_sensitive: config.filter.case_sensitive,
                    refine_filters: Vec::new(),
                    enable_plugin: Vec::new(),
                    disable_plugin: Vec::new(),
                    plugins_dir: config.plugins_dir.clone(),
                    include_dirs: false,
                    dirs_only: false,
                    files_only: false,
                    symlinks_only: false,
                    no_dirs: false,
                    no_files: false,
                    no_symlinks: false,
                    no_dotfiles: config.filter.no_dotfiles,
                    almost_all: false,
                    dotfiles_only: false,
                    permission_format: config.permission_format.clone(),
                    hide_group: config.formatters.long.hide_group,
                    relative_dates: config.formatters.long.relative_dates,
                    output_mode: OutputMode::Human,
                    command: Some(Command::Shortcut(ShortcutAction::Run(
                        potential_shortcut.clone(),
                        args[2..].to_vec(),
                    ))),
                    search: None,
                    search_context: 2,
                    search_pipelines: Vec::new(),
                });
            }
        }

        let matches = Self::build_cli(config).get_matches();
        Self::from_matches(&matches, config)
    }

    pub fn get_cli(config: &Config) -> App<'_> {
        Self::build_cli(config)
    }

    fn from_matches(matches: &ArgMatches, config: &Config) -> Result<Self> {
        let command = if let Some(completion_matches) = matches.subcommand_matches("completion") {
            let shell = match completion_matches.value_of("shell").unwrap() {
                "bash" => Shell::Bash,
                "fish" => Shell::Fish,
                "zsh" => Shell::Zsh,
                "powershell" => Shell::PowerShell,
                "elvish" => Shell::Elvish,
                _ => unreachable!(),
            };
            Some(Command::GenerateCompletion(
                shell,
                completion_matches.value_of("path").map(String::from),
                completion_matches.value_of("output").map(String::from),
            ))
        } else if let Some(theme_matches) = matches.subcommand_matches("theme") {
            if theme_matches.subcommand_matches("pull").is_some() {
                Some(Command::ThemePull)
            } else if let Some(install_matches) = theme_matches.subcommand_matches("install") {
                Some(Command::ThemeInstall(
                    install_matches.value_of("path").unwrap().to_string(),
                ))
            } else if let Some(preview_matches) = theme_matches.subcommand_matches("preview") {
                Some(Command::ThemePreview(
                    preview_matches.value_of("name").unwrap().to_string(),
                ))
            } else {
                Some(Command::Theme)
            }
        } else if let Some(matches) = matches.subcommand_matches("shortcut") {
            if let Some(add_matches) = matches.subcommand_matches("add") {
                Some(Command::Shortcut(ShortcutAction::Add(
                    add_matches.value_of("name").unwrap().to_string(),
                    ShortcutCommand {
                        plugin_name: add_matches.value_of("plugin").unwrap().to_string(),
                        action: add_matches.value_of("action").unwrap().to_string(),
                        description: add_matches.value_of("description").map(String::from),
                    },
                )))
            } else if matches.subcommand_matches("create").is_some() {
                Some(Command::Shortcut(ShortcutAction::Create))
            } else if let Some(remove_matches) = matches.subcommand_matches("remove") {
                Some(Command::Shortcut(ShortcutAction::Remove(
                    remove_matches.value_of("name").unwrap().to_string(),
                )))
            } else if let Some(export_matches) = matches.subcommand_matches("export") {
                Some(Command::Shortcut(ShortcutAction::Export(
                    export_matches.value_of("output").map(String::from),
                )))
            } else if let Some(import_matches) = matches.subcommand_matches("import") {
                Some(Command::Shortcut(ShortcutAction::Import(
                    import_matches.value_of("file").unwrap().to_string(),
                    import_matches.is_present("merge"),
                )))
            } else if matches.subcommand_matches("list").is_some() {
                Some(Command::Shortcut(ShortcutAction::List))
            } else {
                None
            }
        } else if matches.subcommand_matches("clean").is_some() {
            Some(Command::Clean)
        } else if let Some(diff_matches) = matches.subcommand_matches("diff") {
            let left = diff_matches
                .value_of("left")
                .expect("left argument is required")
                .to_string();
            if diff_matches.is_present("git") {
                if diff_matches.value_of("right").is_some() {
                    return Err(LlaError::Other(
                        "Cannot specify a second directory when using --git".into(),
                    ));
                }
                let reference = diff_matches
                    .value_of("git-ref")
                    .unwrap_or("HEAD")
                    .to_string();
                Some(Command::Diff(DiffCommand {
                    left,
                    target: DiffTarget::Git { reference },
                }))
            } else {
                let right = diff_matches.value_of("right").ok_or_else(|| {
                    LlaError::Other(
                        "Provide a second directory (e.g. `lla diff src ../backup/src`) or pass --git to compare against git state".into(),
                    )
                })?;
                Some(Command::Diff(DiffCommand {
                    left,
                    target: DiffTarget::Directory(right.to_string()),
                }))
            }
        } else if let Some(install_matches) = matches.subcommand_matches("install") {
            if install_matches.is_present("prebuilt") {
                Some(Command::Install(InstallSource::Prebuilt))
            } else if let Some(github_url) = install_matches.value_of("git") {
                Some(Command::Install(InstallSource::GitHub(
                    github_url.to_string(),
                )))
            } else if let Some(local_dir) = install_matches.value_of("dir") {
                Some(Command::Install(InstallSource::LocalDir(
                    local_dir.to_string(),
                )))
            } else {
                Some(Command::Install(InstallSource::Prebuilt))
            }
        } else if matches.subcommand_matches("list-plugins").is_some() {
            Some(Command::ListPlugins)
        } else if matches.subcommand_matches("use").is_some() {
            Some(Command::Use)
        } else if let Some(init_matches) = matches.subcommand_matches("init") {
            Some(Command::InitConfig {
                defaults_only: init_matches.is_present("default"),
            })
        } else if let Some(config_matches) = matches.subcommand_matches("config") {
            if let Some(values) = config_matches.values_of("set") {
                let values: Vec<_> = values.collect();
                Some(Command::Config(Some(ConfigAction::Set(
                    values[0].to_string(),
                    values[1].to_string(),
                ))))
            } else if config_matches
                .subcommand_matches("show-effective")
                .is_some()
            {
                Some(Command::Config(Some(ConfigAction::ShowEffective)))
            } else if config_matches.subcommand_matches("diff").is_some() {
                Some(Command::Config(Some(ConfigAction::DiffDefault)))
            } else {
                Some(Command::Config(Some(ConfigAction::View)))
            }
        } else if let Some(plugin_matches) = matches.subcommand_matches("plugin") {
            // Support both positional and flag-based syntax
            let plugin_name = plugin_matches
                .value_of("plugin_name")
                .or_else(|| plugin_matches.value_of("name"))
                .expect("Plugin name is required (use 'lla plugin <name> <action>' or 'lla plugin --name <name> --action <action>')")
                .to_string();

            let action = plugin_matches
                .value_of("plugin_action")
                .or_else(|| plugin_matches.value_of("action"))
                .expect("Plugin action is required (use 'lla plugin <name> <action>' or 'lla plugin --name <name> --action <action>')")
                .to_string();

            let args = plugin_matches
                .values_of("plugin_args")
                .or_else(|| plugin_matches.values_of("args"))
                .map(|v| v.map(String::from).collect())
                .unwrap_or_default();

            Some(Command::PluginAction(plugin_name, action, args))
        } else if let Some(jump_matches) = matches.subcommand_matches("jump") {
            let action = if let Some(path) = jump_matches.value_of("add") {
                JumpAction::Add(path.to_string())
            } else if let Some(path) = jump_matches.value_of("remove") {
                JumpAction::Remove(path.to_string())
            } else if jump_matches.is_present("list") {
                JumpAction::List
            } else if jump_matches.is_present("clear-history") {
                JumpAction::ClearHistory
            } else if jump_matches.is_present("setup") {
                JumpAction::Setup(jump_matches.value_of("shell").map(|s| s.to_string()))
            } else {
                JumpAction::Prompt
            };
            Some(Command::Jump(action))
        } else {
            matches.subcommand_matches("update").map(|update_matches| {
                Command::Update(update_matches.value_of("name").map(String::from))
            })
        };

        let has_format_flag = matches.is_present("long")
            || matches.is_present("tree")
            || matches.is_present("table")
            || matches.is_present("grid")
            || matches.is_present("sizemap")
            || matches.is_present("timeline")
            || matches.is_present("git")
            || matches.is_present("fuzzy")
            || matches.is_present("recursive");

        let preset_names: Vec<String> = matches
            .values_of("preset")
            .map(|vals| vals.map(String::from).collect())
            .unwrap_or_default();

        let mut pattern_filters: Vec<String> = Vec::new();
        let mut size_raw = matches.value_of("size").map(String::from);
        let mut modified_raw = matches.value_of("modified").map(String::from);
        let mut created_raw = matches.value_of("created").map(String::from);
        let mut preset_refinements: Vec<String> = Vec::new();

        for preset in &preset_names {
            let preset_cfg = config
                .filter
                .presets
                .get(preset)
                .ok_or_else(|| LlaError::Filter(format!("Unknown preset '{}'", preset)))?;

            if let Some(pattern) = &preset_cfg.filter {
                pattern_filters.push(pattern.clone());
            }

            if size_raw.is_none() {
                size_raw = preset_cfg.size.clone();
            }
            if modified_raw.is_none() {
                modified_raw = preset_cfg.modified.clone();
            }
            if created_raw.is_none() {
                created_raw = preset_cfg.created.clone();
            }

            preset_refinements.extend(preset_cfg.refine.clone());
        }

        if let Some(cli_filter) = matches.value_of("filter") {
            pattern_filters.push(cli_filter.to_string());
        }

        let filter = combine_pattern_filters(&pattern_filters);
        let refine_filters = {
            let mut refinements = preset_refinements;
            if let Some(values) = matches.values_of("refine") {
                refinements.extend(values.map(String::from));
            }
            refinements
        };

        let size_filter_raw = size_raw.clone();
        let modified_filter_raw = modified_raw.clone();
        let created_filter_raw = created_raw.clone();
        let size_filter = match &size_raw {
            Some(raw) => Some(parse_size_range(raw)?),
            None => None,
        };
        let now = SystemTime::now();
        let modified_filter = match &modified_raw {
            Some(raw) => Some(parse_time_range(raw, now)?),
            None => None,
        };
        let created_filter = match &created_raw {
            Some(raw) => Some(parse_time_range(raw, now)?),
            None => None,
        };

        let search_pipelines = matches
            .values_of("search-pipe")
            .map(|vals| {
                vals.map(parse_search_pipeline_spec)
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();

        Ok(Args {
            directory: matches.value_of("directory").unwrap_or(".").to_string(),
            depth: matches
                .value_of("depth")
                .and_then(|s| s.parse().ok())
                .or(config.default_depth),
            long_format: matches.is_present("long")
                || (!has_format_flag && config.default_format == "long"),
            tree_format: matches.is_present("tree")
                || (!has_format_flag && config.default_format == "tree"),
            table_format: matches.is_present("table")
                || (!has_format_flag && config.default_format == "table"),
            grid_format: matches.is_present("grid")
                || (!has_format_flag && config.default_format == "grid"),
            grid_ignore: matches.is_present("grid-ignore"),
            sizemap_format: matches.is_present("sizemap")
                || (!has_format_flag && config.default_format == "sizemap"),
            timeline_format: matches.is_present("timeline")
                || (!has_format_flag && config.default_format == "timeline"),
            git_format: matches.is_present("git")
                || (!has_format_flag && config.default_format == "git"),
            fuzzy_format: matches.is_present("fuzzy"),
            recursive_format: matches.is_present("recursive")
                || (!has_format_flag && config.default_format == "recursive"),
            show_icons: matches.is_present("icons")
                || (!matches.is_present("no-icons") && config.show_icons),
            no_color: matches.is_present("no-color"),
            sort_by: matches
                .value_of("sort")
                .unwrap_or(&config.default_sort)
                .to_string(),
            sort_reverse: matches.is_present("sort-reverse"),
            sort_dirs_first: matches.is_present("sort-dirs-first") || config.sort.dirs_first,
            sort_case_sensitive: matches.is_present("sort-case-sensitive")
                || config.sort.case_sensitive,
            sort_natural: matches.is_present("sort-natural") || config.sort.natural,
            filter,
            presets: preset_names,
            size_filter,
            size_filter_raw,
            modified_filter,
            modified_filter_raw,
            created_filter,
            created_filter_raw,
            case_sensitive: matches.is_present("case-sensitive") || config.filter.case_sensitive,
            refine_filters,
            enable_plugin: matches
                .values_of("enable-plugin")
                .map(|v| v.map(String::from).collect())
                .unwrap_or_default(),
            disable_plugin: matches
                .values_of("disable-plugin")
                .map(|v| v.map(String::from).collect())
                .unwrap_or_default(),
            plugins_dir: matches
                .value_of("plugins-dir")
                .map(PathBuf::from)
                .unwrap_or_else(|| config.plugins_dir.clone()),
            include_dirs: matches.is_present("include-dirs") || config.include_dirs,
            dirs_only: matches.is_present("dirs-only"),
            files_only: matches.is_present("files-only"),
            symlinks_only: matches.is_present("symlinks-only"),
            no_dirs: matches.is_present("no-dirs"),
            no_files: matches.is_present("no-files"),
            no_symlinks: matches.is_present("no-symlinks"),
            no_dotfiles: matches.is_present("no-dotfiles")
                && !matches.is_present("all")
                && !matches.is_present("almost-all")
                && config.filter.no_dotfiles,
            almost_all: matches.is_present("almost-all"),
            dotfiles_only: matches.is_present("dotfiles-only"),
            permission_format: matches
                .value_of("permission-format")
                .unwrap_or(&config.permission_format)
                .to_string(),
            hide_group: matches.is_present("hide-group") || config.formatters.long.hide_group,
            relative_dates: matches.is_present("relative-dates")
                || config.formatters.long.relative_dates,
            output_mode: {
                let pretty = matches.is_present("pretty");
                if matches.is_present("json") {
                    OutputMode::Json { pretty }
                } else if matches.is_present("ndjson") {
                    OutputMode::Ndjson
                } else if matches.is_present("csv") {
                    OutputMode::Csv
                } else {
                    OutputMode::Human
                }
            },
            command,
            search: matches.value_of("search").map(String::from),
            search_context: matches
                .value_of("search-context")
                .and_then(|s| s.parse().ok())
                .unwrap_or(2),
            search_pipelines,
        })
    }
}

fn combine_pattern_filters(filters: &[String]) -> Option<String> {
    if filters.is_empty() {
        None
    } else if filters.len() == 1 {
        Some(filters[0].clone())
    } else {
        let joined = filters
            .iter()
            .map(|f| format!("({})", f))
            .collect::<Vec<_>>()
            .join(" AND ");
        Some(joined)
    }
}

fn parse_search_pipeline_spec(value: &str) -> Result<SearchPipelineSpec> {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() < 2 {
        return Err(LlaError::Parse(format!(
            "Invalid search pipeline '{}'. Use plugin:action[:arg...] syntax.",
            value
        )));
    }

    let plugin = parts[0].trim();
    let action = parts[1].trim();
    if plugin.is_empty() || action.is_empty() {
        return Err(LlaError::Parse(
            "Search pipeline requires both plugin and action names".into(),
        ));
    }

    let mut args = Vec::new();
    for extra in parts.iter().skip(2) {
        let trimmed = extra.trim();
        if !trimmed.is_empty() {
            args.push(trimmed.to_string());
        }
    }

    Ok(SearchPipelineSpec {
        plugin: plugin.to_string(),
        action: action.to_string(),
        args,
    })
}
