#!/bin/sh

fingerprint_content() {
  root=$1
  [ -d "$root" ] && [ ! -L "$root" ] || return 1
  {
    printf 'skill-deck-wsl-content-v1\0'
    LC_ALL=C find "$root" -mindepth 1 -print0 | LC_ALL=C sort -z | xargs -0 -r -n1 /bin/sh -c '
      root=$1
      path=$2
      relative=${path#"$root"/}
      [ "$relative" != "$path" ] || exit 1
      if [ -L "$path" ]; then
        printf "l\0%s\0" "$relative"
        readlink -n -- "$path" || exit 1
        printf "\0"
      elif [ -d "$path" ]; then
        printf "d\0%s\0" "$relative"
      elif [ -f "$path" ]; then
        digest_line=$(sha256sum -- "$path") || exit 1
        digest_line=${digest_line#\\}
        mode=$(stat -Lc %a -- "$path") || exit 1
        executable=0
        case "$mode" in *[1357]*) executable=1 ;; esac
        printf "f\0%s\0%s\0%s\0" "$relative" "${digest_line%% *}" "$executable"
      else
        exit 1
      fi
    ' sh "$root"
  } | sha256sum | { read -r digest _; printf %s "$digest"; }
}

subcommand=$1
shift
case "$subcommand" in
  stage)
    operation_root=$1
    operation_id=$2
    case "$operation_id" in ''|*[!A-Za-z0-9_-]*) exit 61 ;; esac
    [ -d "$operation_root" ] && [ ! -L "$operation_root" ] || exit 62
    owner=$operation_root/.skill-deck-owner
    [ -f "$owner" ] && [ ! -L "$owner" ] || exit 63
    [ "$(wc -l < "$owner")" -eq 2 ] || exit 64
    [ "$(sed -n '1p' "$owner")" = 1 ] || exit 65
    [ "$(sed -n '2p' "$owner")" = "$operation_id" ] || exit 66
    [ -f "$operation_root/recovery.json" ] && [ ! -L "$operation_root/recovery.json" ] || exit 67
    request_root=$operation_root/request
    [ ! -e "$request_root" ] && [ ! -L "$request_root" ] || exit 68
    umask 077
    mkdir -- "$request_root" || exit 69

    cleanup_stage() {
      for entry_root in "$operation_root"/entry-*; do
        if [ ! -d "$entry_root" ] || [ -L "$entry_root" ]; then continue; fi
        if [ -f "$entry_root/destination" ] && [ ! -L "$entry_root/destination" ]; then
          destination=$(cat -- "$entry_root/destination") || destination=
          parent=${destination%/*}
          [ -n "$parent" ] || parent=/
          rm -rf -- "$parent/.skill-deck-probe-$operation_id-"* "$parent/.skill-deck-probe-renamed-$operation_id-"*
        fi
        if [ -f "$entry_root/stage" ] && [ ! -L "$entry_root/stage" ]; then
          stage_path=$(cat -- "$entry_root/stage") || stage_path=
          if [ -n "$stage_path" ]; then
            name=${stage_path##*/}
            case "$name" in .skill-deck-stage-$operation_id-*) rm -rf -- "$stage_path" ;; esac
          fi
        fi
        rm -rf -- "$entry_root"
      done
      rm -rf -- "$request_root"
    }
    trap cleanup_stage EXIT HUP INT TERM
    export request_root

    # shellcheck disable=SC2016
    if ! xargs -0 -r -n7 /bin/sh -c '
      [ "$#" -eq 7 ] || exit 255
      tag=$1
      second=$2
      third=$3
      fourth=$4
      fifth=$5
      sixth=$6
      seventh=$7
      case "$tag" in
        H)
          [ "$second" = 1 ] || exit 255
          for value in "$third" "$fourth"; do
            case "$value" in ""|*[!0-9]*) exit 255 ;; esac
          done
          [ -z "$fifth" ] && [ -z "$sixth" ] && [ -z "$seventh" ] || exit 255
          [ ! -e "$request_root/header" ] || exit 255
          printf "%s\n%s\n" "$third" "$fourth" > "$request_root/header" || exit 255
          ;;
        E)
          [ -f "$request_root/header" ] || exit 255
          case "$second" in ??????) ;; *) exit 255 ;; esac
          case "$second" in *[!0-9]*) exit 255 ;; esac
          case "$third" in /*) ;; *) exit 255 ;; esac
          case "$fourth" in keep|materialize|symlink|remove) ;; *) exit 255 ;; esac
          case "$sixth" in entry-v1-*) ;; *) exit 255 ;; esac
          case "$seventh" in ""|*[!0-9]*) exit 255 ;; esac
          case "$fourth" in
            keep|remove) [ -z "$fifth" ] && [ "$seventh" = 0 ] || exit 255 ;;
            symlink) case "$fifth" in ""|/*|*\\*) exit 255 ;; esac; [ "$seventh" = 0 ] || exit 255 ;;
            materialize) case "$fifth" in /*) ;; *) exit 255 ;; esac ;;
          esac
          entry=$request_root/entry-$second
          [ ! -e "$entry" ] && mkdir -- "$entry" || exit 255
          printf %s "$third" > "$entry/destination" || exit 255
          printf %s "$fourth" > "$entry/action" || exit 255
          printf %s "$fifth" > "$entry/source" || exit 255
          printf %s "$sixth" > "$entry/expected" || exit 255
          printf %s "$seventh" > "$entry/manifest-count" || exit 255
          : > "$entry/manifest"
          : > "$entry/manifest-seen"
          ;;
        M)
          [ -f "$request_root/header" ] || exit 255
          entry=$request_root/entry-$second
          [ -d "$entry" ] && [ ! -L "$entry" ] || exit 255
          case "$third" in directory|file) ;; *) exit 255 ;; esac
          case "$fourth" in ""|/*|*\\*|..|../*|*/..|*/../*|.|./*|*/.|*/./*|*//* ) exit 255 ;; esac
          case "$third" in
            directory) [ -z "$fifth" ] && [ "$sixth" = 0 ] && [ "$seventh" = 0 ] || exit 255 ;;
            file)
              [ "${#fifth}" -eq 64 ] || exit 255
              case "$fifth" in *[!0-9a-f]*) exit 255 ;; esac
              case "$sixth" in 0|1) ;; *) exit 255 ;; esac
              case "$seventh" in ""|*[!0-9]*) exit 255 ;; esac
              ;;
          esac
          manifest_key=$(printf %s "$fourth" | sha256sum) || exit 255
          manifest_key=${manifest_key%% *}
          [ ! -e "$entry/manifest-key-$manifest_key" ] || exit 255
          : > "$entry/manifest-key-$manifest_key" || exit 255
          printf "%s\0%s\0%s\0%s\0%s\0" "$third" "$fourth" "$fifth" "$sixth" "$seventh" >> "$entry/manifest" || exit 255
          printf "1\n" >> "$entry/manifest-seen" || exit 255
          ;;
        *) exit 255 ;;
      esac
      printf "1\n" >> "$request_root/records-seen" || exit 255
    ' sh; then
      exit 70
    fi

    [ -f "$request_root/header" ] || exit 71
    expected_records=$(sed -n '1p' "$request_root/header") || exit 72
    expected_entries=$(sed -n '2p' "$request_root/header") || exit 73
    actual_records=$(wc -l < "$request_root/records-seen") || exit 74
    [ "$actual_records" = "$expected_records" ] || exit 75
    actual_entries=$(find "$request_root" -mindepth 1 -maxdepth 1 -type d -name 'entry-*' | wc -l) || exit 76
    [ "$actual_entries" = "$expected_entries" ] || exit 77

    entry_number=0
    while [ "$entry_number" -lt "$expected_entries" ]; do
      entry_index=$(printf '%06d' "$entry_number") || exit 78
      request_entry=$request_root/entry-$entry_index
      [ -d "$request_entry" ] && [ ! -L "$request_entry" ] || exit 79
      expected_manifest=$(cat -- "$request_entry/manifest-count") || exit 80
      actual_manifest=$(wc -l < "$request_entry/manifest-seen") || exit 81
      [ "$actual_manifest" = "$expected_manifest" ] || exit 82

      destination=$(cat -- "$request_entry/destination") || exit 83
      action=$(cat -- "$request_entry/action") || exit 84
      source=$(cat -- "$request_entry/source") || exit 85
      expected=$(cat -- "$request_entry/expected") || exit 86
      parent=${destination%/*}
      [ -n "$parent" ] || parent=/
      mkdir -p -- "$parent" || exit 87
      [ -d "$parent" ] && [ ! -L "$parent" ] || exit 88
      parent_identity=$(stat -Lc '%d:%i' -- "$parent") || exit 89
      entry_root=$operation_root/entry-$entry_index
      [ ! -e "$entry_root" ] && [ ! -L "$entry_root" ] || exit 90
      mkdir -- "$entry_root" || exit 91
      stage_path=$parent/.skill-deck-stage-$operation_id-$entry_index
      backup=$parent/.skill-deck-backup-$operation_id-$entry_index
      [ ! -e "$stage_path" ] && [ ! -L "$stage_path" ] || exit 92
      [ ! -e "$backup" ] && [ ! -L "$backup" ] || exit 93
      printf %s "$destination" > "$entry_root/destination" || exit 94
      printf %s "$action" > "$entry_root/action" || exit 95
      printf %s "$expected" > "$entry_root/expected" || exit 96
      printf %s "$stage_path" > "$entry_root/stage" || exit 97
      printf %s "$backup" > "$entry_root/backup" || exit 98
      printf %s "$parent_identity" > "$entry_root/parent-identity" || exit 99
      printf %s "$source" > "$entry_root/source" || exit 100
      printf %s "$expected_manifest" > "$entry_root/manifest-count" || exit 101
      cp -- "$request_entry/manifest" "$entry_root/manifest" || exit 102
      if [ -d "$destination" ] && [ ! -L "$destination" ]; then
        fingerprint_content "$destination" > "$entry_root/expected-content" || exit 122
      fi

      case "$action" in
        keep|remove)
          printf %s '' > "$entry_root/stage" || exit 103
          ;;
        symlink)
          ln -s -- "$source" "$stage_path" || exit 104
          [ -L "$stage_path" ] || exit 105
          [ "$(readlink -- "$stage_path")" = "$source" ] || exit 106
          ;;
        materialize)
          [ -d "$source/blobs" ] && [ ! -L "$source" ] && [ ! -L "$source/blobs" ] || exit 107
          mkdir -- "$stage_path" || exit 108
          export stage_path source
          # shellcheck disable=SC2016
          if ! xargs -0 -r -n5 /bin/sh -c '
            [ "$#" -eq 5 ] || exit 255
            kind=$1
            relative=$2
            blob_id=$3
            executable=$4
            expected_size=$5
            path=$stage_path/$relative
            case "$kind" in
              directory) mkdir -p -- "$path" || exit 255 ;;
              file)
                blob=$source/blobs/$blob_id
                [ -f "$blob" ] && [ ! -L "$blob" ] || exit 255
                [ "$(wc -c < "$blob")" = "$expected_size" ] || exit 255
                digest_line=$(sha256sum -- "$blob") || exit 255
                [ "${digest_line%% *}" = "$blob_id" ] || exit 255
                mkdir -p -- "${path%/*}" || exit 255
                cp -- "$blob" "$path" || exit 255
                if [ "$executable" = 1 ]; then chmod +x -- "$path"; else chmod -x -- "$path"; fi || exit 255
                ;;
              *) exit 255 ;;
            esac
          ' sh < "$request_entry/manifest"; then
            exit 109
          fi
          export stage_path
          # shellcheck disable=SC2016
          if ! xargs -0 -r -n5 /bin/sh -c '
            [ "$#" -eq 5 ] || exit 255
            kind=$1
            relative=$2
            blob_id=$3
            executable=$4
            expected_size=$5
            path=$stage_path/$relative
            case "$kind" in
              directory) [ -d "$path" ] && [ ! -L "$path" ] || exit 255 ;;
              file)
                [ -f "$path" ] && [ ! -L "$path" ] || exit 255
                [ "$(wc -c < "$path")" = "$expected_size" ] || exit 255
                digest_line=$(sha256sum -- "$path") || exit 255
                [ "${digest_line%% *}" = "$blob_id" ] || exit 255
                if [ "$executable" = 1 ]; then [ -x "$path" ]; else [ ! -x "$path" ]; fi || exit 255
                ;;
              *) exit 255 ;;
            esac
          ' sh < "$request_entry/manifest"; then
            exit 110
          fi
          actual_stage_entries=$(find "$stage_path" -mindepth 1 -printf . | wc -c) || exit 111
          [ "$actual_stage_entries" = "$expected_manifest" ] || exit 112
          ;;
        *) exit 113 ;;
      esac

      probe=$parent/.skill-deck-probe-$operation_id-$entry_index
      renamed_probe=$parent/.skill-deck-probe-renamed-$operation_id-$entry_index
      [ ! -e "$probe" ] && [ ! -L "$probe" ] || exit 114
      [ ! -e "$renamed_probe" ] && [ ! -L "$renamed_probe" ] || exit 115
      mkdir -- "$probe" || exit 116
      printf 'stage-preflight-v1\n' > "$probe/.skill-deck-owner" || exit 117
      mv -- "$probe" "$renamed_probe" || { rm -rf -- "$probe" "$renamed_probe"; exit 118; }
      mv -- "$renamed_probe" "$probe" || { rm -rf -- "$probe" "$renamed_probe"; exit 119; }
      rm -rf -- "$probe" || exit 120
      entry_number=$((entry_number + 1))
    done

    rm -rf -- "$request_root" || exit 121
    printf '1\0'
    trap - EXIT HUP INT TERM
    ;;
  swap)

    validate_operation() {
      operation_root=$1
      operation_id=$2
      case "$operation_id" in ''|*[!A-Za-z0-9_-]*) return 1 ;; esac
      [ -d "$operation_root" ] && [ ! -L "$operation_root" ] || return 1
      marker=$operation_root/.skill-deck-owner
      [ -f "$marker" ] && [ ! -L "$marker" ] || return 1
      [ "$(wc -l < "$marker")" -eq 2 ] || return 1
      [ "$(sed -n '1p' "$marker")" = 1 ] || return 1
      [ "$(sed -n '2p' "$marker")" = "$operation_id" ] || return 1
    }

    fingerprint_entry() {
      path=$1
      if [ ! -e "$path" ] && [ ! -L "$path" ]; then
        printf %s entry-v1-missing
        return 0
      fi
      link_target=
      if [ -L "$path" ]; then link_target=$(readlink -- "$path") || return 1; fi
      device=$(stat -c %d -- "$path") || return 1
      inode=$(stat -c %i -- "$path") || return 1
      mode=$(stat -c %f -- "$path") || return 1
      size=$(stat -c %s -- "$path") || return 1
      mtime_seconds=$(stat -c %Y -- "$path") || return 1
      mtime_text=$(stat -c %y -- "$path") || return 1
      case "$mtime_text" in
        *.*) mtime_nanos=${mtime_text#*.}; mtime_nanos=${mtime_nanos%% *} ;;
        *) mtime_nanos=0 ;;
      esac
      digest_line=$({
        printf 'skill-deck-wsl-entry-v1\0'
        for value in "$device" "$inode" "$mode" "$size" "$mtime_seconds" "$mtime_nanos"; do
          printf '%s\0' "$value"
        done
        printf %s "$link_target"
      } | sha256sum) || return 1
      printf 'entry-v1-%s' "${digest_line%% *}"
    }

    remove_no_follow() {
      path=$1
      if [ -L "$path" ] || [ -f "$path" ]; then rm -f -- "$path"
      elif [ -d "$path" ]; then rm -rf -- "$path"
      elif [ -e "$path" ]; then rm -f -- "$path"
      fi
    }

    restore_all() {
      for entry_root in $(find "$operation_root" -mindepth 1 -maxdepth 1 -type d -name 'entry-*' | LC_ALL=C sort -r); do
        destination=$(cat -- "$entry_root/destination") || return 1
        backup=$(cat -- "$entry_root/backup") || return 1
        if [ -f "$entry_root/installed" ]; then
          remove_no_follow "$destination" || return 1
          rm -f -- "$entry_root/installed"
        fi
        if [ -f "$entry_root/backed-up" ]; then
          mv -- "$backup" "$destination" || return 1
          rm -f -- "$entry_root/backed-up"
        fi
      done
    }

    validate_stage() {
      entry_root=$1
      action=$(cat -- "$entry_root/action") || return 1
      stage_path=$(cat -- "$entry_root/stage") || return 1
      case "$action" in
        keep|remove) [ -z "$stage_path" ] || return 1 ;;
        symlink)
          expected_target=$(cat -- "$entry_root/source") || return 1
          case "$expected_target" in ""|/*|*\\*) return 1 ;; esac
          [ -L "$stage_path" ] || return 1
          [ "$(readlink -- "$stage_path")" = "$expected_target" ] || return 1
          ;;
        materialize)
          [ -d "$stage_path" ] && [ ! -L "$stage_path" ] || return 1
          export stage_path
          # shellcheck disable=SC2016
          xargs -0 -r -n5 /bin/sh -c '
            [ "$#" -eq 5 ] || exit 255
            kind=$1
            relative=$2
            blob_id=$3
            executable=$4
            expected_size=$5
            path=$stage_path/$relative
            case "$kind" in
              directory) [ -d "$path" ] && [ ! -L "$path" ] || exit 255 ;;
              file)
                [ -f "$path" ] && [ ! -L "$path" ] || exit 255
                [ "$(wc -c < "$path")" = "$expected_size" ] || exit 255
                digest_line=$(sha256sum -- "$path") || exit 255
                [ "${digest_line%% *}" = "$blob_id" ] || exit 255
                if [ "$executable" = 1 ]; then [ -x "$path" ]; else [ ! -x "$path" ]; fi || exit 255
                ;;
              *) exit 255 ;;
            esac
          ' sh < "$entry_root/manifest" || return 1
          expected_count=$(cat -- "$entry_root/manifest-count") || return 1
          actual_count=$(find "$stage_path" -mindepth 1 -printf . | wc -c) || return 1
          [ "$actual_count" = "$expected_count" ] || return 1
          ;;
        *) return 1 ;;
      esac
    }

    preflight_atomic_replace() {
      parent=$1
      entry_name=$2
      probe=$parent/.skill-deck-probe-$operation_id-$entry_name
      renamed_probe=$parent/.skill-deck-probe-renamed-$operation_id-$entry_name
      [ ! -e "$probe" ] && [ ! -L "$probe" ] || return 1
      [ ! -e "$renamed_probe" ] && [ ! -L "$renamed_probe" ] || return 1
      mkdir -- "$probe" || return 1
      printf 'stage-preflight-v1\n' > "$probe/.skill-deck-owner" || {
        rm -rf -- "$probe" "$renamed_probe"
        return 1
      }
      mv -- "$probe" "$renamed_probe" || {
        rm -rf -- "$probe" "$renamed_probe"
        return 1
      }
      mv -- "$renamed_probe" "$probe" || {
        rm -rf -- "$probe" "$renamed_probe"
        return 1
      }
      rm -rf -- "$probe" || return 1
    }

    operation_root=$1
    operation_id=$2
    validate_operation "$operation_root" "$operation_id" || exit 61
    entries=$(find "$operation_root" -mindepth 1 -maxdepth 1 -type d -name 'entry-*' | LC_ALL=C sort)
    [ -n "$entries" ] || exit 62
    for entry_root in $entries; do
      destination=$(cat -- "$entry_root/destination") || exit 63
      expected=$(cat -- "$entry_root/expected") || exit 64
      expected_parent=$(cat -- "$entry_root/parent-identity") || exit 65
      backup=$(cat -- "$entry_root/backup") || exit 66
      parent=${destination%/*}
      [ -n "$parent" ] || parent=/
      actual_parent=$(stat -Lc '%d:%i' -- "$parent") || exit 67
      [ "$actual_parent" = "$expected_parent" ] || exit 68
      actual=$(fingerprint_entry "$destination") || exit 69
      [ "$actual" = "$expected" ] || exit 70
      if [ -f "$entry_root/expected-content" ] && [ ! -L "$entry_root/expected-content" ]; then
        expected_content=$(cat -- "$entry_root/expected-content") || exit 81
        actual_content=$(fingerprint_content "$destination") || exit 82
        [ "$actual_content" = "$expected_content" ] || exit 83
      fi
      [ ! -e "$backup" ] && [ ! -L "$backup" ] || exit 71
      validate_stage "$entry_root" || exit 72
      preflight_atomic_replace "$parent" "${entry_root##*/}" || exit 73
    done
    for entry_root in $entries; do
      destination=$(cat -- "$entry_root/destination") || { restore_all; exit 74; }
      stage=$(cat -- "$entry_root/stage") || { restore_all; exit 75; }
      backup=$(cat -- "$entry_root/backup") || { restore_all; exit 76; }
      action=$(cat -- "$entry_root/action") || { restore_all; exit 77; }
      [ "$action" = keep ] && continue
      if [ -e "$backup" ] || [ -L "$backup" ]; then
        restore_all
        exit 78
      fi
      if [ -e "$destination" ] || [ -L "$destination" ]; then
        mv -- "$destination" "$backup" || { restore_all; exit 71; }
        : > "$entry_root/backed-up"
      fi
      if [ "$action" != remove ]; then
        mv -- "$stage" "$destination" || { restore_all || exit 90; exit 72; }
        : > "$entry_root/installed"
      fi
    done
    printf '1\0'

    ;;
  verify)

    validate_operation() {
      operation_root=$1
      operation_id=$2
      case "$operation_id" in ''|*[!A-Za-z0-9_-]*) return 1 ;; esac
      [ -d "$operation_root" ] && [ ! -L "$operation_root" ] || return 1
      marker=$operation_root/.skill-deck-owner
      [ -f "$marker" ] && [ ! -L "$marker" ] || return 1
      [ "$(wc -l < "$marker")" -eq 2 ] || return 1
      [ "$(sed -n '1p' "$marker")" = 1 ] || return 1
      [ "$(sed -n '2p' "$marker")" = "$operation_id" ] || return 1
    }

    fingerprint_entry() {
      path=$1
      if [ ! -e "$path" ] && [ ! -L "$path" ]; then
        printf %s entry-v1-missing
        return 0
      fi
      link_target=
      if [ -L "$path" ]; then link_target=$(readlink -- "$path") || return 1; fi
      device=$(stat -c %d -- "$path") || return 1
      inode=$(stat -c %i -- "$path") || return 1
      mode=$(stat -c %f -- "$path") || return 1
      size=$(stat -c %s -- "$path") || return 1
      mtime_seconds=$(stat -c %Y -- "$path") || return 1
      mtime_text=$(stat -c %y -- "$path") || return 1
      case "$mtime_text" in
        *.*) mtime_nanos=${mtime_text#*.}; mtime_nanos=${mtime_nanos%% *} ;;
        *) mtime_nanos=0 ;;
      esac
      digest_line=$({
        printf 'skill-deck-wsl-entry-v1\0'
        for value in "$device" "$inode" "$mode" "$size" "$mtime_seconds" "$mtime_nanos"; do
          printf '%s\0' "$value"
        done
        printf %s "$link_target"
      } | sha256sum) || return 1
      printf 'entry-v1-%s' "${digest_line%% *}"
    }

    remove_no_follow() {
      path=$1
      if [ -L "$path" ] || [ -f "$path" ]; then rm -f -- "$path"
      elif [ -d "$path" ]; then rm -rf -- "$path"
      elif [ -e "$path" ]; then rm -f -- "$path"
      fi
    }

    verify_materialized_destination() {
      entry_root=$1
      destination=$2
      export destination
      # shellcheck disable=SC2016
      xargs -0 -r -n5 /bin/sh -c '
        [ "$#" -eq 5 ] || exit 255
        kind=$1
        relative=$2
        blob_id=$3
        executable=$4
        expected_size=$5
        path=$destination/$relative
        case "$kind" in
          directory) [ -d "$path" ] && [ ! -L "$path" ] || exit 255 ;;
          file)
            [ -f "$path" ] && [ ! -L "$path" ] || exit 255
            [ "$(wc -c < "$path")" = "$expected_size" ] || exit 255
            digest_line=$(sha256sum -- "$path") || exit 255
            [ "${digest_line%% *}" = "$blob_id" ] || exit 255
            if [ "$executable" = 1 ]; then [ -x "$path" ]; else [ ! -x "$path" ]; fi || exit 255
            ;;
          *) exit 255 ;;
        esac
      ' sh < "$entry_root/manifest" || return 1
      expected_count=$(cat -- "$entry_root/manifest-count") || return 1
      actual_count=$(find "$destination" -mindepth 1 -printf . | wc -c) || return 1
      [ "$actual_count" = "$expected_count" ]
    }

    restore_all() {
      for entry_root in $(find "$operation_root" -mindepth 1 -maxdepth 1 -type d -name 'entry-*' | LC_ALL=C sort -r); do
        destination=$(cat -- "$entry_root/destination") || return 1
        backup=$(cat -- "$entry_root/backup") || return 1
        if [ -f "$entry_root/installed" ]; then
          remove_no_follow "$destination" || return 1
          rm -f -- "$entry_root/installed"
        fi
        if [ -f "$entry_root/backed-up" ]; then
          mv -- "$backup" "$destination" || return 1
          rm -f -- "$entry_root/backed-up"
        fi
      done
    }

    operation_root=$1
    operation_id=$2
    validate_operation "$operation_root" "$operation_id" || exit 61
    for entry_root in $(find "$operation_root" -mindepth 1 -maxdepth 1 -type d -name 'entry-*' | LC_ALL=C sort); do
      destination=$(cat -- "$entry_root/destination") || exit 62
      action=$(cat -- "$entry_root/action") || exit 63
      case "$action" in
        keep)
          expected=$(cat -- "$entry_root/expected") || exit 64
          actual=$(fingerprint_entry "$destination") || exit 65
          [ "$actual" = "$expected" ] || exit 66
          ;;
        materialize)
          [ -d "$destination" ] && [ ! -L "$destination" ] || exit 64
          verify_materialized_destination "$entry_root" "$destination" || exit 65
          ;;
        symlink)
          [ -L "$destination" ] || exit 66
          source=$(cat -- "$entry_root/source") || exit 67
          [ "$(readlink -- "$destination")" = "$source" ] || exit 68
          ;;
        remove) [ ! -e "$destination" ] && [ ! -L "$destination" ] || exit 69 ;;
        *) exit 70 ;;
      esac
    done
    printf '1\0'

    ;;
  restore)

    validate_operation() {
      operation_root=$1
      operation_id=$2
      case "$operation_id" in ''|*[!A-Za-z0-9_-]*) return 1 ;; esac
      [ -d "$operation_root" ] && [ ! -L "$operation_root" ] || return 1
      marker=$operation_root/.skill-deck-owner
      [ -f "$marker" ] && [ ! -L "$marker" ] || return 1
      [ "$(wc -l < "$marker")" -eq 2 ] || return 1
      [ "$(sed -n '1p' "$marker")" = 1 ] || return 1
      [ "$(sed -n '2p' "$marker")" = "$operation_id" ] || return 1
    }

    fingerprint_entry() {
      path=$1
      if [ ! -e "$path" ] && [ ! -L "$path" ]; then
        printf %s entry-v1-missing
        return 0
      fi
      link_target=
      if [ -L "$path" ]; then link_target=$(readlink -- "$path") || return 1; fi
      device=$(stat -c %d -- "$path") || return 1
      inode=$(stat -c %i -- "$path") || return 1
      mode=$(stat -c %f -- "$path") || return 1
      size=$(stat -c %s -- "$path") || return 1
      mtime_seconds=$(stat -c %Y -- "$path") || return 1
      mtime_text=$(stat -c %y -- "$path") || return 1
      case "$mtime_text" in
        *.*) mtime_nanos=${mtime_text#*.}; mtime_nanos=${mtime_nanos%% *} ;;
        *) mtime_nanos=0 ;;
      esac
      digest_line=$({
        printf 'skill-deck-wsl-entry-v1\0'
        for value in "$device" "$inode" "$mode" "$size" "$mtime_seconds" "$mtime_nanos"; do
          printf '%s\0' "$value"
        done
        printf %s "$link_target"
      } | sha256sum) || return 1
      printf 'entry-v1-%s' "${digest_line%% *}"
    }

    remove_no_follow() {
      path=$1
      if [ -L "$path" ] || [ -f "$path" ]; then rm -f -- "$path"
      elif [ -d "$path" ]; then rm -rf -- "$path"
      elif [ -e "$path" ]; then rm -f -- "$path"
      fi
    }

    restore_all() {
      for entry_root in $(find "$operation_root" -mindepth 1 -maxdepth 1 -type d -name 'entry-*' | LC_ALL=C sort -r); do
        destination=$(cat -- "$entry_root/destination") || return 1
        backup=$(cat -- "$entry_root/backup") || return 1
        if [ -f "$entry_root/installed" ]; then
          remove_no_follow "$destination" || return 1
          rm -f -- "$entry_root/installed"
        fi
        if [ -f "$entry_root/backed-up" ]; then
          mv -- "$backup" "$destination" || return 1
          rm -f -- "$entry_root/backed-up"
        fi
      done
    }

    operation_root=$1
    operation_id=$2
    validate_operation "$operation_root" "$operation_id" || exit 61
    restore_all || exit 90
    printf '1\0'

    ;;
  cleanup)

    validate_operation() {
      operation_root=$1
      operation_id=$2
      case "$operation_id" in ''|*[!A-Za-z0-9_-]*) return 1 ;; esac
      [ -d "$operation_root" ] && [ ! -L "$operation_root" ] || return 1
      marker=$operation_root/.skill-deck-owner
      [ -f "$marker" ] && [ ! -L "$marker" ] || return 1
      [ "$(wc -l < "$marker")" -eq 2 ] || return 1
      [ "$(sed -n '1p' "$marker")" = 1 ] || return 1
      [ "$(sed -n '2p' "$marker")" = "$operation_id" ] || return 1
    }

    fingerprint_entry() {
      path=$1
      if [ ! -e "$path" ] && [ ! -L "$path" ]; then
        printf %s entry-v1-missing
        return 0
      fi
      link_target=
      if [ -L "$path" ]; then link_target=$(readlink -- "$path") || return 1; fi
      device=$(stat -c %d -- "$path") || return 1
      inode=$(stat -c %i -- "$path") || return 1
      mode=$(stat -c %f -- "$path") || return 1
      size=$(stat -c %s -- "$path") || return 1
      mtime_seconds=$(stat -c %Y -- "$path") || return 1
      mtime_text=$(stat -c %y -- "$path") || return 1
      case "$mtime_text" in
        *.*) mtime_nanos=${mtime_text#*.}; mtime_nanos=${mtime_nanos%% *} ;;
        *) mtime_nanos=0 ;;
      esac
      digest_line=$({
        printf 'skill-deck-wsl-entry-v1\0'
        for value in "$device" "$inode" "$mode" "$size" "$mtime_seconds" "$mtime_nanos"; do
          printf '%s\0' "$value"
        done
        printf %s "$link_target"
      } | sha256sum) || return 1
      printf 'entry-v1-%s' "${digest_line%% *}"
    }

    remove_no_follow() {
      path=$1
      if [ -L "$path" ] || [ -f "$path" ]; then rm -f -- "$path"
      elif [ -d "$path" ]; then rm -rf -- "$path"
      elif [ -e "$path" ]; then rm -f -- "$path"
      fi
    }

    restore_all() {
      for entry_root in $(find "$operation_root" -mindepth 1 -maxdepth 1 -type d -name 'entry-*' | LC_ALL=C sort -r); do
        destination=$(cat -- "$entry_root/destination") || return 1
        backup=$(cat -- "$entry_root/backup") || return 1
        if [ -f "$entry_root/installed" ]; then
          remove_no_follow "$destination" || return 1
          rm -f -- "$entry_root/installed"
        fi
        if [ -f "$entry_root/backed-up" ]; then
          mv -- "$backup" "$destination" || return 1
          rm -f -- "$entry_root/backed-up"
        fi
      done
    }

    operation_root=$1
    operation_id=$2
    validate_operation "$operation_root" "$operation_id" || exit 61
    for entry_root in $(find "$operation_root" -mindepth 1 -maxdepth 1 -type d -name 'entry-*' | LC_ALL=C sort); do
      stage=$(cat -- "$entry_root/stage") || exit 62
      backup=$(cat -- "$entry_root/backup") || exit 63
      path=$stage
      if [ -n "$path" ]; then
        name=${path##*/}
        case "$name" in .skill-deck-stage-$operation_id-*) ;;
          *) exit 64 ;;
        esac
        remove_no_follow "$path" || exit 65
      fi
      if [ -f "$entry_root/backed-up" ]; then
        name=${backup##*/}
        case "$name" in .skill-deck-backup-$operation_id-*) ;;
          *) exit 66 ;;
        esac
        remove_no_follow "$backup" || exit 67
      fi
    done
    rm -rf -- "$operation_root" || exit 68
    printf '1\0'

    ;;
  *)
    printf 'unknown Skill Deck WSL materialize operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
