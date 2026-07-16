#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  inspect)
    root=$1
    case "$root" in
      /*) ;;
      *) exit 64 ;;
    esac
    [ -d "$root" ] && [ ! -L "$root" ] && [ -r "$root" ] && [ -x "$root" ] || exit 66
    [ "$root" = / ] || root=${root%/}

    work_dir=$(mktemp -d "${TMPDIR:-/tmp}/skill-deck-manifest.XXXXXX") || exit 67
    trap 'rm -rf -- "$work_dir"' EXIT HUP INT TERM
    records=$work_dir/records
    counts=$work_dir/counts
    : > "$records" || exit 67
    : > "$counts" || exit 67

    if ! LC_ALL=C find "$root" -mindepth 1 -exec sh -c '
      root=$1
      records=$2
      counts=$3
      shift 3
      for path do
        relative=${path#"$root"/}
        [ "$relative" != "$path" ] || exit 68
        path_length=$(LC_ALL=C printf %s "$relative" | wc -c) || exit 69
        case "$path_length" in ""|*[!0-9]*) exit 69 ;; esac

        executable=0
        data=
        data_file=
        if [ -L "$path" ]; then
          kind=l
          data_file=$(mktemp "${records}.data.XXXXXX") || exit 70
          readlink -n -- "$path" > "$data_file" || exit 70
        elif [ -d "$path" ]; then
          kind=d
        elif [ -f "$path" ]; then
          kind=f
          digest_line=$(sha256sum -- "$path") || exit 71
          digest_line=${digest_line#\\}
          data=${digest_line%% *}
          mode=$(stat -Lc %a -- "$path") || exit 72
          case "$mode" in *[1357]*) executable=1 ;; esac
        else
          exit 73
        fi
        if [ -n "$data_file" ]; then
          data_length=$(wc -c < "$data_file") || exit 69
        else
          data_length=$(LC_ALL=C printf %s "$data" | wc -c) || exit 69
        fi
        case "$data_length" in ""|*[!0-9]*) exit 69 ;; esac
        printf "R %s %s %s %s\n" "$kind" "$executable" "$path_length" "$data_length" >> "$records" || exit 74
        printf %s "$relative" >> "$records" || exit 74
        if [ -n "$data_file" ]; then
          cat "$data_file" >> "$records" || exit 74
          rm -f -- "$data_file" || exit 74
        else
          printf %s "$data" >> "$records" || exit 74
        fi
        printf "1\n" >> "$counts" || exit 74
      done
    ' sh "$root" "$records" "$counts" {} +; then
      exit 75
    fi

    count=$(wc -l < "$counts") || exit 76
    case "$count" in ""|*[!0-9]*) exit 76 ;; esac
    printf 'SDCM 1\n'
    cat "$records" || exit 77
    printf 'E %s\n' "$count"
    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
