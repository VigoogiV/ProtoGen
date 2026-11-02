# ProtoGen

用 Rust 重写的 proto → nanopb C 代码生成工具，替代一套内部的 C# WinForms 工具。

生成的可执行文件叫 `protogen`，对应的 crate 是 `protogen-core`（纯库）和 `protogen-cli`（命令行）。

**单一可执行文件，零外部依赖**——不再需要 `protoc.exe`、Python 2.7 运行时或 .NET Framework。

## 它做什么

读一个 `.proto`，产出三样东西：

1. **`.pb.c` / `.pb.h`** —— nanopb 0.3.9.1 风格的 C 代码
2. **注释同步** —— 把 `.proto` 里的注释写进 `.pb.h` 对应位置，枚举值自动对齐
3. **`.options`** —— 从注释里的 `[max_size:N]` / `[max_count:N]` 生成模板，并可校验是否漏填

输出与旧 C# 工具**逐字节一致**（见"验证"一节）。

## 快速开始

**三种启动方式：**

1. **双击 `protogen.exe`** —— 进入菜单式交互界面（不闪退），可以选文件、生成、校验
2. **把 `.proto` 拖到 `protogen.exe` 图标上** —— 直接生成，结束后停住显示结果
3. **命令行** —— 见下方

```bash
# 生成 .pb.c / .pb.h（默认注入注释、对齐枚举、GB2312 编码）
protogen gen example.proto

# 只生成代码，不动注释
protogen gen example.proto --no-notes --no-align

# 生成 .options 模板
protogen options example.proto

# 校验 .options 是否覆盖了所有 bytes/string 字段
protogen check example.proto
```

### 交互模式

无参数双击时，**直接弹出系统文件选择框**：

```
  ══════════════════════════════════════════════════
   protogen  ·  proto → nanopb C 代码
   注释同步 / 枚举对齐 / .options 生成与校验
  ══════════════════════════════════════════════════

  即将打开文件选择框；取消后也可以手动输入路径。
```

选完文件后：

```
  ── 请选择操作 ────────────────────────
    1) 生成 .pb.c / .pb.h（含注释同步）
    2) 生成 .options 模板
    3) 校验 .options 是否完整
    4) 换个 .proto 文件
    0) 退出
  ──────────────────────────────────────
```

生成完会停住等按回车，不会一闪而过。

> **为什么用文件对话框而不是拖拽？**
> Windows 的 conhost 控制台**不支持**把文件拖进窗口（只有 Windows Terminal 会插入路径），
> 所以拖拽靠不住。改用 Win32 `GetOpenFileNameW` 弹系统对话框，双击用户按一下就能选。
> 另外把 `.proto` 直接拖到 **exe 图标**上也是支持的。
>
> 脚本/自动化场景可以设环境变量 `PROTOGEN_NO_DIALOG=1` 跳过对话框，改走手动输入路径。

## 命令参考

### `protogen gen <proto>`

| 参数 | 说明 |
|------|------|
| `-I, --include <dir>` | 额外的 import 搜索目录（可多次） |
| `-o, --out <dir>` | 输出目录，默认与 proto 同目录 |
| `--options <file>` | 指定 `.options`，默认找同名文件 |
| `--no-notes` | 不注入注释 |
| `--no-align` | 不做枚举值对齐 |
| `--encoding <gb2312\|utf8>` | 输出编码，默认 `gb2312`（兼容旧工程） |
| `--lossy` | GB2312 无法表示的字符降级为 `?`（默认报错） |
| `--timestamp <str>` | 写入生成时间；默认不写，保证可复现 |

### `protogen options <proto>`

生成 `.options` 模板。`--stdout` 只打印不写文件（此时**内容走 stdout、日志走 stderr**，方便脚本分离）。

从 proto 注释里提取两类上限，行尾或上方注释都可以：

| 字段类型 | 提取的键 | 注释写法 |
|---|---|---|
| `bytes` / `string` | `max_size:N` | `/** [max_size:64] 说明 **/` |
| `repeated` | `max_count:N` | `/** [max_count:8] 说明 **/` |
| `repeated string` / `repeated bytes` | 两个都要 | `/** [max_size:16] [max_count:6] **/` |

