#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  session)
    user=$(id -un) || exit 69
    uid=$(id -u) || exit 69
    printf '4\0%s\0%s\0' "$user" "$uid"
    printf '%s\0' "$HOME" "${XDG_STATE_HOME:-}" "${XDG_CONFIG_HOME:-$HOME/.config}" "${CODEX_HOME:-}" "${CLAUDE_CONFIG_DIR:-}" "${VIBE_HOME:-}" "${HERMES_HOME:-}" "${AUTOHAND_HOME:-}" "${GROK_HOME:-}"
    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
