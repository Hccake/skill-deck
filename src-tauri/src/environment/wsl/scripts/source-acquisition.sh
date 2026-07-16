#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  cleanup)
    root=$1
    case "$root" in /tmp/skill-deck-discovery-*/repo) ;; *) exit 64 ;; esac
    parent=${root%/repo}
    case "$parent" in /tmp/skill-deck-discovery-*) ;; *) exit 64 ;; esac
    rm -rf -- "$parent"
    ;;
  git)
    url=$1
    dest=$2
    git_ref=$3
    distro=$4
    case "$dest" in /tmp/skill-deck-discovery-*/repo) ;; *) exit 65 ;; esac
    parent=${dest%/repo}
    case "$parent" in /tmp/skill-deck-discovery-*) ;; *) exit 65 ;; esac
    [ ! -e "$parent" ] && [ ! -L "$parent" ] || exit 66
    command -v git >/dev/null 2>&1 || {
      printf "Git is not available in WSL distro '%s'. Please install Git in that distro and try again.\n" "$distro" >&2
      exit 127
    }
    mkdir -- "$parent" || exit 67
    cleanup_parent=1
    trap '[ "$cleanup_parent" = 0 ] || rm -rf -- "$parent"' EXIT HUP INT TERM
    if [ -n "$git_ref" ]; then
      git clone --depth 1 --progress --branch "$git_ref" -- "$url" "$dest" || exit 68
    else
      git clone --depth 1 --progress -- "$url" "$dest" || exit 68
    fi
    ref_revision=$(git -C "$dest" rev-parse --verify HEAD) || exit 69
    case "${#ref_revision}" in 40|64) ;; *) exit 69 ;; esac
    case "$ref_revision" in *[!0-9a-f]*) exit 69 ;; esac
    cleanup_parent=0
    trap - EXIT HUP INT TERM
    printf '1\0%s\0' "$ref_revision" || exit 70
    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