```proto
message Item {
    uint32 id = 1;
    string name = 2;            /** [max_size:32] 名字 **/
}

message Container {
    /** [max_count:8] 条目列表 **/
    repeated Item items = 1;
    repeated string tags = 2;   /** [max_size:16] [max_count:6] 标签 **/
}
```

生成：

```
Item.name      			max_size:32
Container.items			max_count:8
Container.tags 			max_size:16 max_count:6
```

`repeated` 配了 `max_count` 才会生成静态数组（`Item items[8]`）；
不配就是 `pb_callback_t`，仍可用但需要自己写回调。

### `protogen check <proto>`

校验 `.options`。退出码非 0 表示有问题，可直接用在 CI 里。

## 生成日志

每次操作都会输出结构化日志，`[INFO]` / `[WARN]` / `[ERROR]` 前缀方便着色和过滤。

```
[INFO] 输入: synth.proto (67.4 KB)
[INFO] options: synth.options (240 项)
[INFO] 解析: 120 message / 3 enum / 957 field (9 ms)
[INFO] 生成 .pb.h: 3491 行
[INFO] 生成 .pb.c: 1454 行
[INFO] 注释同步: 已开启（810 处）
[INFO] 枚举对齐: 3 个枚举
[INFO] 编码: GB2312 / CRLF
[INFO] 输出: synth.pb.h
[INFO] 输出: synth.pb.c
[INFO] 完成，耗时 17 ms
```

会报警告的典型情况：

```
[WARN] 未找到 .options，bytes/string 字段会退化为 callback
[WARN] 1 个 bytes/string 字段没有 max_size，已退化为 callback:
[WARN]   - msg_pkg_t.body_data
```

```
[WARN] 缺少 max_size 定义（2 项）:
  - Foo.bar
  - Baz.qux
```

用 `--no-notes` 关掉注释同步时，日志会显示 `注释同步: 已关闭`，便于确认当前行为。

## 编码说明

| 文件 | 编码 | 换行 |
|------|------|------|
| `.proto`（输入） | UTF-8 | LF |
| `.pb.h` / `.pb.c`（输出） | GB2312 | CRLF |
| `.options`（输出） | ASCII | CRLF |

旧工程的 `.pb.h` 是 GB2312，默认保持这个行为，避免已有工程 diff 爆炸、老 IDE 乱码。
新工程可以用 `--encoding utf8`。

GB2312 表示不了的字符（emoji、部分生僻字）默认**报错**，不会像旧工具那样静默写成 `?`。

## 验证

`tests/golden/` 下固化了 5 个样本，三层比对：

```bash
cargo test
```

| 测试 | 比对对象 | 验证什么 |
|------|----------|----------|
| `golden_nanopb_output_matches` | nanopb 0.3.9.1 的原始输出 | 生成器本身是否正确 |
| `golden_notes_injection_matches_csharp` | 旧 C# 工具注入注释后的输出 | 注释同步是否兼容 |
| `golden_options_generation_matches` | 确认过的 `.options` | options 生成是否正确 |

样本：

| 样本 | 来源 | 覆盖 |
|------|------|------|
| `simple` / `fileproto` / `MyProtoFilesfileproto` | nanopb 官方示例 | 基础类型、枚举、bytes/string |
| `edge` | 手工构造 | package、嵌套 message、repeated、proto2 默认值、callback、double |
| `synth` | `tests/gen_synth.py` 生成 | 规模压力：120 message / 957 字段 / 810 处注释 |

`synth` 是合成的压力样本——注释扫描器最关键的验证是「大规模 + 密集行尾注释」的归属是否正确，
但真实业务 proto 不能公开，所以造了个规模相当、内容完全通用的替代品。

`tests/reference/csharp_ref.py` 是旧 C# 工具逻辑的 Python 逐行翻译，用来生成注释基线。
它**保留了两个已知缺陷**：

