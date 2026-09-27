#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 EfterScript contributors
# SPDX-License-Identifier: MIT
#
# PreToolUse guard for an agent's git use. Agents may commit and push on
# a topic branch; `main` and `master` stay human-only, history is never
# rewritten, and nothing an agent writes into a commit names the tool
# that wrote it or its vendor. Inspects the full Bash command, so
# `git -C`, `cd &&`, and flag-laden variants are caught, unlike prefix
# permission rules. The git hooks in `.githooks/` enforce the same rules
# on the commit itself.

cmd=$(jq -r '.tool_input.command // empty')
[ -z "$cmd" ] && exit 0

deny() {
    jq -n --arg reason "Repo A policy: $1" '{
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: $reason
      }
    }'
    exit 0
}

# Git's own options, which may stand between `git` and its verb.
opts='([[:space:]]+(-C|-c|--git-dir|--work-tree|--namespace)[[:space:]]+[^[:space:]]+|[[:space:]]+-[^[:space:]]+)*'

# `git [options] <verb>`, anywhere in a compound command; a verb-like
# word later on, in a message or a path, does not count.
uses() {
    local pattern="(^|[^[:alnum:]_])git${opts}[[:space:]]+($1)([[:space:]]|\$)"
    [[ "$cmd" =~ $pattern ]]
}

# The text of every `git [options] <verb> ...` segment, up to the next
# command separator.
segments() {
    grep -oE "git${opts}[[:space:]]+$1([^|;&]*)" <<<"$cmd"
}

branch() {
    git -C "${CLAUDE_PROJECT_DIR:-.}" symbolic-ref --quiet --short HEAD 2>/dev/null
}

on_protected_branch() {
    case "$(branch)" in
        "" | main | master) return 0 ;;
        *) return 1 ;;
    esac
}

# History-changing operations stay with people.
if uses "merge|rebase|cherry-pick|revert|am|tag|filter-repo|filter-branch|reset[[:space:]]+--hard"; then
    deny "merge, rebase, cherry-pick, revert, am, tag, filter-repo, and hard resets are human-only."
fi
gh_pattern='(^|[^[:alnum:]_])gh[[:space:]]+pr[[:space:]]+merge([[:space:]]|$)'
if [[ "$cmd" =~ $gh_pattern ]]; then
    deny "merging a pull request is human-only."
fi

if uses "commit"; then
    commit=$(segments commit)
    if grep -qE -- '(^|[[:space:]])(--amend|--no-verify|-n|--fixup|--squash)([[:space:]=]|$)' <<<"$commit"; then
        deny "agents make new commits only: no --amend, --fixup, --squash, or --no-verify."
    fi
    if on_protected_branch; then
        deny "agents commit on a topic branch, never on main or master (or a detached HEAD). Create or switch to a branch first, in its own command."
    fi
    # The project names no products: a commit carries no tool or vendor
    # name, no generated-with tagline, and no co-author trailer for an
    # agent. Paths into the tool's own config directory are not names.
    # From the verb to the end of the command, so a message written over
    # several lines (a heredoc, `$(cat ...)`) is read whole.
    text=$(sed -n '/git.*[[:space:]]commit\([[:space:]]\|$\)/,$p' <<<"$cmd" |
        sed -E '1s/^.*[[:space:]]commit([[:space:]]|$)/ /; s#\.claude/#./#g; s#\$\{?CLAUDE_[A-Z_]*\}?##g')
    if grep -qiE 'claude|anthropic|co-authored-by|generated (with|by)|🤖' <<<"$text"; then
        deny "commit messages name no products: no tool or vendor name, no 'Generated with' tagline, no Co-Authored-By trailer."
    fi
fi

if uses "push"; then
    push=$(segments push)
    if grep -qE -- '(^|[[:space:]])(--force|--force-with-lease|--force-if-includes|-f|--mirror|--all|--delete|-d|--tags|--no-verify)([[:space:]=]|$)|[[:space:]]\+[^[:space:]]' <<<"$push"; then
        deny "agents push a topic branch forward only: no force, mirror, all, delete, tags, or --no-verify."
    fi
    if grep -qE '(^|[[:space:]:/])(main|master)([[:space:]]|$)' <<<"$push"; then
        deny "agents never push to main or master; push the topic branch and open a pull request."
    fi
    if on_protected_branch; then
        deny "the current branch is main or master (or HEAD is detached); agents push only a topic branch."
    fi
fi

exit 0
