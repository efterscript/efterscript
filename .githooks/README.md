# Git hooks — humans commit, agents stage

Versioned hooks refusing `commit` and `push` from AI-agent shells (detected
via the `CLAUDECODE` / `CLAUDE_CODE_ENTRYPOINT` environment variables Claude
Code sets). Activate once per clone:

```sh
git config core.hooksPath .githooks
```

This is one of three layers; `.claude/settings.json` denies the same
operations at the tool-permission level and runs `.claude/hooks/guard-git.sh`
as a PreToolUse guard. Neither local layer can stop a determined bypass
(`--no-verify`, unsetting the variable); the only hard guarantee is a GitHub
branch rule requiring signed commits with a key that needs physical presence
(hardware token / touch-to-sign), which an agent shell cannot satisfy.
