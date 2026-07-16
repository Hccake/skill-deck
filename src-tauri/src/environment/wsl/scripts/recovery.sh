#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  enumerate)

    namespace=$1
    [ -d "$namespace" ] && [ ! -L "$namespace" ] || exit 61
    printf '1\0'
    for root in "$namespace"/skill-deck-operation-*; do
      [ -e "$root" ] || [ -L "$root" ] || continue
      status=missing
      content=
      if [ -d "$root" ] && [ ! -L "$root" ]; then
        marker=$root/recovery.json
        if [ -f "$marker" ] && [ ! -L "$marker" ]; then
          status=present
          content=$(cat -- "$marker") || status=unreadable
        fi
      else
        status=unsafe
      fi
      printf 'R\0%s\0%s\0%s\0' "$root" "$status" "$content"
    done

    ;;
  write-marker)

    namespace=$1
    resource_id=$2
    mode=$3
    case "$resource_id" in ''|*[!A-Za-z0-9_-]*) exit 61 ;; esac
    [ -d "$namespace" ] && [ ! -L "$namespace" ] || exit 62
    root=$namespace/skill-deck-operation-$resource_id
    created_root=0
    case "$mode" in
      create)
        [ ! -e "$root" ] && [ ! -L "$root" ] || exit 63
        umask 077
        mkdir -- "$root" || exit 64
        created_root=1
        printf '1\n%s\n' "$resource_id" > "$root/.skill-deck-owner" || exit 65
        ;;
      update)
        [ -d "$root" ] && [ ! -L "$root" ] || exit 63
        ;;
      *) exit 70 ;;
    esac
    owner=$root/.skill-deck-owner
    [ -f "$owner" ] && [ ! -L "$owner" ] || exit 64
    [ "$(wc -l < "$owner")" -eq 2 ] || exit 65
    [ "$(sed -n '1p' "$owner")" = 1 ] || exit 66
    [ "$(sed -n '2p' "$owner")" = "$resource_id" ] || exit 67
    marker=$root/recovery.json
    case "$mode" in
      create) [ ! -e "$marker" ] && [ ! -L "$marker" ] || exit 68 ;;
      update) [ -f "$marker" ] && [ ! -L "$marker" ] || exit 69 ;;
      *) exit 70 ;;
    esac
    tmp=$root/.recovery.$$
    trap 'rm -f -- "$tmp"; if [ "$created_root" = 1 ]; then rm -rf -- "$root"; fi' EXIT HUP INT TERM
    umask 077
    cat > "$tmp" || exit 71
    [ -s "$tmp" ] || exit 72
    sync "$tmp" 2>/dev/null || true
    mv -- "$tmp" "$marker" || exit 73
    created_root=0
    printf '1\0'
    trap - EXIT HUP INT TERM

    ;;
  remove-marker)

    namespace=$1
    resource_id=$2
    case "$resource_id" in ''|*[!A-Za-z0-9_-]*) exit 61 ;; esac
    [ -d "$namespace" ] && [ ! -L "$namespace" ] || exit 62
    root=$namespace/skill-deck-operation-$resource_id
    [ ! -e "$root" ] && [ ! -L "$root" ] && { printf '1\0'; exit 0; }
    [ -d "$root" ] && [ ! -L "$root" ] || exit 63
    owner=$root/.skill-deck-owner
    [ -f "$owner" ] && [ ! -L "$owner" ] || exit 64
    [ "$(wc -l < "$owner")" -eq 2 ] || exit 65
    [ "$(sed -n '1p' "$owner")" = 1 ] || exit 66
    [ "$(sed -n '2p' "$owner")" = "$resource_id" ] || exit 67
    rm -rf -- "$root" || exit 68
    printf '1\0'

    ;;
  cleanup)

    namespace=$1
    resource_id=$2
    shift 2
    case "$resource_id" in ''|*[!A-Za-z0-9_-]*) exit 61 ;; esac
    [ -d "$namespace" ] && [ ! -L "$namespace" ] || exit 62
    root=$namespace/skill-deck-operation-$resource_id
    [ -d "$root" ] && [ ! -L "$root" ] || exit 63
    owner=$root/.skill-deck-owner
    [ -f "$owner" ] && [ ! -L "$owner" ] || exit 64
    [ "$(wc -l < "$owner")" -eq 2 ] || exit 65
    [ "$(sed -n '1p' "$owner")" = 1 ] || exit 66
    [ "$(sed -n '2p' "$owner")" = "$resource_id" ] || exit 67
    marker=$root/recovery.json
    [ -f "$marker" ] && [ ! -L "$marker" ] || exit 68
    expected=$root/.cleanup-expected.$$
    trap 'rm -f -- "$expected"' EXIT HUP INT TERM
    umask 077
    cat > "$expected" || exit 69
    [ -s "$expected" ] || exit 70
    cmp -s -- "$expected" "$marker" || exit 71
    for backup in "$@"; do
      case "$backup" in
        /*) ;;
        *) exit 72 ;;
      esac
      case "$backup" in
        */../*|*/./*|*/..|*/.) exit 73 ;;
      esac
      name=${backup##*/}
      case "$name" in .skill-deck-backup-*) ;; *) exit 74 ;; esac
      [ "$backup" != "$root" ] || exit 75
      rm -rf -- "$backup" || exit 76
    done
    rm -f -- "$expected" || exit 77
    trap - EXIT HUP INT TERM
    rm -rf -- "$root" || exit 78
    printf '1\0'

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
