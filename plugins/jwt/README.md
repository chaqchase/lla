# lla JWT Plugin

JWT decoder and analyzer for `lla` with beautiful formatting, search capabilities, and validation.

## Features

- **Token Decoding**: Beautiful formatted display of JWT header, payload, and signature
- **Expiration Checking**: Automatic expiration validation with time remaining
- **Smart Search**: Regex-powered search through JWT contents
- **History Management**: Store and manage decoded JWT tokens
- **Multiple Input Methods**: Paste, clipboard, or select from history
- **Flexible Copying**: Copy full token, header, payload, or specific claims
- **Claim Highlighting**: Automatically highlight important JWT claims

## Usage

```bash
# Decode and view JWT token
lla plugin run jwt decode

# Search through JWT contents
lla plugin run jwt search

# Manage JWT history
lla plugin run jwt history

# Configure preferences
lla plugin run jwt preferences

# Show help
lla plugin run jwt help
```

## Configuration

Config location: `~/.config/lla/plugins/jwt/config.toml`

```toml
auto_check_expiration = true           # Automatically check token expiration
save_to_history = true                 # Save decoded tokens to history
max_history_size = 50                  # Maximum number of tokens in history
highlight_claims = ["sub", "iss", "aud", "exp", "iat", "nbf"]  # Claims to highlight

[colors]
success = "bright_green"
info = "bright_cyan"
warning = "bright_yellow"
error = "bright_red"
header = "bright_blue"
payload = "bright_magenta"
claim = "bright_green"
expired = "bright_red"
valid = "bright_green"
```

## Display Examples

Token Display:

```
═══════════════════════════════════════════════════════════════════════════════
  🔐 JWT TOKEN DETAILS
═══════════════════════════════════════════════════════════════════════════════

┌─ HEADER ─────────────────────────────────────────────────────────────┐
{
  alg: "HS256",
  typ: "JWT"
}
└──────────────────────────────────────────────────────────────────────┘

┌─ PAYLOAD ────────────────────────────────────────────────────────────┐
{
  sub: "1234567890",
  name: "John Doe",
  iat: 1516239022,
  exp: 1735689600
}
└──────────────────────────────────────────────────────────────────────┘

✅ Expires in 45 days

┌─ SIGNATURE ──────────────────────────────────────────────────────────┐
  SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c
└──────────────────────────────────────────────────────────────────────┘
```

Search Results:

```
═══════════════════════════════════════════════════════════════════════════════
🔍 Found 3 matches for 'user'
═══════════════════════════════════════════════════════════════════════════════

► payload » userId
  "12345"

► payload » username
  "john.doe"

► payload » user_role
  "admin"
```