- 遇到 `}` 就清空当前 message 状态，嵌套 message 场景下注释归属错乱
- 提取字段名时假设行格式是「类型 名字」，`repeated` 字段会取成类型名，行尾注释匹配不上

所以 `synth` 刻意避开了这两处，嵌套场景由 `edge` 单独覆盖（只与 nanopb 比对）。
新实现在这两点上比旧工具正确，属于有意为之的行为差异。

## 配套：C 端编解码封装

`extras/` 下有两个可选的 C 模块，补 nanopb 用起来啰嗦的两处。

### `extras/pb_repeated` —— repeated 字段

proto 里的 repeated 字段如果**没有 `max_count`**，nanopb 会生成 `pb_callback_t`，
每个字段都得手写「遍历数组逐个 `pb_encode_submessage`」和「循环 `pb_decode` 到流结束」。
这个模块把这两段做成通用回调：

```c
pb_repeated_t ctx;
pb_repeated_init(&ctx, items, sizeof(Item_t), 8);   /* 数组、元素大小、容量 */
ctx.count = 5;                                       /* 编码前指定条数 */

PB_REPEATED_BIND_ENCODE(msg, items, &ctx);
pb_msg_encode(buf, sizeof(buf), &msg, Container_fields, &len, NULL);

/* 解码：绑定后 ctx.count 就是实际条数 */
PB_REPEATED_BIND_DECODE(out, items, &ctx);
pb_msg_decode(buf, len, &out, Container_fields, NULL);
```

> 如果 repeated 字段**有** `max_count`（生成静态数组），nanopb 本来就全自动处理，
> 不需要这个模块——那种情况只有「填充数组」啰嗦，用 `PB_REPEATED_SET` / `PB_REPEATED_PUSH` 宏即可。

详见 [extras/pb_repeated/README.md](extras/pb_repeated/README.md)。

### `extras/pb_array` —— 多条消息拼接

把**多条独立消息**编进一个缓冲区，每条前加 varint 长度前缀，接收方逐条切分：

```c
pb_array_encode(buf, sizeof(buf), arr, 10, sizeof(arr[0]), Msg_fields, &len, NULL);
pb_array_decode(buf, len, out, 10, sizeof(out[0]), Msg_fields, &count, NULL);
```

线格式：`[len1:varint][payload1][len2:varint][payload2]...`

详见 [extras/pb_array/README.md](extras/pb_array/README.md)。

两个模块解决的是不同层面的问题，可以同时用：`pb_array` 管消息之间，`pb_repeated` 管消息内部。
都在 gcc 7.3 + nanopb 0.3.9.1 上用 `-Werror` 验证过。

## 架构

```
crates/
├── protogen-core/          # 纯库，无 IO 副作用
│   ├── model.rs            # IR：Message / Field / Enum + 注释 + 行号
│   ├── comments.rs         # 源码注释扫描
│   ├── parse.rs            # protox → IR
│   ├── options.rs          # .options 生成与校验
│   ├── encode.rs           # GB2312 / CRLF
│   └── gen/
│       ├── render.rs       # nanopb C 代码渲染
│       └── comments_inject.rs  # 注释注入 + 枚举对齐
└── protogen-cli/           # 命令行入口
```

### 两个关键设计

**1. 不用 `SourceCodeInfo` 的注释归属**

protoc 会把行尾的 `/** ... **/` 块注释算成**下一个字段**的 leading comment。
`example.proto` 里 818 行注释有 695 行是行尾注释，按 descriptor 归属会整体串行。

所以注释由 `comments.rs` 自己按行扫描，元素位置靠 `Location.span` 的行号对齐——
既有编译器给的行号，又不依赖它错误的归属判断。

**2. 分两步生成**

先生成 nanopb 原样的 C 代码，再做注释注入 + 枚举对齐。
这和旧工具的两步流程一致，也让"生成是否正确"和"注释是否兼容"可以分开验证。

## 作为 BfTools 模块使用

推荐 CLI 子进程方式（与 `server/services/chm/chm-service.js` 调 `hh.exe` 的模式一致）。
详见 [docs/integration-bftools.md](docs/integration-bftools.md)。
