use crate::config::{Config, ShortcutCommand};
use clap::{App, Arg, ArgGroup, ArgMatches, SubCommand};
use clap_complete::Shell;
use std::path::PathBuf;

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
    pub case_sensitive: bool,
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
}

pub enum Command {
    Install(InstallSource),
    ListPlugins,
    Use,
    InitConfig,
    Config(Option<ConfigAction>),
    PluginAction(String, String, Vec<String>),
    Update(Option<String>),
    Clean,
    Shortcut(ShortcutAction),
    GenerateCompletion(Shell, Option<String>, Option<String>),
    Theme,
    ThemePull,
    ThemeInstall(String),
}

pub enum InstallSource {
    GitHub(String),
    LocalDir(String),
}

pub enum ShortcutAction {
    Add(String, ShortcutCommand),
    Remove(String),
    List,
    Run(String, Vec<String>),
}

#[derive(Clone)]
pub enum ConfigAction {
    View,
    Set(String, String),
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
                Arg::with_name("case-sensitive")
                    .short('c')
                    .long("case-sensitive")
                    .help("Enable case-sensitive filtering (overrides config setting)"),
            )
            .arg(
                Arg::with_name("enable-plugin")
                    .long("enable-plugin")
                    .takes_value(true)
                    .multiple(true)
                    .help("Enable specific plugins"),
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
                    ),
            )
            .subcommand(
                SubCommand::with_name("plugin")
                    .about("Run a plugin action")
                    .arg(
                        Arg::with_name("name")
                            .long("name")
                            .short('n')
                            .takes_value(true)
                            .required(true)
                            .help("Name of the plugin"),
                    )
                    .arg(
                        Arg::with_name("action")
                            .long("action")
                            .short('a')
                            .takes_value(true)
                            .required(true)
                            .help("Action to perform"),
                    )
                    .arg(
                        Arg::with_name("args")
                            .long("args")
                            .short('r')
                            .takes_value(true)
                            .multiple(true)
                            .help("Arguments for the plugin action"),
                    ),
            )
            .subcommand(SubCommand::with_name("list-plugins").about("List all available plugins"))
            .subcommand(SubCommand::with_name("use").about("Interactive plugin manager"))
            .subcommand(SubCommand::with_name("init").about("Initialize the configuration file"))
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
                        SubCommand::with_name("remove")
                            .about("Remove a shortcut")
                            .arg(
                                Arg::with_name("name")
                                    .help("Name of the shortcut to remove")
                                    .required(true)
                                    .index(1),
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
                    ),
            )
    }

    pub fn parse(config: &Config) -> Self {
        let args: Vec<String> = std::env::args().collect();
        if args.len() > 1 {
            let potential_shortcut = &args[1];
            if config.get_shortcut(potential_shortcut).is_some() {
                return Self {
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
                    case_sensitive: config.filter.case_sensitive,
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
                };
            }
        }

        let matches = Self::build_cli(config).get_matches();
        Self::from_matches(&matches, config)
    }

    pub fn get_cli(config: &Config) -> App<'_> {
        Self::build_cli(config)
    }

    fn from_matches(matches: &ArgMatches, config: &Config) -> Self {
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
            } else if let Some(remove_matches) = matches.subcommand_matches("remove") {
                Some(Command::Shortcut(ShortcutAction::Remove(
                    remove_matches.value_of("name").unwrap().to_string(),
                )))
            } else if matches.subcommand_matches("list").is_some() {
                Some(Command::Shortcut(ShortcutAction::List))
            } else {
                None
            }
        } else if matches.subcommand_matches("clean").is_some() {
            Some(Command::Clean)
        } else if let Some(install_matches) = matches.subcommand_matches("install") {
            if let Some(github_url) = install_matches.value_of("git") {
                Some(Command::Install(InstallSource::GitHub(
                    github_url.to_string(),
                )))
            } else if let Some(local_dir) = install_matches.value_of("dir") {
                Some(Command::Install(InstallSource::LocalDir(
                    local_dir.to_string(),
                )))
            } else {
                // default --git https://github.com/triyanox/lla
                Some(Command::Install(InstallSource::GitHub(
                    "https://github.com/triyanox/lla".to_string(),
                )))
            }
        } else if matches.subcommand_matches("list-plugins").is_some() {
            Some(Command::ListPlugins)
        } else if matches.subcommand_matches("use").is_some() {
            Some(Command::Use)
        } else if matches.subcommand_matches("init").is_some() {
            Some(Command::InitConfig)
        } else if let Some(config_matches) = matches.subcommand_matches("config") {
            if let Some(values) = config_matches.values_of("set") {
                let values: Vec<_> = values.collect();
                Some(Command::Config(Some(ConfigAction::Set(
                    values[0].to_string(),
                    values[1].to_string(),
                ))))
            } else {
                Some(Command::Config(Some(ConfigAction::View)))
            }
        } else if let Some(plugin_matches) = matches.subcommand_matches("plugin") {
            let plugin_name = plugin_matches.value_of("name").unwrap().to_string();
            let action = plugin_matches.value_of("action").unwrap().to_string();
            let args = plugin_matches
                .values_of("args")
                .map(|v| v.map(String::from).collect())
                .unwrap_or_default();
            Some(Command::PluginAction(plugin_name, action, args))
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

        Args {
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
            filter: matches.value_of("filter").map(String::from),
            case_sensitive: matches.is_present("case-sensitive") || config.filter.case_sensitive,
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
        }
    }
}
