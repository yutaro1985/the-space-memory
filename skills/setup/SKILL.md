---
name: setup
description: Interactive setup wizard to create or update tsm.toml configuration.
user-invocable: true
---

# Setup — tsm.toml Configuration Wizard

Help the user create or update `tsm.toml` in their project root.

## Process

1. Check if `tsm.toml` already exists in `$CLAUDE_PROJECT_DIR`
2. Ask the user about their workspace layout
3. Generate a `tsm.toml` configuration

## Key Configuration Options

| Field | Description | Default |
|---|---|---|
| `index_root` | Root directory containing workspaces to index | `/workspaces` |
| `state_dir` | Directory for DB, logs, PID files | `.tsm` |
| `search_fallback` | Behavior when embedder is down (`error` or `fts_only`) | `error` |
| `embedder_idle_timeout_secs` | Auto-stop embedder after N seconds idle | `600` |
| `[index] content_dirs` | List of directories to index with weights | auto-discover |
| `[index.claude_session] weight` | Score weight for session data | `0.3` |
| `[index.claude_session] half_life_days` | Time decay for session data | `30.0` |

## content_dirs Format

```toml
[index]
content_dirs = [
    { path = "/workspaces/notes", weight = 1.0, half_life_days = 90.0 },
    { path = "/workspaces/docs",  weight = 0.8, half_life_days = 180.0 },
]
```

- `path` — Directory to index (required)
- `weight` — Scoring weight, higher = more important (default: 1.0)
- `half_life_days` — Time decay half-life in days (default: 90.0)

## Reference

See `tsm.toml.sample` in the plugin root for full configuration reference.

## After Setup

Suggest the user run:

```bash
tsm rebuild --force   # Initial index build
tsm doctor            # Verify installation
```
