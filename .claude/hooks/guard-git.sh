#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 EfterScript contributors
# SPDX-License-Identifier: MIT
#
# PreToolUse guard: agents may stage, but history-changing git operations are
# reserved for humans. Inspects the full Bash command (so `git -C`, `cd &&`,
# and flag-laden variants are caught, unlike prefix permission rules).

cmd=$(jq -r '.tool_input.command // empty')
[ -z "$cmd" ] && exit 0

pattern='(^|[^[:alnum:]_])git[[:space:]]+([^|;&]*[[:space:]])?(commit|push|merge|rebase|cherry-pick|revert|am|tag|filter-repo)([[:space:]]|$)'
gh_pattern='(^|[^[:alnum:]_])gh[[:space:]]+pr[[:space:]]+merge([[:space:]]|$)'

if [[ "$cmd" =~ $pattern ]] || [[ "$cmd" =~ $gh_pattern ]]; then
    jq -n '{
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: "Repo A policy: agents stage, humans commit. git commit/push/merge/rebase/tag and gh pr merge are human-only — leave the changes staged and ask the user to commit."
      }
    }'
fi
exit 0
