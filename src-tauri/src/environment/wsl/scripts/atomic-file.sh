#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  read)
    if [ -f "$1" ]; then printf '1\01\0'; cat -- "$1"; elif [ ! -e "$1" ]; then printf '1\00\0'; else exit 66; fi
    ;;
  write)

    path=$1
    dir=${path%/*}
    [ "$dir" != "$path" ] || dir=.
    mkdir -p -- "$dir" || exit 67
    tmp=$(mktemp "$dir/.skill-deck-document.XXXXXX") || exit 68
    trap 'rm -f -- "$tmp"' EXIT HUP INT TERM
    cat > "$tmp" || exit 69
    sync "$tmp" 2>/dev/null || exit 70
    rm -f -- "$path.bak" || exit 71
    sync "$dir" 2>/dev/null || exit 72
    mv -f -- "$tmp" "$path" || exit 73
    tmp=
    sync "$path" 2>/dev/null || exit 74
    sync "$dir" 2>/dev/null || exit 75
    printf '1\0'
    trap - EXIT HUP INT TERM

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
