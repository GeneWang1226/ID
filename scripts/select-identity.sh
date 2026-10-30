#!/usr/bin/env bash
# 在终端用方向键选择已有身份；非交互场景退回普通输入。
set -euo pipefail

mapfile -t identities < <(find identities -mindepth 1 -maxdepth 1 -type d ! -name '.*' -printf '%f\n' | sort)

if (( ${#identities[@]} == 0 )); then
    read -r -p '身份: ' selected
    printf '%s\n' "$selected"
    exit 0
fi

if [[ ! -t 0 || ! -t 2 ]]; then
    read -r -p "身份（${identities[*]}）: " selected
    printf '%s\n' "$selected"
    exit 0
fi

old_stty=$(stty -g)
selected=0

restore() {
    stty "$old_stty"
    printf '\033[?25h' >&2
}
trap restore EXIT

stty -icanon -echo min 1 time 0
printf '\033[?25l' >&2
printf '身份（↑/↓选择，Enter 确认，Ctrl+C 取消）：\n' >&2

render() {
    local i prefix
    for i in "${!identities[@]}"; do
        if (( i == selected )); then prefix='❯'; else prefix=' '; fi
        printf '\r\033[K%s %s\n' "$prefix" "${identities[$i]}" >&2
    done
}

render
while IFS= read -r -n1 key; do
    case "$key" in
        $'\x03') exit 130 ;;
        '') break ;;
        $'\x1b')
            IFS= read -r -n2 sequence || true
            case "$sequence" in
                '[A') selected=$(( (selected - 1 + ${#identities[@]}) % ${#identities[@]} )) ;;
                '[B') selected=$(( (selected + 1) % ${#identities[@]} )) ;;
            esac
            printf '\033[%dA' "${#identities[@]}" >&2
            render
            ;;
        k) selected=$(( (selected - 1 + ${#identities[@]}) % ${#identities[@]} )) ;;
        j) selected=$(( (selected + 1) % ${#identities[@]} )) ;;
    esac
done

printf '\033[%dA' "${#identities[@]}" >&2
for i in "${!identities[@]}"; do
    if (( i == selected )); then prefix='✓'; else prefix=' '; fi
    printf '\r\033[K%s %s\n' "$prefix" "${identities[$i]}" >&2
done
printf '%s\n' "${identities[$selected]}"
