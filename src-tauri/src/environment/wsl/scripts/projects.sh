#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  project-storage)
    printf '1\0'; for path do if mapped=$(wslpath -w -- "$path" 2>/dev/null); then printf 'ok\0%s\0' "$mapped"; else printf 'error\0\0'; fi; done
    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
