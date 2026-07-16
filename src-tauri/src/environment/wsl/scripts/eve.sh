#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  inspect)

    project=$1
    if [ ! -d "$project/agent" ] || [ ! -f "$project/package.json" ]; then
      printf '0\0'
      exit 0
    fi
    printf '1\0'
    cat -- "$project/package.json"
    printf '\0'
    for dir in "$project/agent/subagents"/*; do
      [ -d "$dir" ] || continue
      printf '%s\0' "${dir##*/}"
    done

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
