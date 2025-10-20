# lla Google Meet Plugin

Google Meet plugin for `lla` that creates meeting rooms and manages links with browser profile support.

## Features

- **Quick Meeting Creation**: Instantly create Google Meet rooms
- **Clipboard Integration**: Auto-copy meeting links to clipboard
- **Browser Profiles**: Create meetings with specific browser profiles
- **History Management**: Track and reuse meeting links

## Usage

```bash
# Create a new meeting room
lla plugin --name google_meet --action create

# Create meeting with browser profile
lla plugin --name google_meet --action create-with-profile

# Manage meeting history
lla plugin --name google_meet --action history

# Manage browser profiles
lla plugin --name google_meet --action profiles

# Configure preferences
lla plugin --name google_meet --action preferences

# Show help
lla plugin --name google_meet --action help
```

## Configuration

Config location: `~/.config/lla/plugins/google_meet/config.toml`

```toml
auto_copy_link = true           # Auto-copy links to clipboard
max_history_size = 50          # Maximum history entries

[[browser_profiles]]
name = "Work"
profile_path = "Profile 1"

[[browser_profiles]]
name = "Personal"
profile_path = "Default"

[colors]
success = "bright_green"
info = "bright_cyan"
warning = "bright_yellow"
error = "bright_red"
link = "bright_blue"
```

## Display Examples

Creating Meeting:

```
ℹ Info: Creating Google Meet room...
🔗 Link: https://meet.google.com/abc-defg-hij
✓ Success: Link copied to clipboard!
✓ Success: Meeting room created successfully!
```

History Management:

```
⚡ Choose action
> 📋 Copy selected link
  🌐 Open selected link
  🗑️  Clear all history

🔗 https://meet.google.com/abc-defg-hij (2025-10-20 15:30:45)
🔗 https://meet.google.com/xyz-uvwx-stu (2025-10-20 14:20:30)
```
