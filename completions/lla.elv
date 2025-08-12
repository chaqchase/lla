
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
            cand -d 'Set the depth for tree listing (default from config)'
            cand --depth 'Set the depth for tree listing (default from config)'
            cand -s 'Sort files by name, size, or date'
            cand --sort 'Sort files by name, size, or date'
            cand -f 'Filter files by name or extension'
            cand --filter 'Filter files by name or extension'
            cand --enable-plugin 'Enable specific plugins'
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
            cand --hide-group 'Hide group column in long format'
            cand --relative-dates 'Show relative dates (e.g., ''2h ago'') in long format'
            cand install 'Install a plugin'
            cand plugin 'Run a plugin action'
            cand list-plugins 'List all available plugins'
            cand use 'Interactive plugin manager'
            cand init 'Initialize the configuration file'
            cand config 'View or modify configuration'
            cand update 'Update installed plugins'
            cand clean 'This command will clean up invalid plugins'
            cand shortcut 'Manage command shortcuts'
            cand completion 'Generate shell completion scripts'
            cand theme 'Interactive theme manager'
            cand help 'Print this message or the help of the given subcommand(s)'
        }
        &'lla;install'= {
            cand --git 'Install a plugin from a GitHub repository URL'
            cand --dir 'Install a plugin from a local directory'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;plugin'= {
            cand -n 'Name of the plugin'
            cand --name 'Name of the plugin'
            cand -a 'Action to perform'
            cand --action 'Action to perform'
            cand -r 'Arguments for the plugin action'
            cand --args 'Arguments for the plugin action'
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
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;config'= {
            cand --set 'Set a configuration value (e.g., --set plugins_dir /new/path)'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;update'= {
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
            cand remove 'Remove a shortcut'
            cand list 'List all shortcuts'
            cand help 'Print this message or the help of the given subcommand(s)'
        }
        &'lla;shortcut;add'= {
            cand -d 'Optional description of the shortcut'
            cand --description 'Optional description of the shortcut'
            cand -h 'Print help information'
            cand --help 'Print help information'
        }
        &'lla;shortcut;remove'= {
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
        &'lla;theme;help'= {
        }
        &'lla;help'= {
        }
    ]
    $completions[$command]
}
