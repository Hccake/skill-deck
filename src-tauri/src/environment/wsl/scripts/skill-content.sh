#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  read)

    dir=$1
    [ -d "$dir" ] || exit 44
    for candidate in "$dir"/*; do
      [ -f "$candidate" ] || continue
      base=${candidate##*/}
      lower=$(printf '%s' "$base" | tr '[:upper:]' '[:lower:]')
      if [ "$lower" = 'skill.md' ]; then
        cat -- "$candidate"
        exit 0
      fi
    done
    exit 44

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
