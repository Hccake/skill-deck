#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  inspect)

    missing_kind() {
      probe=${1%/*}
      [ -n "$probe" ] || probe=/
      while [ "$probe" != / ] && [ ! -e "$probe" ] && [ ! -L "$probe" ]; do
        next=${probe%/*}
        [ -n "$next" ] || next=/
        [ "$next" != "$probe" ] || break
        probe=$next
      done
      if [ -d "$probe" ] && [ ! -x "$probe" ]; then
        printf inaccessible
      else
        printf missing
      fi
    }

    printf '1\0'
    while [ "$#" -ge 2 ]; do
      path=$1
      inspect_content=$2
      shift 2
      if [ -L "$path" ]; then
        if [ ! -e "$path" ]; then
          kind=broken-link
        elif [ -d "$path" ]; then
          kind=symlink-directory
        else
          kind=symlink-other
        fi
      elif [ -d "$path" ]; then
        kind=directory
      elif [ -e "$path" ]; then
        kind=other
      else
        kind=$(missing_kind "$path")
      fi
      printf 'path\0%s\0%s\0' "$path" "$kind"
      if [ "$inspect_content" = 1 ] && [ -f "$path" ]; then
        if payload=$(dd if="$path" bs=1048576 count=1 2>/dev/null); then
          if [ -n "$payload" ]; then
            printf 'eve\0%s\0' "$payload"
          else
            printf 'eve-empty\0-\0'
          fi
        else
          printf 'eve-unreadable\0-\0'
        fi
      else
        printf 'none\0-\0'
      fi
    done

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
