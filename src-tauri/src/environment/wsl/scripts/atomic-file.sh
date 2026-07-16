#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  read)
    if [ -f "$1" ]; then printf '1\01\0'; cat -- "$1"; elif [ ! -e "$1" ]; then printf '1\00\0'; else exit 66; fi
    ;;
  backup-exists)
    if [ -f "$1.bak" ]; then printf '1\01\0'; else printf '1\00\0'; fi
    ;;
  write)

    path=$1
    dir=${path%/*}
    [ "$dir" != "$path" ] || dir=.
    mkdir -p -- "$dir" || exit 67
    tmp=$(mktemp "$dir/.skill-deck-document.XXXXXX") || exit 68
    backup_tmp=
    trap 'rm -f -- "$tmp" ${backup_tmp:+"$backup_tmp"}' EXIT HUP INT TERM
    cat > "$tmp" || exit 69
    sync "$tmp" 2>/dev/null || exit 70
    if [ -f "$path" ]; then
      backup_tmp=$(mktemp "$dir/.skill-deck-backup.XXXXXX") || exit 71
      cat -- "$path" > "$backup_tmp" || exit 72
      sync "$backup_tmp" 2>/dev/null || exit 73
      mv -f -- "$backup_tmp" "$path.bak" || exit 74
      backup_tmp=
    else
      rm -f -- "$path.bak" || exit 75
    fi
    sync "$dir" 2>/dev/null || exit 76
    mv -f -- "$tmp" "$path" || exit 77
    tmp=
    sync "$path" 2>/dev/null || exit 78
    sync "$dir" 2>/dev/null || exit 79
    printf '1\0'
    trap - EXIT HUP INT TERM

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
