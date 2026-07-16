#!/bin/sh
subcommand=$1
shift
case "$subcommand" in
  scan)

    per_file=$1
    aggregate=$2
    stat_only=$3
    recursive=$4
    shift 4
    total=0
    root_index=0
    printf '2\0'
    emit_entry() {
      path=$1
      relative=$2
      kind=other
      target=
      size=0
      mode=0
      modified=0
      truncated=0
      error=
      content_len=0
      if [ -L "$path" ]; then
        kind=symlink
        target=$(readlink -- "$path" 2>/dev/null) || error=readLinkFailed
        if [ -f "$path" ]; then
          size=$(wc -c < "$path" 2>/dev/null) || { size=0; error=readFailed; }
        fi
      elif [ -d "$path" ]; then
        kind=directory
      elif [ -f "$path" ]; then
        kind='file'
        size=$(wc -c < "$path" 2>/dev/null) || { size=0; error=readFailed; }
        case "$relative" in
          SKILL.md|*/SKILL.md|skills-lock.json)
            remaining=$((aggregate - total))
            [ "$remaining" -lt 0 ] && remaining=0
            content_len=$size
            [ "$content_len" -gt "$per_file" ] && content_len=$per_file
            [ "$content_len" -gt "$remaining" ] && content_len=$remaining
            [ "$content_len" -lt "$size" ] && truncated=1
            ;;
        esac
      elif [ ! -e "$path" ]; then
        kind=missing
      fi
      if [ "$kind" != missing ]; then
        mode_text=$(stat -c %a -- "$path" 2>/dev/null) || { mode_text=0; error=statFailed; }
        mode=$((0$mode_text))
        modified=$(stat -c %Y -- "$path" 2>/dev/null) || { modified=0; error=statFailed; }
      fi
      case "$relative" in
        .claude-plugin/marketplace.json|.claude-plugin/plugin.json)
          remaining=$((aggregate - total))
          [ "$remaining" -lt 0 ] && remaining=0
          content_len=$size
          [ "$content_len" -gt "$per_file" ] && content_len=$per_file
          [ "$content_len" -gt "$remaining" ] && content_len=$remaining
          [ "$content_len" -lt "$size" ] && truncated=1
          ;;
      esac
      if [ -z "$relative" ] && [ "${path##*/}" = skills-lock.json ]; then
        remaining=$((aggregate - total))
        [ "$remaining" -lt 0 ] && remaining=0
        content_len=$size
        [ "$content_len" -gt "$per_file" ] && content_len=$per_file
        [ "$content_len" -gt "$remaining" ] && content_len=$remaining
        [ "$content_len" -lt "$size" ] && truncated=1
      fi
      printf 'E\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0' \
        "$root_index" "$relative" "$kind" "$target" "$size" "$mode" "$modified" "$truncated" "$error" "$content_len"
      if [ "$content_len" -gt 0 ]; then
        dd if="$path" bs=1 count="$content_len" status=none || exit 71
        total=$((total + content_len))
      fi
      printf '\0'
    }
    is_stat_only() {
      case ",$stat_only," in
        *",$1,"*) return 0 ;;
        *) return 1 ;;
      esac
    }
    for root do
      emit_entry "$root" ''
      if ! is_stat_only "$root_index" && [ -d "$root" ] && [ ! -L "$root" ]; then
        if [ "$recursive" = 1 ]; then
          find "$root" -mindepth 1 -maxdepth 6 \
            \( -name .git -o -name node_modules -o -name dist -o -name build -o -name __pycache__ -o -name __pypackages__ \) -prune \
            -o \( \
              \( -type f -o -type l \) -a \( -iname SKILL.md -o -path "$root/.claude-plugin/marketplace.json" -o -path "$root/.claude-plugin/plugin.json" -o -path "$root/skills-lock.json" \) \
            \) -print | while IFS= read -r path; do
              if [ ! -e "$path" ] && [ ! -L "$path" ]; then
                continue
              fi
              relative=${path#"$root"/}
              emit_entry "$path" "$relative"
            done
        elif [ "$recursive" = 2 ]; then
          for path in "$root"/* "$root"/.[!.]* "$root"/..?*; do
            if [ ! -d "$path" ] || [ -L "$path" ]; then
              continue
            fi
            find "$path" -mindepth 1 -maxdepth 1 \( -type f -o -type l \) -iname SKILL.md -print | while IFS= read -r skill_path; do
              relative=${skill_path#"$root"/}
              emit_entry "$skill_path" "$relative"
            done
          done
        else
          for path in "$root"/* "$root"/.[!.]* "$root"/..?*; do
            if [ ! -e "$path" ] && [ ! -L "$path" ]; then
              continue
            fi
            relative=${path#"$root"/}
            emit_entry "$path" "$relative"
            if [ -d "$path" ]; then
              skill_path=$path/SKILL.md
              if [ -e "$skill_path" ] || [ -L "$skill_path" ]; then
                emit_entry "$skill_path" "$relative/SKILL.md"
              fi
            fi
          done
        fi
      fi
      root_index=$((root_index + 1))
    done

    ;;
  *)
    printf 'unknown Skill Deck WSL operation: %s\n' "$subcommand" >&2
    exit 64
    ;;
esac
