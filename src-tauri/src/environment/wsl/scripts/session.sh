#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  session)
    probe_root=${TMPDIR:-/tmp}/skill-deck-session-probe-$$
    probe_root_created=0
    if mkdir -- "$probe_root" 2>/dev/null; then
      probe_root_created=1
      trap 'rm -rf -- "$probe_root"' EXIT HUP INT TERM
      printf 'x\0' > "$probe_root/xargs-expected"
      if printf 'x\0' | xargs -0 -r -n1 printf %s > "$probe_root/xargs-actual" 2>/dev/null \
          && cmp -s "$probe_root/xargs-expected" "$probe_root/xargs-actual"; then
        nul_xargs=1
      else
        nul_xargs=0
      fi
      printf 'a\0b\0' > "$probe_root/sort-expected"
      if printf 'b\0a\0' | LC_ALL=C sort -z -f > "$probe_root/sort-actual" 2>/dev/null \
          && cmp -s "$probe_root/sort-expected" "$probe_root/sort-actual"; then
        nul_sort=1
      else
        nul_sort=0
      fi
    else
      nul_xargs=0
      nul_sort=0
    fi
    if [ "$(printf '' | sha256sum 2>/dev/null | awk '{print $1}')" = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 ]; then sha256=1; else sha256=0; fi
    if [ "$(readlink -f / 2>/dev/null)" = / ]; then canonical_readlink=1; else canonical_readlink=0; fi
    if stat -Lc '%d:%i:%f:%s:%Y:%y' -- / >/dev/null 2>&1; then stable_stat=1; else stable_stat=0; fi
    printf '2\0'; id -un | tr -d '\n'; printf '\0'; id -u | tr -d '\n'; printf '\0'
    printf '%s\0' "$HOME" "${XDG_STATE_HOME:-}" "${XDG_CONFIG_HOME:-$HOME/.config}" "${CODEX_HOME:-}" "${CLAUDE_CONFIG_DIR:-}" "${VIBE_HOME:-}" "${HERMES_HOME:-}" "${AUTOHAND_HOME:-}"
    if command -v git >/dev/null 2>&1; then printf '1\0'; else printf '0\0'; fi
    printf '%s\0' "$nul_xargs" "$nul_sort" "$sha256" "$canonical_readlink" "$stable_stat"
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
