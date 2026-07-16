#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  project-targets)

    printf '2\0'
    index=0
    for destination do
      case "$destination" in /*) ;; *) exit 61 ;; esac
      parent=${destination%/*}
      [ -n "$parent" ] || parent=/
      relative=${destination##*/}
      case "$relative" in ''|.|..) exit 62 ;; esac
      while [ ! -e "$parent" ] && [ ! -L "$parent" ]; do
        component=${parent##*/}
        case "$component" in ''|.|..) exit 63 ;; esac
        relative=$component/$relative
        next=${parent%/*}
        [ -n "$next" ] || next=/
        [ "$next" != "$parent" ] || exit 64
        parent=$next
      done
      [ -d "$parent" ] || exit 65
      resolved=$(realpath -e -- "$parent") || exit 66
      identity=$(stat -Lc '%d %i' -- "$parent") || exit 67
      storage_projection=$(wslpath -w -- "$resolved") || exit 68
      device=${identity%% *}
      inode=${identity#* }
      if [ "$resolved" = / ]; then
        physical=/$relative
      else
        physical=$resolved/$relative
      fi
      printf 'P\0%s\0%s\0%s\0%s\0%s\0%s\0' \
        "$index" "$device" "$inode" "$physical" "$relative" "$storage_projection"
      index=$((index + 1))
    done

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
