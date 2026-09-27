# Git hooks — agents commit on branches, people own main

Versioned hooks for commits and pushes made from AI-agent shells
(recognised by the `CLAUDECODE` / `CLAUDE_CODE_ENTRYPOINT` environment
variables the coding tool sets). Activate once per clone:

```sh
git config core.hooksPath .githooks
```

- `pre-commit`: an agent commits on a topic branch, never on `main` or
  `master` or a detached HEAD.
- `commit-msg`: no commit message or author/committer identity, from anyone,
  names the coding tool or its vendor; an agent's commit also carries no generated-with tagline and
  no co-author trailer.
- `pre-push`: an agent pushes a topic branch forward only — never to `main`
  or `master`, never tags, deletions, or rewrites.

Merging into `main`, tagging, and releasing stay with people.

This is one of three layers; `.claude/settings.json` denies the
history-changing operations at the tool-permission level and runs
`.claude/hooks/guard-git.sh` as a PreToolUse guard with the same rules.
Neither local layer can stop a determined bypass (`--no-verify`, unsetting
the variable); the hard guarantee for `main` is a GitHub branch rule that
requires pull requests and approving review.
