
use builtin;
use str;

set edit:completion:arg-completer[lla] = {|@words|
    fn spaces {|n|
        builtin:repeat $n ' ' | str:join ''
    }
    fn cand {|text desc|
        edit:complex-candidate $text &display=$text' '(spaces (- 14 (wcswidth $text)))$desc
    }
    var command = 'lla'
    for word $words[1..-1] {
        if (str:has-prefix $word '-') {
            break
        }
        set command = $command';'$word
    }
    var completions = [
        &'lla'= {
            cand --search 'Search file contents with ripgrep for the given pattern'
            cand --search-context 'Number of context lines to show before and after matches (default: 2)'
            cand -d 'Set the depth for tree listing (default from config)'
            cand --depth 'Set the depth for tree listing (default from config)'
            cand -s 'Sort files by name, size, or date'
            cand --sort 'Sort files by name, size, or date'
            cand -f 'Filter files by name or extension'
            cand --filter 'Filter files by name or extension'
            cand --preset 'Apply a named filter preset defined in your config'
            cand --size 'Filter by file size (e.g., ''>10M'', ''5K..2G'')'
            cand --modified 'Filter by modified time (e.g., ''<7d'', ''2023-01-01..2023-12-31'')'
            cand --created 'Filter by creation time using the same syntax as --modified'
            cand --refine 'Refine a previous listing (or cache) without re-walking the filesystem using additional filters'
            cand --enable-plugin 'Enable specific plugins'
            cand --search-pipe 'After --search finishes, run plugin action(s) on matching files (syntax: plugin:action[:arg...])'
            cand --disable-plugin 'Disable specific plugins'
            cand --plugins-dir 'Specify the plugins directory'
            cand --permission-format 'Format for displaying permissions (symbolic, octal, binary, verbose, compact)'
            cand -h 'Print help information'
            cand --help 'Print help information'
            cand -V 'Print version information'
            cand --version 'Print version information'
            cand --json 'Output a single JSON array'
            cand --ndjson 'Output newline-delimited JSON (one object per line)'
            cand --csv 'Output CSV with header row'
            cand --pretty 'Pretty print JSON (only applies to --json)'
            cand -l 'Use long listing format (overrides config format)'
            cand --long 'Use long listing format (overrides config format)'
            cand -t 'Use tree listing format (overrides config format)'
            cand --tree 'Use tree listing format (overrides config format)'
            cand -T 'Use table listing format (overrides config format)'
            cand --table 'Use table listing format (overrides config format)'
            cand -g 'Use grid listing format (overrides config format)'
            cand --grid 'Use grid listing format (overrides config format)'
            cand --grid-ignore 'Use grid view ignoring terminal width (Warning: output may extend beyond screen width)'
            cand -S 'Show visual representation of file sizes (overrides config format)'
            cand --sizemap 'Show visual representation of file sizes (overrides config format)'
            cand --timeline 'Group files by time periods (overrides config format)'
            cand -G 'Show git status and information (overrides config format)'
            cand --git 'Show git status and information (overrides config format)'
            cand -F 'Use interactive fuzzy finder'
            cand --fuzzy 'Use interactive fuzzy finder'
            cand --icons 'Show icons for files and directories (overrides config setting)'
            cand --no-icons 'Hide icons for files and directories (overrides config setting)'
            cand --no-color 'Disable all colors in the output'
            cand -r 'Reverse the sort order'
            cand --sort-reverse 'Reverse the sort order'
            cand --sort-dirs-first 'List directories before files (overrides config setting)'
            cand --sort-case-sensitive 'Enable case-sensitive sorting (overrides config setting)'
            cand --sort-natural 'Use natural sorting for numbers (overrides config setting)'
            cand -c 'Enable case-sensitive filtering (overrides config setting)'
            cand --case-sensitive 'Enable case-sensitive filtering (overrides config setting)'
            cand -R 'Use recursive listing format'
            cand --recursive 'Use recursive listing format'
            cand --include-dirs 'Include directory sizes in the metadata'
            cand --dirs-only 'Show only directories'
            cand --files-only 'Show only regular files'
            cand --symlinks-only 'Show only symbolic links'
            cand --no-dirs 'Hide directories'
            cand --no-files 'Hide regular files'
            cand --no-symlinks 'Hide symbolic links'
            cand --no-dotfiles 'Hide files starting with a dot (overrides config setting)'
            cand -a 'Show all files including dotfiles (overrides no_dotfiles config)'
            cand --all 'Show all files including dotfiles (overrides no_dotfiles config)'
            cand -A 'Show all files including dotfiles except . and .. (overrides no_dotfiles config)'
            cand --almost-all 'Show all files including dotfiles except . and .. (overrides no_dotfiles config)'
            cand --dotfiles-only 'Show only dot files and directories (those starting with a dot)'
            cand --respect-gitignore 'Hide files that match .gitignore (and git exclude) rules'
            cand --no-gitignore 'Disable .gitignore filtering even if enabled in config'
            cand --hide-group 'Hide group column in long format'
            cand --relative-dates 'Show relative dates (e.g., ''2h ago'') in long format'
            cand diff 'Compare two directories or a directory against a git reference'
            cand jump 'Jump to a bookmarked or recent directory'
            cand install 'Install a plugin'
            cand plugin 'Run a plugin action'
            cand list-plugins 'List all available plugins'
            cand use 'Interactive plugin manager'
            cand init 'Initialize the configuration file'
            cand config 'View or modify configuration'
            cand update 'Update installed plugins'
            cand upgrade 'Upgrade the lla CLI to the latest (or specified) release'
            cand clean 'This command will clean up invalid plugins'
            cand shortcut 'Manage command shortcuts'
            cand completion 'Generate shell completion scripts'
            cand theme 'Interactive theme manager'
            cand help 'Print this message or the help of the given subcommand(s)'
        }
        &'lla;diff'= {
            cand --git-ref 'Git reference to compare against (default: HEAD)'
            cand --git 'Compare the directory against a git reference instead of another directory'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;jump'= {
            cand --add 'Add a directory to bookmarks'
            cand --remove 'Remove a directory from bookmarks'
            cand --shell 'Override shell detection for setup (bash|zsh|fish)'
            cand --list 'List bookmarks and history'
            cand --clear-history 'Clear directory history'
            cand --setup 'Setup shell integration for seamless directory jumping'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;install'= {
            cand --git 'Install a plugin from a GitHub repository URL'
            cand --dir 'Install a plugin from a local directory'
            cand --prebuilt 'Install plugins from the latest prebuilt release (default)'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;plugin'= {
            cand -n 'Name of the plugin (alternative to positional)'
            cand --name 'Name of the plugin (alternative to positional)'
            cand -a 'Action to perform (alternative to positional)'
            cand --action 'Action to perform (alternative to positional)'
            cand -r 'Arguments for the plugin action (alternative to positional)'
            cand --args 'Arguments for the plugin action (alternative to positional)'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;list-plugins'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;use'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;init'= {
            cand --default 'Write the default config without launching the wizard'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;config'= {
            cand --set 'Set a configuration value (e.g., --set plugins_dir /new/path)'
            cand -h 'Print help information'
            cand --help 'Print help information'
            cand show-effective 'Show the merged config (global + nearest .lla.toml)'
            cand diff 'Compare config overrides against defaults'
            cand help 'Print this message or the help of the given subcommand(s)'
        }
        &'lla;config;show-effective'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;config;diff'= {
            cand --default 'Diff against the built-in defaults'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;config;help'= {
        }
        &'lla;update'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;upgrade'= {
            cand -v 'Upgrade to a specific release tag (defaults to the latest release)'
            cand --version 'Upgrade to a specific release tag (defaults to the latest release)'
            cand --path 'Install location for the lla binary (defaults to the current executable path)'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;clean'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;shortcut'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
            cand add 'Add a new shortcut'
            cand create 'Interactively create a new shortcut'
            cand remove 'Remove a shortcut'
            cand export 'Export shortcuts to a file'
            cand import 'Import shortcuts from a file'
            cand list 'List all shortcuts'
            cand help 'Print this message or the help of the given subcommand(s)'
        }
        &'lla;shortcut;add'= {
            cand -d 'Optional description of the shortcut'
            cand --description 'Optional description of the shortcut'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;shortcut;create'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;shortcut;remove'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;shortcut;export'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;shortcut;import'= {
            cand --merge 'Merge with existing shortcuts (skip conflicts)'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;shortcut;list'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;shortcut;help'= {
        }
        &'lla;completion'= {
            cand -p 'Custom installation path for the completion script'
            cand --path 'Custom installation path for the completion script'
            cand -o 'Output path for the completion script (prints to stdout if not specified)'
            cand --output 'Output path for the completion script (prints to stdout if not specified)'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;theme'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
            cand pull 'Pull and install themes from the official repository'
            cand install 'Install theme(s) from a file or directory'
            cand preview 'Preview a theme using sample output'
            cand help 'Print this message or the help of the given subcommand(s)'
        }
        &'lla;theme;pull'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;theme;install'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;theme;preview'= {
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;theme;help'= {
        }
        &'lla;help'= {
        }
    ]
    $completions[$command]
}
