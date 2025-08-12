
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'lla' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'lla'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'lla' {
            [CompletionResult]::new('-d', 'd', [CompletionResultType]::ParameterName, 'Set the depth for tree listing (default from config)')
            [CompletionResult]::new('--depth', 'depth', [CompletionResultType]::ParameterName, 'Set the depth for tree listing (default from config)')
            [CompletionResult]::new('-s', 's', [CompletionResultType]::ParameterName, 'Sort files by name, size, or date')
            [CompletionResult]::new('--sort', 'sort', [CompletionResultType]::ParameterName, 'Sort files by name, size, or date')
            [CompletionResult]::new('-f', 'f', [CompletionResultType]::ParameterName, 'Filter files by name or extension')
            [CompletionResult]::new('--filter', 'filter', [CompletionResultType]::ParameterName, 'Filter files by name or extension')
            [CompletionResult]::new('--enable-plugin', 'enable-plugin', [CompletionResultType]::ParameterName, 'Enable specific plugins')
            [CompletionResult]::new('--disable-plugin', 'disable-plugin', [CompletionResultType]::ParameterName, 'Disable specific plugins')
            [CompletionResult]::new('--plugins-dir', 'plugins-dir', [CompletionResultType]::ParameterName, 'Specify the plugins directory')
            [CompletionResult]::new('--permission-format', 'permission-format', [CompletionResultType]::ParameterName, 'Format for displaying permissions (symbolic, octal, binary, verbose, compact)')
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('-V', 'V', [CompletionResultType]::ParameterName, 'Print version information')
            [CompletionResult]::new('--version', 'version', [CompletionResultType]::ParameterName, 'Print version information')
            [CompletionResult]::new('--json', 'json', [CompletionResultType]::ParameterName, 'Output a single JSON array')
            [CompletionResult]::new('--ndjson', 'ndjson', [CompletionResultType]::ParameterName, 'Output newline-delimited JSON (one object per line)')
            [CompletionResult]::new('--csv', 'csv', [CompletionResultType]::ParameterName, 'Output CSV with header row')
            [CompletionResult]::new('--pretty', 'pretty', [CompletionResultType]::ParameterName, 'Pretty print JSON (only applies to --json)')
            [CompletionResult]::new('-l', 'l', [CompletionResultType]::ParameterName, 'Use long listing format (overrides config format)')
            [CompletionResult]::new('--long', 'long', [CompletionResultType]::ParameterName, 'Use long listing format (overrides config format)')
            [CompletionResult]::new('-t', 't', [CompletionResultType]::ParameterName, 'Use tree listing format (overrides config format)')
            [CompletionResult]::new('--tree', 'tree', [CompletionResultType]::ParameterName, 'Use tree listing format (overrides config format)')
            [CompletionResult]::new('-T', 'T', [CompletionResultType]::ParameterName, 'Use table listing format (overrides config format)')
            [CompletionResult]::new('--table', 'table', [CompletionResultType]::ParameterName, 'Use table listing format (overrides config format)')
            [CompletionResult]::new('-g', 'g', [CompletionResultType]::ParameterName, 'Use grid listing format (overrides config format)')
            [CompletionResult]::new('--grid', 'grid', [CompletionResultType]::ParameterName, 'Use grid listing format (overrides config format)')
            [CompletionResult]::new('--grid-ignore', 'grid-ignore', [CompletionResultType]::ParameterName, 'Use grid view ignoring terminal width (Warning: output may extend beyond screen width)')
            [CompletionResult]::new('-S', 'S', [CompletionResultType]::ParameterName, 'Show visual representation of file sizes (overrides config format)')
            [CompletionResult]::new('--sizemap', 'sizemap', [CompletionResultType]::ParameterName, 'Show visual representation of file sizes (overrides config format)')
            [CompletionResult]::new('--timeline', 'timeline', [CompletionResultType]::ParameterName, 'Group files by time periods (overrides config format)')
            [CompletionResult]::new('-G', 'G', [CompletionResultType]::ParameterName, 'Show git status and information (overrides config format)')
            [CompletionResult]::new('--git', 'git', [CompletionResultType]::ParameterName, 'Show git status and information (overrides config format)')
            [CompletionResult]::new('-F', 'F', [CompletionResultType]::ParameterName, 'Use interactive fuzzy finder')
            [CompletionResult]::new('--fuzzy', 'fuzzy', [CompletionResultType]::ParameterName, 'Use interactive fuzzy finder')
            [CompletionResult]::new('--icons', 'icons', [CompletionResultType]::ParameterName, 'Show icons for files and directories (overrides config setting)')
            [CompletionResult]::new('--no-icons', 'no-icons', [CompletionResultType]::ParameterName, 'Hide icons for files and directories (overrides config setting)')
            [CompletionResult]::new('--no-color', 'no-color', [CompletionResultType]::ParameterName, 'Disable all colors in the output')
            [CompletionResult]::new('-r', 'r', [CompletionResultType]::ParameterName, 'Reverse the sort order')
            [CompletionResult]::new('--sort-reverse', 'sort-reverse', [CompletionResultType]::ParameterName, 'Reverse the sort order')
            [CompletionResult]::new('--sort-dirs-first', 'sort-dirs-first', [CompletionResultType]::ParameterName, 'List directories before files (overrides config setting)')
            [CompletionResult]::new('--sort-case-sensitive', 'sort-case-sensitive', [CompletionResultType]::ParameterName, 'Enable case-sensitive sorting (overrides config setting)')
            [CompletionResult]::new('--sort-natural', 'sort-natural', [CompletionResultType]::ParameterName, 'Use natural sorting for numbers (overrides config setting)')
            [CompletionResult]::new('-c', 'c', [CompletionResultType]::ParameterName, 'Enable case-sensitive filtering (overrides config setting)')
            [CompletionResult]::new('--case-sensitive', 'case-sensitive', [CompletionResultType]::ParameterName, 'Enable case-sensitive filtering (overrides config setting)')
            [CompletionResult]::new('-R', 'R', [CompletionResultType]::ParameterName, 'Use recursive listing format')
            [CompletionResult]::new('--recursive', 'recursive', [CompletionResultType]::ParameterName, 'Use recursive listing format')
            [CompletionResult]::new('--include-dirs', 'include-dirs', [CompletionResultType]::ParameterName, 'Include directory sizes in the metadata')
            [CompletionResult]::new('--dirs-only', 'dirs-only', [CompletionResultType]::ParameterName, 'Show only directories')
            [CompletionResult]::new('--files-only', 'files-only', [CompletionResultType]::ParameterName, 'Show only regular files')
            [CompletionResult]::new('--symlinks-only', 'symlinks-only', [CompletionResultType]::ParameterName, 'Show only symbolic links')
            [CompletionResult]::new('--no-dirs', 'no-dirs', [CompletionResultType]::ParameterName, 'Hide directories')
            [CompletionResult]::new('--no-files', 'no-files', [CompletionResultType]::ParameterName, 'Hide regular files')
            [CompletionResult]::new('--no-symlinks', 'no-symlinks', [CompletionResultType]::ParameterName, 'Hide symbolic links')
            [CompletionResult]::new('--no-dotfiles', 'no-dotfiles', [CompletionResultType]::ParameterName, 'Hide files starting with a dot (overrides config setting)')
            [CompletionResult]::new('-a', 'a', [CompletionResultType]::ParameterName, 'Show all files including dotfiles (overrides no_dotfiles config)')
            [CompletionResult]::new('--all', 'all', [CompletionResultType]::ParameterName, 'Show all files including dotfiles (overrides no_dotfiles config)')
            [CompletionResult]::new('-A', 'A', [CompletionResultType]::ParameterName, 'Show all files including dotfiles except . and .. (overrides no_dotfiles config)')
            [CompletionResult]::new('--almost-all', 'almost-all', [CompletionResultType]::ParameterName, 'Show all files including dotfiles except . and .. (overrides no_dotfiles config)')
            [CompletionResult]::new('--dotfiles-only', 'dotfiles-only', [CompletionResultType]::ParameterName, 'Show only dot files and directories (those starting with a dot)')
            [CompletionResult]::new('--hide-group', 'hide-group', [CompletionResultType]::ParameterName, 'Hide group column in long format')
            [CompletionResult]::new('--relative-dates', 'relative-dates', [CompletionResultType]::ParameterName, 'Show relative dates (e.g., ''2h ago'') in long format')
            [CompletionResult]::new('install', 'install', [CompletionResultType]::ParameterValue, 'Install a plugin')
            [CompletionResult]::new('plugin', 'plugin', [CompletionResultType]::ParameterValue, 'Run a plugin action')
            [CompletionResult]::new('list-plugins', 'list-plugins', [CompletionResultType]::ParameterValue, 'List all available plugins')
            [CompletionResult]::new('use', 'use', [CompletionResultType]::ParameterValue, 'Interactive plugin manager')
            [CompletionResult]::new('init', 'init', [CompletionResultType]::ParameterValue, 'Initialize the configuration file')
            [CompletionResult]::new('config', 'config', [CompletionResultType]::ParameterValue, 'View or modify configuration')
            [CompletionResult]::new('update', 'update', [CompletionResultType]::ParameterValue, 'Update installed plugins')
            [CompletionResult]::new('clean', 'clean', [CompletionResultType]::ParameterValue, 'This command will clean up invalid plugins')
            [CompletionResult]::new('shortcut', 'shortcut', [CompletionResultType]::ParameterValue, 'Manage command shortcuts')
            [CompletionResult]::new('completion', 'completion', [CompletionResultType]::ParameterValue, 'Generate shell completion scripts')
            [CompletionResult]::new('theme', 'theme', [CompletionResultType]::ParameterValue, 'Interactive theme manager')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'lla;install' {
            [CompletionResult]::new('--git', 'git', [CompletionResultType]::ParameterName, 'Install a plugin from a GitHub repository URL')
            [CompletionResult]::new('--dir', 'dir', [CompletionResultType]::ParameterName, 'Install a plugin from a local directory')
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;plugin' {
            [CompletionResult]::new('-n', 'n', [CompletionResultType]::ParameterName, 'Name of the plugin')
            [CompletionResult]::new('--name', 'name', [CompletionResultType]::ParameterName, 'Name of the plugin')
            [CompletionResult]::new('-a', 'a', [CompletionResultType]::ParameterName, 'Action to perform')
            [CompletionResult]::new('--action', 'action', [CompletionResultType]::ParameterName, 'Action to perform')
            [CompletionResult]::new('-r', 'r', [CompletionResultType]::ParameterName, 'Arguments for the plugin action')
            [CompletionResult]::new('--args', 'args', [CompletionResultType]::ParameterName, 'Arguments for the plugin action')
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;list-plugins' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;use' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;init' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;config' {
            [CompletionResult]::new('--set', 'set', [CompletionResultType]::ParameterName, 'Set a configuration value (e.g., --set plugins_dir /new/path)')
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;update' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;clean' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;shortcut' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('add', 'add', [CompletionResultType]::ParameterValue, 'Add a new shortcut')
            [CompletionResult]::new('remove', 'remove', [CompletionResultType]::ParameterValue, 'Remove a shortcut')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List all shortcuts')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'lla;shortcut;add' {
            [CompletionResult]::new('-d', 'd', [CompletionResultType]::ParameterName, 'Optional description of the shortcut')
            [CompletionResult]::new('--description', 'description', [CompletionResultType]::ParameterName, 'Optional description of the shortcut')
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;shortcut;remove' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;shortcut;list' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;shortcut;help' {
            break
        }
        'lla;completion' {
            [CompletionResult]::new('-p', 'p', [CompletionResultType]::ParameterName, 'Custom installation path for the completion script')
            [CompletionResult]::new('--path', 'path', [CompletionResultType]::ParameterName, 'Custom installation path for the completion script')
            [CompletionResult]::new('-o', 'o', [CompletionResultType]::ParameterName, 'Output path for the completion script (prints to stdout if not specified)')
            [CompletionResult]::new('--output', 'output', [CompletionResultType]::ParameterName, 'Output path for the completion script (prints to stdout if not specified)')
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;theme' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('pull', 'pull', [CompletionResultType]::ParameterValue, 'Pull and install themes from the official repository')
            [CompletionResult]::new('install', 'install', [CompletionResultType]::ParameterValue, 'Install theme(s) from a file or directory')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'lla;theme;pull' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;theme;install' {
            [CompletionResult]::new('-h', 'h', [CompletionResultType]::ParameterName, 'Print help information')
            [CompletionResult]::new('--help', 'help', [CompletionResultType]::ParameterName, 'Print help information')
            break
        }
        'lla;theme;help' {
            break
        }
        'lla;help' {
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
