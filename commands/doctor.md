---
description: Run tsm health check
---

Run the tsm health check from the workspace directory where `tsm.toml` is located:

```bash
cd "${CLAUDE_PROJECT_DIR:-/workspaces/workspace}" && ${CLAUDE_PLUGIN_ROOT}/tsm doctor
```

Show the full output to the user. If the DB is not found, check that `tsm.toml` exists in the workspace root with the correct `data_dir`.
