#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
C# ProtoBufScript 的参考实现（仅用于交叉验证 Rust 版）。

严格按 MainForm.cs 的 addNotesToCode + formatEmum 逐行翻译，
用来给没有历史输出的样本（bf_ibcp / bf_idcp）生成参考结果。

用法：
    python csharp_ref.py <input.pb.h> <input.proto> <output.pb.h>
"""

import re
import sys


# ---------------------------------------------------------------------------
# MainForm.cs 里的辅助函数
# ---------------------------------------------------------------------------

def is_code_with_notes(s):
    """isCodeWithNotes"""
    index_fh = s.find(';')
    if index_fh == -1:
        return False
    index_szs = s.find('//')
    index_mzs = s.find('/*')
    index_mzs_end = s.find('*/')

    if index_szs != -1 and index_fh < index_szs:
        return True
    if index_mzs != -1 and index_mzs_end > index_mzs and index_fh < index_mzs:
        return True
    return False


def is_pure_notes(s):
    """isPureNotes"""
    index_szs = s.find('//')
    index_mzs = s.find('/*')
    index_mzs_end = s.find('*/')

    if index_szs == 0:
        return True
    if index_mzs == 0 and index_mzs_end > index_mzs:
        return True
    return False


def get_complex_single_note(s):
    """getComplexSingleNote"""
    note = ''
    index_fh = s.find(';')
    index_szs = s.find('//')
    index_mzs = s.find('/*')
    index_mzs_end = s.find('*/')

    if index_szs != -1:
        if index_fh == -1 or index_szs < index_fh:
            if index_mzs == -1:
                note = s[index_szs:]
            elif index_szs < index_mzs:
                note = s[index_szs:]
    else:
        if index_fh == -1 or index_mzs < index_fh:
            if index_mzs_end != -1 and index_mzs != -1 and index_mzs < index_mzs_end:
                note = s[index_mzs:]
    return note


def is_multi_notes_end(s):
    """isMultiNotesEnd"""
    index_mzs_end = s.find('*/')
    if index_mzs_end == -1:
        return False
    if (len(s) - 2) != index_mzs_end:
        return False
    if is_code_with_notes(s):
        return False
    if is_pure_notes(s):
        return False
    return True


def squeeze(s):
    """等价于 Regex.Replace(s, r'\\s', ' ') + Split/Join 压缩"""
    return ' '.join(s.split())


class ProtoNotes:
    __slots__ = ('is_pkg_name', 'pkg_name', 'type', 'para_name', 'up_notes', 'right_notes')

    def __init__(self):
        self.is_pkg_name = False
        self.pkg_name = ''
        self.type = ''
        self.para_name = ''
        self.up_notes = ''
        self.right_notes = ''


def search_para_notes(arr, is_pkg_name, pkg_name, pkg_type, para_name):
    """searchParaNotes"""
    for i, n in enumerate(arr):
        if pkg_name == '':
            continue
        if pkg_type != n.type:
            continue
        if pkg_name != n.pkg_name:
            continue
        if is_pkg_name:
            if n.is_pkg_name:
                return i
        else:
            if para_name == '':
                continue
            if para_name == n.para_name:
                return i
    return -1


def get_notes(proto_lines, start_index, dst, note):
    """getNotes"""
    multi = []
    while start_index >= 0:
        line = proto_lines[start_index]

        if is_pure_notes(line):
            n = start_index
            while n >= 0:
                if is_pure_notes(proto_lines[n]):
                    multi.append(proto_lines[n])
                    n -= 1
                else:
                    break
            start_index = n
        elif is_multi_notes_end(line):
            n = start_index
            while n >= 0:
                ret = proto_lines[n].find('/*')
                if ret == -1:
                    multi.append(proto_lines[n])
                else:
                    multi.append(proto_lines[n][ret:])
                    break
                n -= 1
            start_index = n - 1
            head = multi[-1]
            if '/*' not in head:
                multi.append('/*')
        elif get_complex_single_note(line) != '':
            multi.append(get_complex_single_note(line))
            break
        else:
            break

    if len(multi) > 0:
        pre = '' if note.is_pkg_name else '    '
        for n in range(len(multi) - 1, -1, -1):
            if n == 0:
                note.up_notes += pre + multi[n]
            else:
                note.up_notes += pre + multi[n] + '\r\n'
        dst.append(note)
    else:
        if note.right_notes != '':
            dst.append(note)


def add_blanks_to_tail(s, count):
    return s + ' ' * count


# ---------------------------------------------------------------------------
# addNotesToCode
# ---------------------------------------------------------------------------

def add_notes_to_code(proto_path, pbh_path):
    proto_notes_array = []

    with open(proto_path, 'r', encoding='utf-8') as f:
        raw = f.read().split('\n')
    raw = [l.rstrip('\r') for l in raw]

    # 移除仅含空格的行
    temp = [l for l in raw if l.replace(' ', '') != '']

    pkg_type = ''
    pkg_name = ''

    for i in range(len(temp)):
        temp[i] = temp[i].replace('\t', ' ')
        temp[i] = re.sub(r'\s', ' ', temp[i])
        temp[i] = squeeze(temp[i])

        detail = [x for x in temp[i].strip().split(' ') if x != '']

        if len(detail) == 1 and '}' in temp[i]:
            pkg_type = ''
            pkg_name = ''
            continue

        if len(detail) < 2:
            continue

        if detail[0] in ('message', 'enum'):
            pkg_type = detail[0]
            pkg_name = detail[1]
            if pkg_type == 'message':
                pkg_type = 'struct'

            if i < 1:
                continue

            note = ProtoNotes()
            note.is_pkg_name = True
            note.type = pkg_type
            note.pkg_name = pkg_name
            note.para_name = ''
            note.right_notes = ''
            note.up_notes = ''
            get_notes(temp, i - 1, proto_notes_array, note)
            continue

        elif pkg_name != '':
            if is_pure_notes(temp[i]):
                continue

            field_array = [x for x in re.split(r'[= ]', temp[i]) if x != '']
            if len(field_array) < 2:
                continue

            if pkg_type == 'enum':
                para_name = pkg_name + '_' + field_array[0]
            else:
                para_name = field_array[1]

            right_notes = ''
            index_fh = temp[i].find(';')
            if index_fh != -1 and is_code_with_notes(temp[i]):
                right_notes = temp[i][index_fh + 1:]

            note = ProtoNotes()
            note.is_pkg_name = False
            note.type = pkg_type
            note.pkg_name = pkg_name
            note.para_name = para_name
            note.up_notes = ''
            note.right_notes = right_notes
            get_notes(temp, i - 1, proto_notes_array, note)
            continue

    # 注入
    with open(pbh_path, 'r', encoding='utf-8') as f:
        pbh_lines = f.read().split('\n')
    pbh_lines = [l.rstrip('\r') for l in pbh_lines]

    out = []
    is_find_enum = False
    is_find_struct = False
    pbh_pkg_name = ''
    pbh_para_name = ''
    field_para_max_len = 0

    for i in range(len(pbh_lines)):
        index = -1
        str_write = ''
        detail = [x for x in re.split(r'[ ;]', pbh_lines[i].strip()) if x != '']

        if 'typedef enum' in pbh_lines[i]:
            is_find_enum = True
            pbh_pkg_name = detail[2][1:]
            index = search_para_notes(proto_notes_array, True, pbh_pkg_name, 'enum', '')
            field_para_max_len = 0
            for x in range(i + 1, len(pbh_lines)):
                if pbh_lines[x].find('}') == 0 or pbh_lines[x].find('/*') == 0:
                    break
                field_para_max_len = max(field_para_max_len, len(pbh_lines[x]))
        elif 'typedef struct' in pbh_lines[i]:
            is_find_struct = True
            pbh_pkg_name = detail[2][1:]
            index = search_para_notes(proto_notes_array, True, pbh_pkg_name, 'struct', '')
            field_para_max_len = 0
            for x in range(i + 1, len(pbh_lines)):
                if pbh_lines[x].find('}') == 0 or pbh_lines[x].find('/*') == 0:
                    break
                field_para_max_len = max(field_para_max_len, len(pbh_lines[x]))
        elif is_find_enum or is_find_struct:
            if '}' in pbh_lines[i]:
                is_find_enum = False
                is_find_struct = False
                pbh_pkg_name = ''
                pbh_para_name = ''

            if is_find_enum and len(detail) > 0:
                pbh_para_name = detail[0]
                index = search_para_notes(proto_notes_array, False, pbh_pkg_name, 'enum', pbh_para_name)
            elif is_find_struct and len(detail) > 1:
                pbh_para_name = detail[1]
                index = search_para_notes(proto_notes_array, False, pbh_pkg_name, 'struct', pbh_para_name)

        if index != -1:
            n = proto_notes_array[index]
            str_write = ''
            if n.up_notes != '':
                str_write = n.up_notes + '\r\n'
            str_write += pbh_lines[i]
            if n.right_notes != '':
                cur_len = len(pbh_lines[i])
                if n.type == 'struct':
                    str_write = add_blanks_to_tail(str_write, field_para_max_len - cur_len)
                str_write += n.right_notes
            if n.up_notes != '' and n.right_notes == '' and not n.is_pkg_name:
                str_write += '\r\n'
        else:
            str_write = pbh_lines[i]

        out.append(str_write)

    return out


# ---------------------------------------------------------------------------
# formatEmum
# ---------------------------------------------------------------------------

def get_emum_member_max_len(lines, start_index, member_pre):
    para_name = []
    for i in range(start_index, len(lines)):
        detail = [x for x in lines[i].strip().split(' ') if x != '']
        if len(lines[i]) == 0:
            continue
        if len(detail) >= 2 and member_pre in detail[0] and detail[1] == '=':
            para_name.append(detail[0])
        elif len(detail) == 2:
            if detail[0] == '}':
                break
    return max((len(x) for x in para_name), default=0)


def format_emum(lines):
    enum_name = ''
    member_pre = ''
    out = list(lines)

    for i in range(len(out)):
        detail = [x for x in out[i].strip().split(' ') if x != '']

        if len(detail) >= 3:
            if detail[0] == 'typedef' and detail[1] == 'enum':
                enum_name = detail[2][1:]
                member_pre = member_pre + '_'
                get_emum_member_max_len(out, i + 1, member_pre)
            elif enum_name != '':
                if member_pre in detail[0] and detail[1] == '=':
                    str_name = detail[0]
                    equal_index = out[i].find('=')
                    tail = out[i][equal_index:]
                    cur_len = len(str_name)
                    str_name = add_blanks_to_tail(str_name, _PARA_MAXLEN[0] - cur_len)
                    out[i] = '    ' + str_name + ' ' + tail
        elif len(detail) == 2:
            if detail[0] == '}':
                enum_name = ''
                member_pre = ''

    return out


_PARA_MAXLEN = [0]


def format_emum_full(lines):
    """formatEmum 的完整版：每个枚举块独立计算对齐宽度"""
    enum_name = ''
    member_pre = ''
    para_maxlen = 0
    out = list(lines)

    for i in range(len(out)):
        detail = [x for x in out[i].strip().split(' ') if x != '']

        if len(detail) >= 3:
            if detail[0] == 'typedef' and detail[1] == 'enum':
                enum_name = detail[2][1:]
                member_pre = member_pre + '_'
                para_maxlen = get_emum_member_max_len(out, i + 1, member_pre)
            elif enum_name != '':
                if member_pre in detail[0] and detail[1] == '=':
                    str_name = detail[0]
                    equal_index = out[i].find('=')
                    tail = out[i][equal_index:]
                    cur_len = len(str_name)
                    str_name = add_blanks_to_tail(str_name, para_maxlen - cur_len)
                    out[i] = '    ' + str_name + ' ' + tail
        elif len(detail) == 2:
            if detail[0] == '}':
                enum_name = ''
                member_pre = ''

    return out


def main():
    if len(sys.argv) != 4:
        print(__doc__)
        return 1

    pbh_in, proto, pbh_out = sys.argv[1], sys.argv[2], sys.argv[3]

    lines = add_notes_to_code(proto, pbh_in)
    # 拆回行：注入时可能一个元素含多行（\r\n 分隔）
    flat = []
    for l in lines:
        flat.extend(l.split('\r\n'))

    flat = format_emum_full(flat)

    with open(pbh_out, 'w', encoding='utf-8', newline='\n') as f:
        f.write('\n'.join(flat))
    return 0


if __name__ == '__main__':
    sys.exit(main())
