#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
把改过的 C# options 生成逻辑逐行翻译成 Python，用来交叉验证。

因为本机安全策略禁止执行 MSBuild/csc，无法直接编译 C# 验证，
所以用这份等价实现跑同一份 proto，与 protogen 的输出对比。
只要两边内容一致，说明 C# 的算法改对了。
"""

import re
import sys


def squeeze(s):
    """等价 C# 的 Regex.Replace(s, r'\\s', ' ') + Split/Join"""
    return ' '.join(s.split())


def split_field(line):
    """等价 C# 的 Split(new char[]{' ','=','{'})，保留空项"""
    return re.split(r'[ ={]', line)


def get_bytes_and_string_paras(lines):
    """等价 getBytesAndStringTypeParas（含新增的 repeated string/bytes 支持）"""
    names, sizes = [], []
    message = ''
    upline_maxsize = ''

    for raw in lines:
        line = squeeze(re.sub(r'\s', ' ', raw))
        tokens = split_field(line)
        if len(tokens) < 2:
            continue

        if tokens[0] == 'message':
            message = tokens[1]
            continue

        is_bs = (tokens[0] in ('bytes', 'string')) or (
            tokens[0] == 'repeated' and len(tokens) >= 3
            and tokens[1] in ('bytes', 'string')
        )

        if is_bs:
            idx = 2 if tokens[0] == 'repeated' else 1
            names.append(message + '.' + tokens[idx])

            noblank = ''.join(line.split())
            parts = re.split(r'[\[\]:]', noblank)
            temp = 'max_size:'

            if len(parts) >= 3:
                for k in range(len(parts)):
                    if parts[k] == 'max_size' and k + 1 < len(parts):
                        temp += parts[k + 1]
                        break
                sizes.append(temp)
            else:
                if upline_maxsize:
                    temp += upline_maxsize
                    upline_maxsize = ''
                sizes.append(temp)
        else:
            noblank = ''.join(line.split())
            parts = re.split(r'[\[\]:]', noblank)
            if len(parts) >= 3:
                for k in range(len(parts)):
                    if parts[k] == 'max_size' and k + 1 < len(parts):
                        upline_maxsize = parts[k + 1]
                        break

    return names, sizes


def get_repeated_paras(lines):
    """等价 getRepeatedTypeParas（新增）"""
    names, counts = [], []
    message = ''
    upline_maxcount = ''

    for raw in lines:
        line = squeeze(re.sub(r'\s', ' ', raw))
        tokens = split_field(line)
        if len(tokens) < 2:
            continue

        if tokens[0] == 'message':
            message = tokens[1]
            upline_maxcount = ''
            continue

        if tokens[0] == 'repeated' and len(tokens) >= 3 and message:
            para_name = message + '.' + tokens[2]

            noblank = ''.join(line.split())
            parts = re.split(r'[\[\]:]', noblank)
            count_value = ''

            if len(parts) >= 3:
                for k in range(len(parts)):
                    if parts[k] == 'max_count' and k + 1 < len(parts):
                        count_value = parts[k + 1]
                        break

            if not count_value and upline_maxcount:
                count_value = upline_maxcount
            upline_maxcount = ''

            if count_value:
                names.append(para_name)
                counts.append('max_count:' + count_value)
        else:
            noblank = ''.join(line.split())
            parts = re.split(r'[\[\]:]', noblank)
            if len(parts) >= 3:
                for k in range(len(parts)):
                    if parts[k] == 'max_count' and k + 1 < len(parts):
                        upline_maxcount = parts[k + 1]
                        break

    return names, counts


def collect_options_entries(lines):
    """等价 collectOptionsEntries（新增）"""
    entries = []          # [(name, max_size, max_count)]

    names, sizes = get_bytes_and_string_paras(lines)
    for i in range(len(names)):
        entries.append([names[i], sizes[i], None])

    rnames, rcounts = get_repeated_paras(lines)
    for i in range(len(rnames)):
        name = rnames[i]
        found = None
        for e in entries:
            if e[0] == name:
                found = e
                break
        if found is not None:
            found[2] = rcounts[i]
        else:
            entries.append([name, None, rcounts[i]])

    return entries


def render(entries):
    """等价 btnCreateOptionsFile_Click 的写出逻辑"""
    if not entries:
        return ''

    max_len = max(len(e[0]) for e in entries)
    out = []
    for name, max_size, max_count in entries:
        line = name + ' ' * (max_len - len(name))
        value = ''
        if max_size is not None:
            value = max_size
        if max_count is not None:
            if value:
                value += ' '
            value += max_count
        out.append(line + '\t\t\t' + value)
    return '\n'.join(out) + '\n'


def main():
    if len(sys.argv) != 2:
        print(__doc__)
        return 1

    with open(sys.argv[1], 'r', encoding='utf-8') as f:
        lines = f.read().split('\n')

    entries = collect_options_entries(lines)
    sys.stdout.write(render(entries))
    return 0


if __name__ == '__main__':
    sys.exit(main())
