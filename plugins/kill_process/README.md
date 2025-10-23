# Kill Process Plugin

A plugin for lla that provides an interactive interface for managing and terminating system processes, inspired by Raycast's Kill Process command.

## Features

- **Live Fuzzy Search**: Real-time search that updates as you type - instantly filter 1000+ processes
- **Interactive Terminal UI**: Full-screen interface with arrow key navigation
- **List Running Processes**: Display all running processes with detailed information (PID, name, CPU%, memory usage)
- **Smart Ranking**: Fuzzy matcher ranks results by relevance score
- **Multi-Select**: Choose multiple processes to kill at once
- **Force Kill**: Option to forcefully terminate unresponsive processes
- **Cross-Platform**: Works on macOS, Linux, and Windows
- **Safety Confirmations**: Asks for confirmation before terminating any process
- **Fast & Responsive**: Handles large process lists efficiently

## Available Actions

### `list`

List all running processes with detailed information.

```bash
lla plugin --name kill_process --action list
```

### `kill`

Interactively select and kill a process with **live fuzzy search**.

```bash
lla plugin --name kill_process --action kill
```

Features:

- **Real-time fuzzy search** - Results update as you type!
- Type to filter processes instantly (e.g., "chr" matches "Chrome")
- Navigate with ↑↓ arrow keys
- Shows up to 20 matching processes with PID, CPU%, and memory usage
- Press Enter to proceed with filtered results
- Press Escape to cancel
- After filtering, interactive multi-select interface to choose processes
- Confirmation dialog before killing

Example workflow:

1. Start typing to filter processes in real-time (e.g., "firefox")
2. Use arrow keys to browse filtered results
3. Press Enter to select from filtered processes
4. Choose which processes to kill using multi-select
5. Confirm before killing

### `force-kill`

Forcefully terminate a process with **live fuzzy search** (SIGKILL on Unix, /F flag on Windows).

```bash
lla plugin --name kill_process --action force-kill
```

This is useful for unresponsive processes that don't respond to normal termination signals. Includes the same real-time fuzzy search filtering as the regular `kill` action.

### `kill-by-name`

Kill processes matching a specific name pattern.

```bash
lla plugin --name kill_process --action kill-by-name --args "chrome"
```

### `kill-by-pid`

Kill a specific process by its PID.

```bash
lla plugin --name kill_process --action kill-by-pid --args "1234"
```

### `help`

Display help information about the plugin.

```bash
lla plugin --name kill_process --action help
```

## Platform-Specific Behavior

### macOS

- Uses `kill -15` (SIGTERM) for normal termination
- Uses `kill -9` (SIGKILL) for force termination
- May require `sudo` for system processes or force kill

### Linux

- Uses `kill -15` (SIGTERM) for normal termination
- Uses `kill -9` (SIGKILL) for force termination
- May require elevated privileges for processes owned by other users

### Windows

- Uses `taskkill /PID` for normal termination
- Uses `taskkill /F /PID` for force termination
- May require administrator privileges

## Configuration

The plugin uses a configuration file located at `~/.config/lla/plugins/kill_process.toml`.

### Default Configuration

```toml
[colors]
success = "bright_green"
info = "bright_blue"
error = "bright_red"
warning = "bright_yellow"
process_name = "bright_cyan"
pid = "bright_magenta"
```

## Safety Notes

- Always double-check before killing processes, especially system processes
- Force kill should be used as a last resort as it doesn't allow processes to clean up
- Killing critical system processes may cause system instability
- Some processes may require elevated privileges (sudo/admin) to terminate

## Examples

### Kill a specific application

```bash
lla plugin --name kill_process --action kill-by-name --args "Firefox"
```

### Force kill an unresponsive process

```bash
lla plugin --name kill_process --action force-kill
# Then select the process from the interactive list
```

### List all processes

```bash
lla plugin --name kill_process --action list
```

## Troubleshooting

### "Permission denied" errors

- On macOS/Linux: Try running with `sudo` or ensure you have permission to kill the process
- On Windows: Run your terminal as Administrator

### Process still running after kill

- Some processes may take time to terminate
- Try using `force-kill` for stubborn processes
- Check if the process is a system-protected process

## License

This plugin is part of the lla project and follows the same license.
