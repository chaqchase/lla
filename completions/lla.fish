complete -c lla -n "__fish_use_subcommand" -l search -d 'Search file contents with ripgrep for the given pattern' -r
complete -c lla -n "__fish_use_subcommand" -l search-context -d 'Number of context lines to show before and after matches (default: 2)' -r
complete -c lla -n "__fish_use_subcommand" -s d -l depth -d 'Set the depth for tree listing (default from config)' -r
complete -c lla -n "__fish_use_subcommand" -s s -l sort -d 'Sort files by name, size, or date' -r -f -a "{name	,size	,date	}"
complete -c lla -n "__fish_use_subcommand" -s f -l filter -d 'Filter files by name or extension' -r
complete -c lla -n "__fish_use_subcommand" -l preset -d 'Apply a named filter preset defined in your config' -r
complete -c lla -n "__fish_use_subcommand" -l size -d 'Filter by file size (e.g., \'>10M\', \'5K..2G\')' -r
complete -c lla -n "__fish_use_subcommand" -l modified -d 'Filter by modified time (e.g., \'<7d\', \'2023-01-01..2023-12-31\')' -r
complete -c lla -n "__fish_use_subcommand" -l created -d 'Filter by creation time using the same syntax as --modified' -r
complete -c lla -n "__fish_use_subcommand" -l refine -d 'Refine a previous listing (or cache) without re-walking the filesystem using additional filters' -r
complete -c lla -n "__fish_use_subcommand" -l enable-plugin -d 'Enable specific plugins' -r
complete -c lla -n "__fish_use_subcommand" -l search-pipe -d 'After --search finishes, run plugin action(s) on matching files (syntax: plugin:action[:arg...])' -r
complete -c lla -n "__fish_use_subcommand" -l disable-plugin -d 'Disable specific plugins' -r
complete -c lla -n "__fish_use_subcommand" -l plugins-dir -d 'Specify the plugins directory' -r
complete -c lla -n "__fish_use_subcommand" -l permission-format -d 'Format for displaying permissions (symbolic, octal, binary, verbose, compact)' -r -f -a "{symbolic	,octal	,binary	,verbose	,compact	}"
complete -c lla -n "__fish_use_subcommand" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_use_subcommand" -s V -l version -d 'Print version information'
complete -c lla -n "__fish_use_subcommand" -l json -d 'Output a single JSON array'
complete -c lla -n "__fish_use_subcommand" -l ndjson -d 'Output newline-delimited JSON (one object per line)'
complete -c lla -n "__fish_use_subcommand" -l csv -d 'Output CSV with header row'
complete -c lla -n "__fish_use_subcommand" -l pretty -d 'Pretty print JSON (only applies to --json)'
complete -c lla -n "__fish_use_subcommand" -s l -l long -d 'Use long listing format (overrides config format)'
complete -c lla -n "__fish_use_subcommand" -s t -l tree -d 'Use tree listing format (overrides config format)'
complete -c lla -n "__fish_use_subcommand" -s T -l table -d 'Use table listing format (overrides config format)'
complete -c lla -n "__fish_use_subcommand" -s g -l grid -d 'Use grid listing format (overrides config format)'
complete -c lla -n "__fish_use_subcommand" -l grid-ignore -d 'Use grid view ignoring terminal width (Warning: output may extend beyond screen width)'
complete -c lla -n "__fish_use_subcommand" -s S -l sizemap -d 'Show visual representation of file sizes (overrides config format)'
complete -c lla -n "__fish_use_subcommand" -l timeline -d 'Group files by time periods (overrides config format)'
complete -c lla -n "__fish_use_subcommand" -s G -l git -d 'Show git status and information (overrides config format)'
complete -c lla -n "__fish_use_subcommand" -s F -l fuzzy -d 'Use interactive fuzzy finder'
complete -c lla -n "__fish_use_subcommand" -l icons -d 'Show icons for files and directories (overrides config setting)'
complete -c lla -n "__fish_use_subcommand" -l no-icons -d 'Hide icons for files and directories (overrides config setting)'
complete -c lla -n "__fish_use_subcommand" -l no-color -d 'Disable all colors in the output'
complete -c lla -n "__fish_use_subcommand" -s r -l sort-reverse -d 'Reverse the sort order'
complete -c lla -n "__fish_use_subcommand" -l sort-dirs-first -d 'List directories before files (overrides config setting)'
complete -c lla -n "__fish_use_subcommand" -l sort-case-sensitive -d 'Enable case-sensitive sorting (overrides config setting)'
complete -c lla -n "__fish_use_subcommand" -l sort-natural -d 'Use natural sorting for numbers (overrides config setting)'
complete -c lla -n "__fish_use_subcommand" -s c -l case-sensitive -d 'Enable case-sensitive filtering (overrides config setting)'
complete -c lla -n "__fish_use_subcommand" -s R -l recursive -d 'Use recursive listing format'
complete -c lla -n "__fish_use_subcommand" -l include-dirs -d 'Include directory sizes in the metadata'
complete -c lla -n "__fish_use_subcommand" -l dirs-only -d 'Show only directories'
complete -c lla -n "__fish_use_subcommand" -l files-only -d 'Show only regular files'
complete -c lla -n "__fish_use_subcommand" -l symlinks-only -d 'Show only symbolic links'
complete -c lla -n "__fish_use_subcommand" -l no-dirs -d 'Hide directories'
complete -c lla -n "__fish_use_subcommand" -l no-files -d 'Hide regular files'
complete -c lla -n "__fish_use_subcommand" -l no-symlinks -d 'Hide symbolic links'
complete -c lla -n "__fish_use_subcommand" -l no-dotfiles -d 'Hide files starting with a dot (overrides config setting)'
complete -c lla -n "__fish_use_subcommand" -s a -l all -d 'Show all files including dotfiles (overrides no_dotfiles config)'
complete -c lla -n "__fish_use_subcommand" -s A -l almost-all -d 'Show all files including dotfiles except . and .. (overrides no_dotfiles config)'
complete -c lla -n "__fish_use_subcommand" -l dotfiles-only -d 'Show only dot files and directories (those starting with a dot)'
complete -c lla -n "__fish_use_subcommand" -l respect-gitignore -d 'Hide files that match .gitignore (and git exclude) rules'
complete -c lla -n "__fish_use_subcommand" -l no-gitignore -d 'Disable .gitignore filtering even if enabled in config'
complete -c lla -n "__fish_use_subcommand" -l hide-group -d 'Hide group column in long format'
complete -c lla -n "__fish_use_subcommand" -l relative-dates -d 'Show relative dates (e.g., \'2h ago\') in long format'
complete -c lla -n "__fish_use_subcommand" -f -a "diff" -d 'Compare two directories or a directory against a git reference'
complete -c lla -n "__fish_use_subcommand" -f -a "jump" -d 'Jump to a bookmarked or recent directory'
complete -c lla -n "__fish_use_subcommand" -f -a "install" -d 'Install a plugin'
complete -c lla -n "__fish_use_subcommand" -f -a "plugin" -d 'Run a plugin action'
complete -c lla -n "__fish_use_subcommand" -f -a "list-plugins" -d 'List all available plugins'
complete -c lla -n "__fish_use_subcommand" -f -a "use" -d 'Interactive plugin manager'
complete -c lla -n "__fish_use_subcommand" -f -a "init" -d 'Initialize the configuration file'
complete -c lla -n "__fish_use_subcommand" -f -a "config" -d 'View or modify configuration'
complete -c lla -n "__fish_use_subcommand" -f -a "update" -d 'Update installed plugins'
complete -c lla -n "__fish_use_subcommand" -f -a "upgrade" -d 'Upgrade the lla CLI to the latest (or specified) release'
complete -c lla -n "__fish_use_subcommand" -f -a "clean" -d 'This command will clean up invalid plugins'
complete -c lla -n "__fish_use_subcommand" -f -a "shortcut" -d 'Manage command shortcuts'
complete -c lla -n "__fish_use_subcommand" -f -a "completion" -d 'Generate shell completion scripts'
complete -c lla -n "__fish_use_subcommand" -f -a "theme" -d 'Interactive theme manager'
complete -c lla -n "__fish_use_subcommand" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c lla -n "__fish_seen_subcommand_from diff" -l git-ref -d 'Git reference to compare against (default: HEAD)' -r
complete -c lla -n "__fish_seen_subcommand_from diff" -l git -d 'Compare the directory against a git reference instead of another directory'
complete -c lla -n "__fish_seen_subcommand_from diff" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from jump" -l add -d 'Add a directory to bookmarks' -r
complete -c lla -n "__fish_seen_subcommand_from jump" -l remove -d 'Remove a directory from bookmarks' -r
complete -c lla -n "__fish_seen_subcommand_from jump" -l shell -d 'Override shell detection for setup (bash|zsh|fish)' -r -f -a "{bash	,zsh	,fish	}"
complete -c lla -n "__fish_seen_subcommand_from jump" -l list -d 'List bookmarks and history'
complete -c lla -n "__fish_seen_subcommand_from jump" -l clear-history -d 'Clear directory history'
complete -c lla -n "__fish_seen_subcommand_from jump" -l setup -d 'Setup shell integration for seamless directory jumping'
complete -c lla -n "__fish_seen_subcommand_from jump" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from install" -l git -d 'Install a plugin from a GitHub repository URL' -r
complete -c lla -n "__fish_seen_subcommand_from install" -l dir -d 'Install a plugin from a local directory' -r
complete -c lla -n "__fish_seen_subcommand_from install" -l prebuilt -d 'Install plugins from the latest prebuilt release (default)'
complete -c lla -n "__fish_seen_subcommand_from install" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from plugin" -s n -l name -d 'Name of the plugin (alternative to positional)' -r
complete -c lla -n "__fish_seen_subcommand_from plugin" -s a -l action -d 'Action to perform (alternative to positional)' -r
complete -c lla -n "__fish_seen_subcommand_from plugin" -s r -l args -d 'Arguments for the plugin action (alternative to positional)' -r
complete -c lla -n "__fish_seen_subcommand_from plugin" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from list-plugins" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from use" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from init" -l default -d 'Write the default config without launching the wizard'
complete -c lla -n "__fish_seen_subcommand_from init" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from config; and not __fish_seen_subcommand_from show-effective; and not __fish_seen_subcommand_from diff; and not __fish_seen_subcommand_from help" -l set -d 'Set a configuration value (e.g., --set plugins_dir /new/path)' -r
complete -c lla -n "__fish_seen_subcommand_from config; and not __fish_seen_subcommand_from show-effective; and not __fish_seen_subcommand_from diff; and not __fish_seen_subcommand_from help" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from config; and not __fish_seen_subcommand_from show-effective; and not __fish_seen_subcommand_from diff; and not __fish_seen_subcommand_from help" -f -a "show-effective" -d 'Show the merged config (global + nearest .lla.toml)'
complete -c lla -n "__fish_seen_subcommand_from config; and not __fish_seen_subcommand_from show-effective; and not __fish_seen_subcommand_from diff; and not __fish_seen_subcommand_from help" -f -a "diff" -d 'Compare config overrides against defaults'
complete -c lla -n "__fish_seen_subcommand_from config; and not __fish_seen_subcommand_from show-effective; and not __fish_seen_subcommand_from diff; and not __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c lla -n "__fish_seen_subcommand_from config; and __fish_seen_subcommand_from show-effective" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from config; and __fish_seen_subcommand_from diff" -l default -d 'Diff against the built-in defaults'
complete -c lla -n "__fish_seen_subcommand_from config; and __fish_seen_subcommand_from diff" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from update" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from upgrade" -s v -l version -d 'Upgrade to a specific release tag (defaults to the latest release)' -r
complete -c lla -n "__fish_seen_subcommand_from upgrade" -l path -d 'Install location for the lla binary (defaults to the current executable path)' -r
complete -c lla -n "__fish_seen_subcommand_from upgrade" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from clean" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and not __fish_seen_subcommand_from add; and not __fish_seen_subcommand_from create; and not __fish_seen_subcommand_from remove; and not __fish_seen_subcommand_from export; and not __fish_seen_subcommand_from import; and not __fish_seen_subcommand_from list; and not __fish_seen_subcommand_from help" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and not __fish_seen_subcommand_from add; and not __fish_seen_subcommand_from create; and not __fish_seen_subcommand_from remove; and not __fish_seen_subcommand_from export; and not __fish_seen_subcommand_from import; and not __fish_seen_subcommand_from list; and not __fish_seen_subcommand_from help" -f -a "add" -d 'Add a new shortcut'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and not __fish_seen_subcommand_from add; and not __fish_seen_subcommand_from create; and not __fish_seen_subcommand_from remove; and not __fish_seen_subcommand_from export; and not __fish_seen_subcommand_from import; and not __fish_seen_subcommand_from list; and not __fish_seen_subcommand_from help" -f -a "create" -d 'Interactively create a new shortcut'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and not __fish_seen_subcommand_from add; and not __fish_seen_subcommand_from create; and not __fish_seen_subcommand_from remove; and not __fish_seen_subcommand_from export; and not __fish_seen_subcommand_from import; and not __fish_seen_subcommand_from list; and not __fish_seen_subcommand_from help" -f -a "remove" -d 'Remove a shortcut'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and not __fish_seen_subcommand_from add; and not __fish_seen_subcommand_from create; and not __fish_seen_subcommand_from remove; and not __fish_seen_subcommand_from export; and not __fish_seen_subcommand_from import; and not __fish_seen_subcommand_from list; and not __fish_seen_subcommand_from help" -f -a "export" -d 'Export shortcuts to a file'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and not __fish_seen_subcommand_from add; and not __fish_seen_subcommand_from create; and not __fish_seen_subcommand_from remove; and not __fish_seen_subcommand_from export; and not __fish_seen_subcommand_from import; and not __fish_seen_subcommand_from list; and not __fish_seen_subcommand_from help" -f -a "import" -d 'Import shortcuts from a file'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and not __fish_seen_subcommand_from add; and not __fish_seen_subcommand_from create; and not __fish_seen_subcommand_from remove; and not __fish_seen_subcommand_from export; and not __fish_seen_subcommand_from import; and not __fish_seen_subcommand_from list; and not __fish_seen_subcommand_from help" -f -a "list" -d 'List all shortcuts'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and not __fish_seen_subcommand_from add; and not __fish_seen_subcommand_from create; and not __fish_seen_subcommand_from remove; and not __fish_seen_subcommand_from export; and not __fish_seen_subcommand_from import; and not __fish_seen_subcommand_from list; and not __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and __fish_seen_subcommand_from add" -s d -l description -d 'Optional description of the shortcut' -r
complete -c lla -n "__fish_seen_subcommand_from shortcut; and __fish_seen_subcommand_from add" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and __fish_seen_subcommand_from create" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and __fish_seen_subcommand_from remove" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and __fish_seen_subcommand_from export" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and __fish_seen_subcommand_from import" -l merge -d 'Merge with existing shortcuts (skip conflicts)'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and __fish_seen_subcommand_from import" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from shortcut; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from completion" -s p -l path -d 'Custom installation path for the completion script' -r
complete -c lla -n "__fish_seen_subcommand_from completion" -s o -l output -d 'Output path for the completion script (prints to stdout if not specified)' -r
complete -c lla -n "__fish_seen_subcommand_from completion" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from theme; and not __fish_seen_subcommand_from pull; and not __fish_seen_subcommand_from install; and not __fish_seen_subcommand_from preview; and not __fish_seen_subcommand_from help" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from theme; and not __fish_seen_subcommand_from pull; and not __fish_seen_subcommand_from install; and not __fish_seen_subcommand_from preview; and not __fish_seen_subcommand_from help" -f -a "pull" -d 'Pull and install themes from the official repository'
complete -c lla -n "__fish_seen_subcommand_from theme; and not __fish_seen_subcommand_from pull; and not __fish_seen_subcommand_from install; and not __fish_seen_subcommand_from preview; and not __fish_seen_subcommand_from help" -f -a "install" -d 'Install theme(s) from a file or directory'
complete -c lla -n "__fish_seen_subcommand_from theme; and not __fish_seen_subcommand_from pull; and not __fish_seen_subcommand_from install; and not __fish_seen_subcommand_from preview; and not __fish_seen_subcommand_from help" -f -a "preview" -d 'Preview a theme using sample output'
complete -c lla -n "__fish_seen_subcommand_from theme; and not __fish_seen_subcommand_from pull; and not __fish_seen_subcommand_from install; and not __fish_seen_subcommand_from preview; and not __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c lla -n "__fish_seen_subcommand_from theme; and __fish_seen_subcommand_from pull" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from theme; and __fish_seen_subcommand_from install" -s h -l help -d 'Print help information'
complete -c lla -n "__fish_seen_subcommand_from theme; and __fish_seen_subcommand_from preview" -s h -l help -d 'Print help information'
