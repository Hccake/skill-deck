#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  inspect)

    printf '1\0'
    index=0
    for path do
      kind=missing
      device=
      inode=
      mode=
      size=
      mtime_seconds=
      mtime_nanos=
      link_target=
      if [ -e "$path" ] || [ -L "$path" ]; then
        if [ -L "$path" ]; then
          kind=symlink
          link_target=$(readlink -- "$path") || exit 61
          [ -e "$path" ] || kind=brokenLink
        elif [ -d "$path" ]; then
          kind=directory
        elif [ -f "$path" ]; then
          kind='file'
        else
          kind=other
        fi
        device=$(stat -c %d -- "$path") || exit 62
        inode=$(stat -c %i -- "$path") || exit 63
        mode=$(stat -c %f -- "$path") || exit 64
        size=$(stat -c %s -- "$path") || exit 65
        mtime_seconds=$(stat -c %Y -- "$path") || exit 66
        mtime_text=$(stat -c %y -- "$path") || exit 67
        case "$mtime_text" in
          *.*) mtime_nanos=${mtime_text#*.}; mtime_nanos=${mtime_nanos%% *} ;;
          *) mtime_nanos=0 ;;
        esac
      fi
      printf 'S\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0' \
        "$index" "$kind" "$device" "$inode" "$mode" "$size" \
        "$mtime_seconds" "$mtime_nanos" "$link_target"
      index=$((index + 1))
    done

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
