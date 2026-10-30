"""仅处理密码条目标题，不读取密码内容。"""

import re


def infer_secret_title(text: str) -> str:
    compact = re.sub(r"\s+", "", text)
    match = re.search(r"([\u4e00-\u9fffA-Za-z0-9_.-]+?)(?:账号|账户)?密码", compact, re.I)
    english_match = re.search(r"([A-Za-z0-9_.-]+)\s*(?:password|passwd|passcode)", text, re.I)
    if not match and english_match:
        match = english_match
    title = "密码"
    if match:
        prefix = re.sub(
            r"^(?:(?:重新设置|重置|重设|更新|修改|保存|存入|录入|新增|添加|设置|给我的|给我|给|查一下|查询|看看|查看|看|显示|复制|取出|拿到|帮我|帮忙|请|我的|我|这个|该|my|the))+",
            "",
            match.group(1),
            flags=re.I,
        )
        prefix = re.sub(
            r"(?:(?:账号|账户)?(?:保存|存入|录入|新增|添加|设置|修改|重置|重设|更新))$|(?:账号|账户)$",
            "",
            prefix,
            flags=re.I,
        )
        if prefix:
            title = f"{prefix}密码"
    return title


def matching_entries(paths: list[str], query: str) -> list[str]:
    def key(value: str) -> str:
        value = re.sub(r"\s+", "", value).casefold()
        return re.sub(r"(?:(?:账号|账户)?密码|password|passwd|passcode)$", "", value)

    inferred = infer_secret_title(query)
    term = key(inferred if inferred != "密码" else query)
    return [path for path in paths if term in key(path)]
