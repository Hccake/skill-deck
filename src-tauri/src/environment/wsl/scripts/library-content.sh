#!/bin/sh
set -eu

subcommand=$1
shift
root=${HOME%/}/.skill-deck/skill-libraries
transactions=$root/.transactions

write_state() {
  target=$1
  value=$2
  temporary=$target.tmp.$$
  printf '%s' "$value" > "$temporary"
  mv -f -- "$temporary" "$target"
}

catalog_hash() {
  if [ -f "$root/catalog.json" ]; then
    sha256sum -- "$root/catalog.json" | awk '{ print $1 }'
  fi
}

rollback_content() {
  destination=$1
  backup=$2
  rm -rf -- "$destination"
  if [ -d "$backup" ]; then
    mv -- "$backup" "$destination" || exit 72
  fi
}

recover_transactions() {
  [ -d "$transactions" ] || return 0
  for transaction in "$transactions"/*; do
    [ -d "$transaction" ] || continue
    [ -f "$transaction/destination" ] && [ -f "$transaction/phase" ] || exit 75
    destination=$(cat -- "$transaction/destination")
    phase=$(cat -- "$transaction/phase")
    [ -f "$transaction/desired-presence" ] || exit 75
    desired_presence=$(cat -- "$transaction/desired-presence")
    stage=$transaction/stage
    backup=$transaction/backup
    case "$phase" in
      preparing)
        ;;
      staged)
        ;;
      backedUp)
        if [ ! -e "$destination" ] && [ -d "$backup" ]; then
          mv -- "$backup" "$destination" || exit 72
        elif [ -e "$destination" ] && [ ! -e "$backup" ]; then
          :
        elif [ -e "$destination" ] && [ -d "$backup" ] && [ ! -e "$stage" ]; then
          :
        else
          exit 73
        fi
        ;;
      activated)
        rollback_content "$destination" "$backup"
        ;;
      catalogPrepared)
        if [ "$desired_presence" = 1 ]; then
          [ -d "$destination" ] || exit 74
        else
          [ ! -e "$destination" ] || exit 74
        fi
        [ -f "$transaction/expected-catalog-hash" ] || exit 75
        expected=$(cat -- "$transaction/expected-catalog-hash")
        current=$(catalog_hash)
        if [ "$current" != "$expected" ]; then
          rollback_content "$destination" "$backup"
        fi
        ;;
      catalogCommitted)
        if [ "$desired_presence" = 1 ]; then
          [ -d "$destination" ] || exit 74
        else
          [ ! -e "$destination" ] || exit 74
        fi
        ;;
      *) exit 75 ;;
    esac
    rm -rf -- "$stage" "$backup"
    rm -rf -- "$transaction"
  done
}

mkdir -p -- "$root" "$transactions"
case "$subcommand" in
  prepare-catalog|finalize-catalog) ;;
  *) recover_transactions ;;
esac

case "$subcommand" in
  recover)
    printf '1\0'
    ;;
  ensure-libraries)
    for library_id in "$@"; do
      mkdir -p -- "$root/libraries/$library_id/skills"
    done
    printf '1\0'
    ;;
  replace)
    library_id=$1
    skill_name=$2
    operation_id=$3
    destination=$root/libraries/$library_id/skills/$skill_name
    transaction=$transactions/$operation_id
    stage=$transaction/stage
    backup=$transaction/backup
    mkdir -p -- "$transaction" "${destination%/*}"
    write_state "$transaction/destination" "$destination"
    write_state "$transaction/desired-presence" '1'
    write_state "$transaction/phase" 'preparing'
    tar -xf - -C "$transaction" --no-same-owner || exit 76
    [ -d "$stage" ] || exit 77
    [ -f "$stage/SKILL.md" ] || exit 78
    write_state "$transaction/phase" 'staged'
    if [ -e "$destination" ]; then
      write_state "$transaction/phase" 'backedUp'
      mv -- "$destination" "$backup" || exit 79
    fi
    write_state "$transaction/phase" 'activated'
    mv -- "$stage" "$destination" || exit 80
    [ -f "$destination/SKILL.md" ] || exit 81
    printf '1\0'
    ;;
  delete)
    library_id=$1
    skill_name=$2
    operation_id=$3
    destination=$root/libraries/$library_id/skills/$skill_name
    transaction=$transactions/$operation_id
    backup=$transaction/backup
    [ -e "$destination" ] || exit 85
    mkdir -p -- "$transaction"
    write_state "$transaction/destination" "$destination"
    write_state "$transaction/desired-presence" '0'
    write_state "$transaction/phase" 'preparing'
    write_state "$transaction/phase" 'backedUp'
    mv -- "$destination" "$backup" || exit 79
    write_state "$transaction/phase" 'activated'
    printf '1\0'
    ;;
  prepare-catalog)
    expected=$1
    for transaction in "$transactions"/*; do
      [ -d "$transaction" ] || continue
      [ -f "$transaction/phase" ] || continue
      if [ "$(cat -- "$transaction/phase")" = 'activated' ]; then
        write_state "$transaction/expected-catalog-hash" "$expected"
        write_state "$transaction/phase" 'catalogPrepared'
      fi
    done
    printf '1\0'
    ;;
  finalize-catalog)
    expected=$1
    current=$(catalog_hash)
    [ "$current" = "$expected" ] || exit 89
    for transaction in "$transactions"/*; do
      [ -d "$transaction" ] || continue
      [ -f "$transaction/phase" ] || continue
      if [ "$(cat -- "$transaction/phase")" = 'catalogPrepared' ]; then
        [ -f "$transaction/expected-catalog-hash" ] || exit 90
        [ "$(cat -- "$transaction/expected-catalog-hash")" = "$expected" ] || exit 91
        write_state "$transaction/phase" 'catalogCommitted'
        rm -rf -- "$transaction/backup" "$transaction"
      fi
    done
    printf '1\0'
    ;;
  remove-library)
    library_id=$1
    destination=$root/libraries/$library_id
    if [ -e "$destination" ]; then
      rm -rf -- "$destination"
    fi
    printf '1\0'
    ;;
  remove-application)
    project_id=$1
    rm -f -- "$root/applications/projects/$project_id.json"
    printf '1\0'
    ;;
  *) exit 64 ;;
esac
