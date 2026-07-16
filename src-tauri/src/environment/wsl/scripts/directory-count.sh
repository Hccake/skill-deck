#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  inspect)

    limit=$1
    shift
    max=$((limit + 1))
    work_dir=$(mktemp -d "${TMPDIR:-/tmp}/skill-deck-count.XXXXXX") || exit 67
    trap 'rm -rf -- "$work_dir"' EXIT HUP INT TERM
    printf '1\0'
    index=0
    for path do
      if [ ! -d "$path" ] || [ ! -r "$path" ] || [ ! -x "$path" ]; then
        printf 'path\0%s\0none\0%s\0%s\0' "$path" 0 0
        continue
      fi
      entries="$work_dir/entries.$index"
      index=$((index + 1))
      if ! LC_ALL=C find "$path" -mindepth 1 -maxdepth 1 -print0 > "$entries" 2>/dev/null; then
        printf 'path\0%s\0none\0%s\0%s\0' "$path" 0 0
        continue
      fi
      count=$(LC_ALL=C tr -cd '\000' < "$entries" | head -c "$max" | wc -c)
      case "$count" in
        ""|*[!0-9]*) printf 'path\0%s\0none\0%s\0%s\0' "$path" 0 0; continue ;;
      esac
      truncated=0
      if [ "$count" -gt "$limit" ]; then
        count=$limit
        truncated=1
      fi
      printf 'path\0%s\0count\0%s\0%s\0' "$path" "$count" "$truncated"
    done

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
