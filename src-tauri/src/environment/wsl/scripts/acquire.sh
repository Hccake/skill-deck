#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  source-revision)
    repository_root=$1
    skill_path=$2
    case "$repository_root" in /*) ;; *) exit 61 ;; esac
    case "$skill_path" in /*|..|../*|*/..|*/../*) exit 62 ;; esac
    physical_root=$(cd -P -- "$repository_root" 2>/dev/null && pwd -P) || exit 63
    if [ -n "$skill_path" ]; then
      revision_spec=HEAD:$skill_path
    else
      revision_spec='HEAD^{tree}'
    fi
    revision=$(git -C "$physical_root" rev-parse --verify "$revision_spec" 2>/dev/null) || exit 64
    case "${#revision}" in 40|64) ;; *) exit 65 ;; esac
    case "$revision" in *[!0-9a-fA-F]*) exit 66 ;; esac
    printf '1\0%s\0' "$revision"
    ;;
  fingerprint)
    source_root=$1
    physical_root=$(cd -P -- "$source_root" 2>/dev/null && pwd -P) || exit 61
    [ -d "$physical_root" ] || exit 62

    # Reject links that cannot be resolved inside the selected Skill.
    # shellcheck disable=SC2016
    find "$physical_root" -mindepth 1 \
      \( -name .git -o -name __pycache__ -o -name __pypackages__ \) -prune -o \
      -type l -exec /bin/sh -c '
        root=$1
        shift
        for link do
          target=$(readlink -f -- "$link" 2>/dev/null) || exit 1
          case "$target" in "$root"/*) ;; *) exit 1 ;; esac
        done
      ' sh "$physical_root" {} + || exit 63

    # shellcheck disable=SC2016
    fingerprint=$(
      find "$physical_root" -mindepth 1 \
        \( -name .git -o -name __pycache__ -o -name __pypackages__ \) -prune -o \
        ! -name metadata.json -print0 \
        | LC_ALL=C sort -z \
        | xargs -0 -r /bin/sh -c '
            root=$1
            shift
            for path do
              relative=${path#"$root"/}
              if [ -L "$path" ]; then
                kind=link
                target=$(readlink -- "$path") || exit 1
              elif [ -d "$path" ]; then
                kind=directory
                target=
              elif [ -f "$path" ]; then
                kind=file
                target=
              else
                kind=other
                target=
              fi
              size=$(stat -c %s -- "$path") || exit 1
              modified=$(stat -c %Y -- "$path") || exit 1
              mode=$(stat -c %a -- "$path") || exit 1
              printf "%s\0%s\0%s\0%s\0%s\0%s\0" \
                "$relative" "$kind" "$size" "$modified" "$mode" "$target" || exit 1
            done
          ' sh "$physical_root" \
        | sha256sum
    ) || exit 64
    fingerprint=${fingerprint%% *}
    [ "${#fingerprint}" -eq 64 ] || exit 65
    case "$fingerprint" in *[!0-9a-f]*) exit 66 ;; esac
    printf '1\0%s\0' "$fingerprint"
    ;;
  acquire)

    source_root=$1
    session_root=$2
    payload_root=$3
    session_id=$4
    physical_root=$(cd -P -- "$source_root" 2>/dev/null && pwd -P) || exit 61
    [ -d "$physical_root" ] || exit 62
    payload_name=${payload_root#"$session_root"/}
    [ "$payload_root" = "$session_root/$payload_name" ] || exit 63
    case "$payload_name" in payload-*) ;; *) exit 64 ;; esac
    case "$payload_name" in */*) exit 65 ;; esac
    umask 077
    if [ ! -e "$session_root" ] && [ ! -L "$session_root" ]; then
      mkdir -- "$session_root" || exit 66
      marker_tmp=$session_root/.skill-deck-owner.$$
      trap 'rm -rf -- "${stage-}" "$marker_tmp"' EXIT HUP INT TERM
      printf '1\n%s\n' "$session_id" > "$marker_tmp" || exit 67
      mv -- "$marker_tmp" "$session_root/.skill-deck-owner" || exit 68
    else
      trap 'rm -rf -- "${stage-}"' EXIT HUP INT TERM
    fi
    [ -d "$session_root" ] && [ ! -L "$session_root" ] || exit 69
    marker=$session_root/.skill-deck-owner
    [ -f "$marker" ] && [ ! -L "$marker" ] || exit 70
    [ "$(wc -l < "$marker")" -eq 2 ] || exit 71
    [ "$(sed -n '1p' "$marker")" = 1 ] || exit 72
    [ "$(sed -n '2p' "$marker")" = "$session_id" ] || exit 73
    [ ! -e "$payload_root" ] && [ ! -L "$payload_root" ] || exit 74
    stage=$session_root/.stage-$payload_name-$$
    mkdir -- "$stage" || exit 75
    mkdir -- "$stage/blobs" || exit 76

    # shellcheck disable=SC2016
    find "$physical_root" -mindepth 1 \
      \( -name .git -o -name __pycache__ -o -name __pypackages__ \) -prune -o \
      -type l -exec /bin/sh -c '
        root=$1
        stage=$2
        shift 2
        for link do
          target=$(readlink -f -- "$link" 2>/dev/null) || { : > "$stage/.failed"; continue; }
          case "$target" in "$root"/*) ;; *) : > "$stage/.failed" ;; esac
        done
      ' sh "$physical_root" "$stage" {} + || exit 77
    [ ! -e "$stage/.failed" ] || exit 78

    # shellcheck disable=SC2016
    cli_hash=$(
      find -L "$physical_root" -mindepth 1 \
        \( -name .git -o -name node_modules -o -name __pycache__ -o -name __pypackages__ \) -prune -o \
        -type f ! -name metadata.json -print0 \
        | LC_ALL=C sort -z -f \
        | xargs -0 -r /bin/sh -c '
            root=$1
            shift
            for path do
              relative=${path#"$root"/}
              printf %s "$relative" || exit 1
              cat -- "$path" || exit 1
            done
          ' sh "$physical_root" \
        | sha256sum
    ) || exit 77
    cli_hash=${cli_hash%% *}
    [ "${#cli_hash}" -eq 64 ] || exit 78
    case "$cli_hash" in *[!0-9a-f]*) exit 79 ;; esac
    printf '1\0H\0%s\0' "$cli_hash"
    # shellcheck disable=SC2016
    find -L "$physical_root" -mindepth 1 \
      \( -name .git -o -name __pycache__ -o -name __pypackages__ \) -prune -o \
      -exec /bin/sh -c '
        root=$1
        stage=$2
        shift 2
        for path do
          relative=${path#"$root"/}
          if [ -d "$path" ]; then
            printf "E\\0%s\\0%s\\0%s\\0%s\\0%s\\0" \
              directory "$relative" "" 0 0
            continue
          fi
          [ -f "$path" ] || continue
          [ "${path##*/}" != metadata.json ] || continue
          digest_line=$(sha256sum -- "$path" 2>/dev/null) || {
            : > "$stage/.failed"
            continue
          }
          blob_id=${digest_line%% *}
          size=$(wc -c < "$path" 2>/dev/null) || { : > "$stage/.failed"; continue; }
          mode=$(stat -c %a -- "$path" 2>/dev/null) || { : > "$stage/.failed"; continue; }
          executable=$(( (0$mode & 0111) != 0 ))
          blob=$stage/blobs/$blob_id
          if [ ! -e "$blob" ]; then
            cp -- "$path" "$blob" 2>/dev/null || { : > "$stage/.failed"; continue; }
            chmod 600 -- "$blob" 2>/dev/null || { : > "$stage/.failed"; continue; }
          fi
          digest_line=$(sha256sum -- "$blob" 2>/dev/null) || {
            : > "$stage/.failed"
            continue
          }
          [ "${digest_line%% *}" = "$blob_id" ] || {
            : > "$stage/.failed"
            continue
          }
          printf "E\\0%s\\0%s\\0%s\\0%s\\0%s\\0" \
            file "$relative" "$blob_id" "$size" "$executable"
        done
      ' sh "$physical_root" "$stage" {} + || exit 80
    [ ! -e "$stage/.failed" ] || exit 81
    find "$stage/blobs" -mindepth 1 -maxdepth 1 -type f -printf '%f\n' \
      | LC_ALL=C sort > "$stage/blob-list" || exit 82
    mv -- "$stage" "$payload_root" || exit 83
    stage=
    trap - EXIT HUP INT TERM

    ;;
  store-begin)

    session_root=$1
    payload_root=$2
    session_id=$3
    payload_name=${payload_root#"$session_root"/}
    [ "$payload_root" = "$session_root/$payload_name" ] || exit 61
    case "$payload_name" in payload-*) ;; *) exit 62 ;; esac
    case "$payload_name" in */*) exit 63 ;; esac
    stage=$payload_root.upload
    umask 077
    if [ ! -e "$session_root" ] && [ ! -L "$session_root" ]; then
      mkdir -- "$session_root" || exit 64
      marker_tmp=$session_root/.skill-deck-owner.$$
      trap 'rm -rf -- "$stage" "$marker_tmp"' EXIT HUP INT TERM
      printf '1\n%s\n' "$session_id" > "$marker_tmp" || exit 65
      mv -- "$marker_tmp" "$session_root/.skill-deck-owner" || exit 66
    else
      trap 'rm -rf -- "$stage"' EXIT HUP INT TERM
    fi
    [ -d "$session_root" ] && [ ! -L "$session_root" ] || exit 67
    marker=$session_root/.skill-deck-owner
    [ -f "$marker" ] && [ ! -L "$marker" ] || exit 68
    [ "$(wc -l < "$marker")" -eq 2 ] || exit 69
    [ "$(sed -n '1p' "$marker")" = 1 ] || exit 70
    [ "$(sed -n '2p' "$marker")" = "$session_id" ] || exit 71
    [ ! -e "$payload_root" ] && [ ! -L "$payload_root" ] || exit 72
    [ ! -e "$stage" ] && [ ! -L "$stage" ] || exit 73
    mkdir -- "$stage" || exit 74
    mkdir -- "$stage/blobs" || exit 75
    printf '1\0'
    trap - EXIT HUP INT TERM

    ;;
  store-blob)

    session_root=$1
    payload_root=$2
    session_id=$3
    blob_id=$4
    [ "${#blob_id}" -eq 64 ] || exit 61
    case "$blob_id" in *[!0-9a-f]*) exit 62 ;; esac
    [ -d "$session_root" ] && [ ! -L "$session_root" ] || exit 63
    marker=$session_root/.skill-deck-owner
    [ -f "$marker" ] && [ ! -L "$marker" ] || exit 64
    [ "$(wc -l < "$marker")" -eq 2 ] || exit 65
    [ "$(sed -n '1p' "$marker")" = 1 ] || exit 66
    [ "$(sed -n '2p' "$marker")" = "$session_id" ] || exit 67
    payload_name=${payload_root#"$session_root"/}
    [ "$payload_root" = "$session_root/$payload_name" ] || exit 68
    case "$payload_name" in payload-*) ;; *) exit 69 ;; esac
    case "$payload_name" in */*) exit 70 ;; esac
    stage=$payload_root.upload
    [ -d "$stage" ] && [ ! -L "$stage" ] || exit 71
    [ -d "$stage/blobs" ] && [ ! -L "$stage/blobs" ] || exit 72
    blob=$stage/blobs/$blob_id
    [ ! -e "$blob" ] && [ ! -L "$blob" ] || exit 73
    tmp=$stage/.blob.$$
    trap 'rm -f -- "$tmp"' EXIT HUP INT TERM
    cat > "$tmp" || exit 74
    [ "$(sha256sum -- "$tmp" | awk '{print $1}')" = "$blob_id" ] || exit 75
    chmod 600 -- "$tmp" || exit 76
    sync "$tmp" 2>/dev/null || true
    mv -- "$tmp" "$blob" || exit 77
    printf '1\0'
    trap - EXIT HUP INT TERM

    ;;
  store-finalize)

    session_root=$1
    payload_root=$2
    session_id=$3
    [ -d "$session_root" ] && [ ! -L "$session_root" ] || exit 61
    marker=$session_root/.skill-deck-owner
    [ -f "$marker" ] && [ ! -L "$marker" ] || exit 62
    [ "$(wc -l < "$marker")" -eq 2 ] || exit 63
    [ "$(sed -n '1p' "$marker")" = 1 ] || exit 64
    [ "$(sed -n '2p' "$marker")" = "$session_id" ] || exit 65
    payload_name=${payload_root#"$session_root"/}
    [ "$payload_root" = "$session_root/$payload_name" ] || exit 66
    case "$payload_name" in payload-*) ;; *) exit 67 ;; esac
    case "$payload_name" in */*) exit 68 ;; esac
    [ ! -e "$payload_root" ] && [ ! -L "$payload_root" ] || exit 69
    stage=$payload_root.upload
    [ -d "$stage" ] && [ ! -L "$stage" ] || exit 70
    [ -d "$stage/blobs" ] && [ ! -L "$stage/blobs" ] || exit 71
    find "$stage/blobs" -mindepth 1 -maxdepth 1 -type f -printf '%f\n' \
      | LC_ALL=C sort > "$stage/blob-list" || exit 72
    IFS= read -r expected_count || exit 73
    case "$expected_count" in ''|*[!0-9]*) exit 74 ;; esac
    exec 3< "$stage/blob-list"
    index=0
    while [ "$index" -lt "$expected_count" ]; do
      IFS= read -r expected_id || exit 75
      IFS= read -r actual_id <&3 || exit 76
      [ "$expected_id" = "$actual_id" ] || exit 77
      blob=$stage/blobs/$actual_id
      [ -f "$blob" ] && [ ! -L "$blob" ] || exit 78
      [ "$(sha256sum -- "$blob" | awk '{print $1}')" = "$actual_id" ] || exit 79
      index=$((index + 1))
    done
    if IFS= read -r _ <&3; then exit 80; fi
    manifest_tmp=$stage/.manifest.$$
    trap 'rm -f -- "$manifest_tmp"' EXIT HUP INT TERM
    cat > "$manifest_tmp" || exit 81
    [ -s "$manifest_tmp" ] || exit 82
    sync "$manifest_tmp" 2>/dev/null || true
    mv -- "$manifest_tmp" "$stage/manifest.json" || exit 83
    mv -- "$stage" "$payload_root" || exit 84
    printf '1\0'
    trap - EXIT HUP INT TERM

    ;;
  finalize)

    session_root=$1
    payload_root=$2
    session_id=$3
    [ -d "$session_root" ] && [ ! -L "$session_root" ] || exit 61
    marker=$session_root/.skill-deck-owner
    [ -f "$marker" ] && [ ! -L "$marker" ] || exit 62
    [ "$(wc -l < "$marker")" -eq 2 ] || exit 63
    [ "$(sed -n '1p' "$marker")" = 1 ] || exit 64
    [ "$(sed -n '2p' "$marker")" = "$session_id" ] || exit 65
    payload_name=${payload_root#"$session_root"/}
    [ "$payload_root" = "$session_root/$payload_name" ] || exit 66
    case "$payload_name" in payload-*) ;; *) exit 67 ;; esac
    case "$payload_name" in */*) exit 68 ;; esac
    [ -d "$payload_root" ] && [ ! -L "$payload_root" ] || exit 69
    [ -f "$payload_root/blob-list" ] && [ ! -L "$payload_root/blob-list" ] || exit 70
    [ -d "$payload_root/blobs" ] && [ ! -L "$payload_root/blobs" ] || exit 71
    [ ! -e "$payload_root/manifest.json" ] && [ ! -L "$payload_root/manifest.json" ] || exit 72
    IFS= read -r expected_count || exit 73
    case "$expected_count" in ''|*[!0-9]*) exit 74 ;; esac
    exec 3< "$payload_root/blob-list"
    index=0
    while [ "$index" -lt "$expected_count" ]; do
      IFS= read -r expected_id || exit 75
      IFS= read -r actual_id <&3 || exit 76
      [ "$expected_id" = "$actual_id" ] || exit 77
      blob=$payload_root/blobs/$actual_id
      [ -f "$blob" ] && [ ! -L "$blob" ] || exit 78
      [ "$(sha256sum -- "$blob" | awk '{print $1}')" = "$actual_id" ] || exit 79
      index=$((index + 1))
    done
    if IFS= read -r _ <&3; then exit 80; fi
    manifest_tmp=$payload_root/.manifest.$$
    trap 'rm -f -- "$manifest_tmp"' EXIT HUP INT TERM
    cat > "$manifest_tmp" || exit 81
    [ -s "$manifest_tmp" ] || exit 82
    sync "$manifest_tmp" 2>/dev/null || true
    mv -- "$manifest_tmp" "$payload_root/manifest.json" || exit 83
    printf '1\0'
    trap - EXIT HUP INT TERM

    ;;
  verify)

    session_root=$1
    payload_root=$2
    session_id=$3
    validate_expected=0
    if [ "${4-}" = --expected ]; then
      validate_expected=1
    fi
    [ -d "$session_root" ] && [ ! -L "$session_root" ] || exit 61
    marker=$session_root/.skill-deck-owner
    [ -f "$marker" ] && [ ! -L "$marker" ] || exit 62
    [ "$(wc -l < "$marker")" -eq 2 ] || exit 63
    [ "$(sed -n '1p' "$marker")" = 1 ] || exit 64
    [ "$(sed -n '2p' "$marker")" = "$session_id" ] || exit 65
    payload_name=${payload_root#"$session_root"/}
    [ "$payload_root" = "$session_root/$payload_name" ] || exit 66
    case "$payload_name" in payload-*) ;; *) exit 67 ;; esac
    case "$payload_name" in */*) exit 68 ;; esac
    [ -d "$payload_root" ] && [ ! -L "$payload_root" ] || exit 69
    manifest=$payload_root/manifest.json
    blob_list=$payload_root/blob-list
    blobs=$payload_root/blobs
    [ -f "$manifest" ] && [ ! -L "$manifest" ] || exit 70
    [ -f "$blob_list" ] && [ ! -L "$blob_list" ] || exit 71
    [ -d "$blobs" ] && [ ! -L "$blobs" ] || exit 72
    if [ "$validate_expected" -eq 1 ]; then
      cmp -s - "$blob_list" || exit 78
    fi
    actual_count=0
    while IFS= read -r blob_id || [ -n "$blob_id" ]; do
      [ -n "$blob_id" ] || exit 73
      [ "${#blob_id}" -eq 64 ] || exit 74
      case "$blob_id" in *[!0-9a-f]*) exit 75 ;; esac
      blob=$blobs/$blob_id
      [ -f "$blob" ] && [ ! -L "$blob" ] || exit 76
      [ "$(sha256sum -- "$blob" | awk '{print $1}')" = "$blob_id" ] || exit 77
      actual_count=$((actual_count + 1))
    done < "$blob_list"
    cat -- "$manifest"

    ;;
  read-blob)

    session_root=$1
    payload_root=$2
    session_id=$3
    blob_id=$4
    [ "${#blob_id}" -eq 64 ] || exit 61
    case "$blob_id" in *[!0-9a-f]*) exit 62 ;; esac
    [ -d "$session_root" ] && [ ! -L "$session_root" ] || exit 63
    marker=$session_root/.skill-deck-owner
    [ -f "$marker" ] && [ ! -L "$marker" ] || exit 64
    [ "$(wc -l < "$marker")" -eq 2 ] || exit 65
    [ "$(sed -n '1p' "$marker")" = 1 ] || exit 66
    [ "$(sed -n '2p' "$marker")" = "$session_id" ] || exit 67
    payload_name=${payload_root#"$session_root"/}
    [ "$payload_root" = "$session_root/$payload_name" ] || exit 68
    case "$payload_name" in payload-*) ;; *) exit 69 ;; esac
    case "$payload_name" in */*) exit 70 ;; esac
    [ -d "$payload_root/blobs" ] && [ ! -L "$payload_root" ] && [ ! -L "$payload_root/blobs" ] || exit 71
    blob=$payload_root/blobs/$blob_id
    [ -f "$blob" ] && [ ! -L "$blob" ] || exit 72
    [ "$(sha256sum -- "$blob" | awk '{print $1}')" = "$blob_id" ] || exit 73
    cat -- "$blob"

    ;;
  remove-payload)

    session_root=$1
    payload_root=$2
    session_id=$3
    [ ! -e "$session_root" ] && [ ! -L "$session_root" ] && exit 0
    [ -d "$session_root" ] && [ ! -L "$session_root" ] || exit 61
    marker=$session_root/.skill-deck-owner
    [ -f "$marker" ] && [ ! -L "$marker" ] || exit 62
    [ "$(wc -l < "$marker")" -eq 2 ] || exit 63
    [ "$(sed -n '1p' "$marker")" = 1 ] || exit 64
    [ "$(sed -n '2p' "$marker")" = "$session_id" ] || exit 65
    payload_name=${payload_root#"$session_root"/}
    [ "$payload_root" = "$session_root/$payload_name" ] || exit 66
    case "$payload_name" in payload-*) ;; *) exit 67 ;; esac
    case "$payload_name" in */*) exit 68 ;; esac
    [ ! -L "$payload_root" ] || exit 69
    [ ! -e "$payload_root" ] && exit 0
    [ -d "$payload_root" ] || exit 70
    rm -rf -- "$payload_root"

    ;;
  remove-session)

    session_root=$1
    session_id=$2
    expected_root=/tmp/skill-deck-source-$session_id
    [ "$session_root" = "$expected_root" ] || exit 61
    [ ! -e "$session_root" ] && [ ! -L "$session_root" ] && exit 0
    [ -d "$session_root" ] && [ ! -L "$session_root" ] || exit 62
    marker=$session_root/.skill-deck-owner
    [ -f "$marker" ] && [ ! -L "$marker" ] || exit 63
    [ "$(wc -l < "$marker")" -eq 2 ] || exit 64
    [ "$(sed -n '1p' "$marker")" = 1 ] || exit 65
    [ "$(sed -n '2p' "$marker")" = "$session_id" ] || exit 66
    rm -rf -- "$session_root"

    ;;
  sweep-orphans)

    base=$1
    shift
    case "$base" in /*) ;; *) exit 61 ;; esac
    removed=0
    protected=0
    external=0
    blocked=0
    printf '1\0'
    for root in "$base"/skill-deck-source-*; do
      [ -e "$root" ] || [ -L "$root" ] || continue
      candidate=${root##*/}
      retain() {
        code=$1
        blocked=1
        size=$(du -sb -- "$root" 2>/dev/null | awk '{print $1}')
        case "$size" in ''|*[!0-9]*)
          printf 'W\0sizeUnavailable\0%s\0-\0' "$candidate"
          ;;
          *) external=$((external + size)) ;;
        esac
        printf 'W\0%s\0%s\0-\0' "$code" "$candidate"
      }
      if [ ! -d "$root" ] || [ -L "$root" ]; then
        retain boundaryRejected
        continue
      fi
      marker=$root/.skill-deck-owner
      if [ ! -f "$marker" ] || [ -L "$marker" ] || [ "$(wc -l < "$marker" 2>/dev/null)" -ne 2 ]; then
        retain invalidMarker
        continue
      fi
      version=$(sed -n '1p' "$marker")
      session_id=$(sed -n '2p' "$marker")
      if [ "$version" != 1 ]; then
        retain futureMarkerVersion
        continue
      fi
      if [ -z "$session_id" ] || [ "$root" != "$base/skill-deck-source-$session_id" ]; then
        retain invalidMarker
        continue
      fi
      is_protected=0
      for protected_id do
        if [ "$protected_id" = "$session_id" ]; then
          is_protected=1
          break
        fi
      done
      if [ "$is_protected" -eq 1 ]; then
        protected=$((protected + 1))
        continue
      fi
      if rm -rf -- "$root"; then
        removed=$((removed + 1))
      else
        retain deleteFailed
      fi
    done
    printf 'S\0%s\0%s\0%s\0%s\0' "$removed" "$protected" "$external" "$blocked"

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
