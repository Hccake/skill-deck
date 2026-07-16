#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  map-host)
    mapped=$(wslpath -u -- "$1") || exit $?; case "$mapped" in /*) ;; *) exit 61 ;; esac; printf '1\0%s\0' "$mapped"
    ;;
  map-storage-host)
    mapped=$(wslpath -w -- "$1") || exit $?; [ -n "$mapped" ] || exit 61; printf '1\0%s\0' "$mapped"
    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
