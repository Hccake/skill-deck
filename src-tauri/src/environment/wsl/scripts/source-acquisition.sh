#!/bin/sh
LC_ALL=C
GIT_ALLOW_PROTOCOL=https:http:ssh:git:file
export LC_ALL GIT_ALLOW_PROTOCOL

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
    git_timeout_secs=$5
    proxy_mode=$6
    proxy_url=$7
    case "$dest" in /tmp/skill-deck-discovery-*/repo) ;; *) exit 65 ;; esac
    parent=${dest%/repo}
    case "$parent" in /tmp/skill-deck-discovery-*) ;; *) exit 65 ;; esac
    [ ! -e "$parent" ] && [ ! -L "$parent" ] || exit 66
    command -v git >/dev/null 2>&1 || {
      printf "Git is not available in WSL distro '%s'. Please install Git in that distro and try again.\n" "$distro" >&2
      exit 127
    }
    command -v timeout >/dev/null 2>&1 || {
      printf "GNU timeout is not available in WSL distro '%s'. Please install GNU coreutils in that distro and try again.\n" "$distro" >&2
      exit 127
    }
    case "$git_timeout_secs" in ''|*[!0-9]*) exit 65 ;; esac
    case "$proxy_mode" in inject|preserve) ;; *) exit 65 ;; esac
    mkdir -- "$parent" || exit 67
    cleanup_parent=1
    trap '[ "$cleanup_parent" = 0 ] || rm -rf -- "$parent"' EXIT HUP INT TERM
    if [ "$proxy_mode" = inject ] && [ -n "$git_ref" ]; then
      timeout --kill-after=2s "${git_timeout_secs}s" \
        git -c "http.proxy=$proxy_url" clone --depth 1 --progress --branch "$git_ref" -- "$url" "$dest"
    elif [ "$proxy_mode" = inject ]; then
      timeout --kill-after=2s "${git_timeout_secs}s" \
        git -c "http.proxy=$proxy_url" clone --depth 1 --progress -- "$url" "$dest"
    elif [ -n "$git_ref" ]; then
      timeout --kill-after=2s "${git_timeout_secs}s" \
        git clone --depth 1 --progress --branch "$git_ref" -- "$url" "$dest"
    else
      timeout --kill-after=2s "${git_timeout_secs}s" \
        git clone --depth 1 --progress -- "$url" "$dest"
    fi
    clone_status=$?
    case "$clone_status" in 0) ;; 124|137) exit 72 ;; *) exit 68 ;; esac
    ref_revision=$(git -C "$dest" rev-parse --verify HEAD) || exit 69
    case "${#ref_revision}" in 40|64) ;; *) exit 69 ;; esac
    case "$ref_revision" in *[!0-9a-f]*) exit 69 ;; esac
    cleanup_parent=0
    trap - EXIT HUP INT TERM
    printf '1\0%s\0' "$ref_revision" || exit 70
    ;;
  git-probe)
    url=$1
    distro=$2
    git_timeout_secs=$3
    proxy_mode=$4
    proxy_url=$5
    command -v git >/dev/null 2>&1 || {
      printf "Git is not available in WSL distro '%s'. Please install Git in that distro and try again.\n" "$distro" >&2
      exit 127
    }
    command -v timeout >/dev/null 2>&1 || exit 127
    case "$git_timeout_secs" in ''|*[!0-9]*) exit 65 ;; esac
    case "$proxy_mode" in inject|preserve) ;; *) exit 65 ;; esac
    if [ "$proxy_mode" = inject ]; then
      ref_revision=$(timeout --kill-after=2s "${git_timeout_secs}s" \
        git -c "http.proxy=$proxy_url" ls-remote --exit-code -- "$url" HEAD)
    else
      ref_revision=$(timeout --kill-after=2s "${git_timeout_secs}s" \
        git ls-remote --exit-code -- "$url" HEAD)
    fi
    probe_status=$?
    case "$probe_status" in 0) ;; 124|137) exit 72 ;; *) exit 68 ;; esac
    ref_revision=${ref_revision%%[[:space:]]*}
    case "${#ref_revision}" in 40|64) ;; *) exit 69 ;; esac
    case "$ref_revision" in *[!0-9a-f]*) exit 69 ;; esac
    printf '1\0%s\0' "$ref_revision" || exit 70
    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
