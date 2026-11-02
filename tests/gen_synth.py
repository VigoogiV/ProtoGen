#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
生成合成的压力测试样本 synth.proto / synth.options。

为什么需要它：
    公开仓库不能放公司真实产品的协议定义（bf_ibcp / bf_idcp 等），
    但生成器最关键的验证恰恰是「大规模 + 密集注释」这个场景——
    103 个 message、695 处行尾注释的归属是否正确，是注释扫描器唯一的硬指标。

    所以这里造一个规模相当、但内容完全通用的样本：
      - 120 个 message，字段数 5~11 不等
      - 3 个 enum
      - 注释模式轮换：上方块注释 / 行尾块注释 / 行尾单行注释 / 无注释
      - bytes/string 字段在注释里带 [max_size:N]
      - 部分 message 嵌套、部分字段 repeated

用法：
    python tests/gen_synth.py            # 生成到 tests/golden/synth/
"""

import os

OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'golden', 'synth')

MESSAGE_COUNT = 120
ENUM_COUNT = 3
ENUM_VALUES = 4

# 字段类型轮换表：(proto 类型, 名字前缀)
SCALAR_TYPES = [
    ('uint32', 'u32'),
    ('int32', 'i32'),
    ('bool', 'flag'),
    ('uint64', 'u64'),
    ('int64', 'i64'),
    ('fixed32', 'fx32'),
    ('sint32', 's32'),
]


def comment_for(kind, scope, field):
    """按 kind 生成不同风格的注释"""
    if kind == 0:
        return None, f'/** {scope} 的字段 {field}：上方块注释 **/'
    if kind == 1:
        return f'/** {scope}.{field} 行尾块注释 **/', None
    if kind == 2:
        return f'// {scope}.{field} 行尾单行注释', None
    return None, None


def build_proto():
    lines = [
        '// 合成压力测试样本：120 个 message / 3 个 enum / 密集注释',
        '// 由 tests/gen_synth.py 生成，请勿手工修改',
        'syntax = "proto3";',
        '',
        'option optimize_for = LITE_RUNTIME;',
        '',
    ]

    # ---- 枚举 ----
    for e in range(ENUM_COUNT):
        lines.append(f'/** 枚举 ENUM_{e:02d} 的说明 **/')
        lines.append(f'enum ENUM_{e:02d} {{')
        for v in range(ENUM_VALUES):
            # 第一个值必须为 0
            num = 0 if v == 0 else v * 10
            tail = f'    /** 枚举值 {v} 的说明 **/' if v % 2 == 0 else ''
            lines.append(f'    ENUM_{e:02d}_VALUE_{v} = {num};{tail}')
        lines.append('}')
        lines.append('')

    # ---- 消息 ----
    for m in range(MESSAGE_COUNT):
        name = f'MSG_{m:03d}'
        field_count = 5 + (m % 7)

        # 说明：这里刻意不生成嵌套 message。
        # 旧 C# 工具遇到 `}` 会清空当前 message 状态，嵌套场景下注释归属会错乱，
        # 无法用它作为注释基线。嵌套场景由 tests/golden/edge 单独覆盖（只与 nanopb 比对）。
        if m % 3 == 0:
            lines.append(f'// 消息 {name} 的说明（单行）')
        elif m % 3 == 1:
            lines.append(f'/** 消息 {name} 的说明（块注释） **/')

        lines.append(f'message {name} {{')

        for f in range(field_count):
            fname = f'f{f:02d}'
            tag = f + 1
            kind = f % 4
            trailing, leading = comment_for(kind, name, fname)

            if leading:
                lines.append(f'    {leading}')

            # 前两个字段固定用标量，保证每个 message 都能被解析
            if f < 2:
                ptype, prefix = SCALAR_TYPES[(m + f) % len(SCALAR_TYPES)]
                decl = f'    {ptype} {prefix}_{fname} = {tag};'
            elif f == 2:
                # 引用枚举
                e = m % ENUM_COUNT
                decl = f'    ENUM_{e:02d} e_{fname} = {tag};'
            elif f == 3:
                # bytes + max_size（写在注释里，供 options 提取）
                size = 8 + (m % 5) * 4
                note = f'/** [max_size:{size}] 二进制数据 **/'
                if kind == 0:
                    lines.append(f'    {note}')
                    decl = f'    bytes b_{fname} = {tag};'
                else:
                    decl = f'    bytes b_{fname} = {tag};{note}'
            elif f == 4:
                # string + max_size
                size = 16 + (m % 4) * 8
                note = f'/** [max_size:{size}] 文本内容 **/'
                if kind == 0:
                    lines.append(f'    {note}')
                    decl = f'    string s_{fname} = {tag};'
                else:
                    decl = f'    string s_{fname} = {tag};{note}'
            elif f == 6:
                # repeated 标量
                #
                # 注意：旧 C# 工具提取字段名时假设行格式是「类型 名字 ...」，
                # 直接取第 2 个 token，遇到 repeated 会取成类型名（uint32），
                # 导致这类字段的行尾注释永远匹配不上。
                # 这里刻意不给 repeated 加注释，避免把旧工具的缺陷固化进基线；
                # repeated 的类型映射与 callback 退化由 nanopb 比对覆盖。
                trailing = None
                decl = f'    repeated uint32 r_{fname} = {tag};'
            else:
                ptype, prefix = SCALAR_TYPES[(m + f) % len(SCALAR_TYPES)]
                decl = f'    {ptype} {prefix}_{fname} = {tag};'

            if trailing:
                decl += f'    {trailing}'
            lines.append(decl)

        lines.append('}')
        lines.append('')

    return '\n'.join(lines)


def build_options(proto_text):
    """
    从注释里提取 [max_size:N]，生成 .options。

    注意：嵌套 message 的键是点分全名（`MSG_000.MSG_000_NESTED.label`），
    不是下划线 C 名 —— 这是 nanopb 的约定，写错了 options 不会生效。
    """
    import re

    entries = []
    top_msg = None      # 顶层 message 名
    nested_msg = None   # 当前嵌套 message 的完整点分名
    pending_note = None

    for line in proto_text.split('\n'):
        s = line.strip()
        indent = len(line) - len(line.lstrip())

        mm = re.match(r'^message\s+(\w+)\s*\{', s)
        if mm:
            if indent == 0:
                top_msg = mm.group(1)
                nested_msg = None
            elif top_msg:
                nested_msg = f'{top_msg}.{mm.group(1)}'
            pending_note = None
            continue

        if s.startswith('}'):
            if indent == 0:
                top_msg = None
                nested_msg = None
            else:
                nested_msg = None
            pending_note = None
            continue

        scope = nested_msg or top_msg
        if not scope:
            continue

        # 上方注释行（只有注释、没有字段定义）
        if '[max_size:' in s and not re.search(r'=\s*\d+', s):
            pending_note = s
            continue

        fm = re.search(r'\b(bytes|string)\s+(\w+)\s*=\s*\d+', s)
        if not fm:
            pending_note = None
            continue

        field = fm.group(2)
        src = s if '[max_size:' in s else (pending_note or '')
        ms = re.search(r'\[max_size:(\d+)\]', src)
        if ms:
            entries.append((f'{scope}.{field}', int(ms.group(1))))
        pending_note = None

    if not entries:
        return ''

    width = max(len(k) for k, _ in entries)
    out = [f'{key}{" " * (width - len(key))}\t\t\tmax_size:{size}' for key, size in entries]
    return '\n'.join(out) + '\n'


def main():
    os.makedirs(OUT_DIR, exist_ok=True)

    proto_text = build_proto()
    with open(os.path.join(OUT_DIR, 'synth.proto'), 'w', encoding='utf-8', newline='\n') as f:
        f.write(proto_text)

    options_text = build_options(proto_text)
    if options_text:
        with open(os.path.join(OUT_DIR, 'synth.options'), 'w', encoding='ascii', newline='\r\n') as f:
            f.write(options_text)
        print(f'options: {len(options_text.strip().splitlines())} 项')

    msg_count = proto_text.count('\nmessage ') + proto_text.count('\n    message ')
    print(f'proto: {len(proto_text.splitlines())} 行, {msg_count} 个 message')
    print(f'输出目录: {OUT_DIR}')


if __name__ == '__main__':
    main()
