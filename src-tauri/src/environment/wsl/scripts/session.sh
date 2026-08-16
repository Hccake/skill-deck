#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  session)
    fail() {
      printf '%s\n' "$1" >&2
      exit 69
    }

    git --version >/dev/null 2>&1 || fail 'Skill Deck requires Git in the WSL environment'
    timeout --kill-after=1s 1s /bin/sh -c 'exit 0' >/dev/null 2>&1 \
      || fail 'Skill Deck requires GNU timeout in the WSL environment'

    probe_root=${TMPDIR:-/tmp}/skill-deck-session-probe-$$
    probe_root_created=0
    if mkdir -- "$probe_root" 2>/dev/null; then
      probe_root_created=1
      trap 'rm -rf -- "$probe_root"' EXIT HUP INT TERM
      printf 'a\nb\n' > "$probe_root/xargs-expected"
      if ! printf 'a\0b\0' | xargs -0 -r -n1 printf '%s\n' > "$probe_root/xargs-actual" 2>/dev/null; then
        fail 'Skill Deck requires xargs with -0 and -r support in the WSL environment'
      fi
      if ! cmp -s "$probe_root/xargs-expected" "$probe_root/xargs-actual"; then
        fail 'Skill Deck requires xargs with -0 and -r support in the WSL environment'
      fi
      printf 'a\0B\0' > "$probe_root/sort-expected"
      if ! printf 'B\0a\0' | LC_ALL=C sort -z -f > "$probe_root/sort-actual" 2>/dev/null; then
        fail 'Skill Deck requires sort with -z and -f support in the WSL environment'
      fi
      if ! cmp -s "$probe_root/sort-expected" "$probe_root/sort-actual"; then
        fail 'Skill Deck requires sort with -z and -f support in the WSL environment'
      fi
      : > "$probe_root/sha256-empty"
    else
      fail 'Skill Deck could not create a WSL capability probe directory'
    fi
    [ "$(sha256sum -- "$probe_root/sha256-empty" 2>/dev/null | awk '{print $1}')" = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 ] \
      || fail 'Skill Deck requires sha256sum with -- support in the WSL environment'
    [ "$(readlink -f -- / 2>/dev/null)" = / ] \
      || fail 'Skill Deck requires readlink with -f and -- support in the WSL environment'
    stat -Lc '%d:%i:%f:%s:%Y:%y' -- / >/dev/null 2>&1 \
      || fail 'Skill Deck requires GNU-compatible stat in the WSL environment'
    printf '4\0'; id -un | tr -d '\n'; printf '\0'; id -u | tr -d '\n'; printf '\0'
    printf '%s\0' "$HOME" "${XDG_STATE_HOME:-}" "${XDG_CONFIG_HOME:-$HOME/.config}" "${CODEX_HOME:-}" "${CLAUDE_CONFIG_DIR:-}" "${VIBE_HOME:-}" "${HERMES_HOME:-}" "${AUTOHAND_HOME:-}" "${GROK_HOME:-}"
    if [ "$probe_root_created" = 1 ]; then
      rm -rf -- "$probe_root" 2>/dev/null || true
    fi
    trap - EXIT HUP INT TERM
    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
