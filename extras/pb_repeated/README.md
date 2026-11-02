# pb_repeated — nanopb repeated 字段的通用编解码回调

给 proto 里的 `repeated` 字段提供一套可复用的 encode/decode 回调，不用每个字段手写一遍。

## 先判断：你可能不需要这个模块

nanopb 会根据 **有没有 `max_count`** 生成两种完全不同的结构体：

| proto 写法 | 生成的结构体 | 编解码 |
|---|---|---|
| `repeated Item items = 1;` + options 里 `max_count:N` | `pb_size_t items_count; Item_t items[N];` | **全自动**，`pb_encode` / `pb_decode` 直接处理 |
| `repeated Item items = 1;`（无 max_count） | `pb_callback_t items;` | **必须自己写回调** |

**第一种情况不需要本模块**——直接编解码就行，条数在 `items_count` 里。
这时唯一的不便是「填充数组」有点啰嗦，用本模块提供的 `PB_REPEATED_SET` / `PB_REPEATED_PUSH` 宏可以省点事。

**第二种情况才是本模块的用武之地**：没有 max_count 时，每个 repeated 字段都要手写
「遍历数组逐个 `pb_encode_submessage`」和「循环 `pb_decode` 直到流结束」，
重复且容易漏边界判断。

## 用法

```c
#include "pb_repeated.h"
#include "my_msg.pb.h"

Item_t items[8];
pb_repeated_t ctx;
Container msg = Container_init_zero;

/* ---- 编码 ---- */
items[0].id = 1;  strcpy(items[0].name, "a");
items[1].id = 2;  strcpy(items[1].name, "b");

pb_repeated_init(&ctx, items, sizeof(Item_t), 8);   /* 数组、元素大小、容量 */
ctx.count = 2;                                       /* 实际有几条 */

PB_REPEATED_BIND_ENCODE(msg, items, &ctx);           /* 绑到字段上 */
pb_msg_encode(buf, sizeof(buf), &msg, Container_fields, &len, NULL);

/* ---- 解码 ---- */
Container out = Container_init_zero;
pb_repeated_init(&ctx, items, sizeof(Item_t), 8);    /* count 会从 0 开始 */
PB_REPEATED_BIND_DECODE(out, items, &ctx);

pb_msg_decode(buf, len, &out, Container_fields, NULL);
/* ctx.count 就是实际解出的条数 */
```

`pb_msg_encode` / `pb_msg_decode` 来自配套的 `extras/pb_array`，
也可以直接用 nanopb 原生的 `pb_encode` / `pb_decode`，效果一样。

## 接口

```c
/* 上下文：描述「数组在哪、元素多大、最多几条」 */
typedef struct {
    void     *array;      /* 元素数组首地址 */
    uint16_t  elem_size;  /* sizeof(元素类型) */
    uint16_t  max_count;  /* 容量 */
    uint16_t  count;      /* 实际条数：编码前设置，解码后读取 */
    int32_t   err;        /* 失败原因 */
} pb_repeated_t;

void pb_repeated_init(pb_repeated_t *ctx, void *array,
                      uint16_t elem_size, uint16_t max_count);

bool pb_repeated_encode_cb(pb_ostream_t *stream, const pb_field_t *field, void * const *arg);
bool pb_repeated_decode_cb(pb_istream_t *stream, const pb_field_t *field, void **arg);

const char *pb_repeated_strerror(int32_t code);
```

### 错误码

| 码 | 含义 |
|---|---|
| `PB_REPEATED_OK` (0) | 成功 |
| `PB_REPEATED_ERR_PARAM` (-1) | 入参非法（array 为 NULL、elem_size 为 0、count > max_count） |
| `PB_REPEATED_ERR_FULL` (-2) | 解码时条数超过 max_count |
| `PB_REPEATED_ERR_ENCODE` (-3) | 编码回调失败 |
| `PB_REPEATED_ERR_DECODE` (-4) | 解码回调失败（数据非法） |

回调失败时 `pb_encode` / `pb_decode` 返回 false，具体原因看 `ctx.err`：

```c
if (!pb_decode(&is, Container_fields, &out)) {
    printf("失败: %s（已解 %u 条）\n", pb_repeated_strerror(ctx.err), ctx.count);
}
```

## 静态 repeated 的便捷宏

proto 里给了 `max_count` 时，结构体是静态数组，不需要回调，但填充仍嫌啰嗦：

```c
/* 一次性拷贝 n 个元素 */
PB_REPEATED_SET(msg, items, src_arr, 5);

/* 追加一条，满了返回 false（不会越界写） */
if (!PB_REPEATED_PUSH(msg, items, item)) {
    /* 数组满了 */
}

/* 容量与当前条数 */
uint16_t cap = PB_REPEATED_CAP(msg, items);
uint16_t n   = PB_REPEATED_COUNT(msg, items);
```

## 注意事项

1. **只支持 repeated message 字段**。标量 repeated（`repeated uint32`）的 callback 形态
   需要按 varint/fixed 分类型处理，本模块不覆盖——那种情况建议直接加 `max_count` 变成静态数组，
   没必要走回调。
2. **数组必须连续存放**。`Item_t arr[N]` 可以，`Item_t *arr[N]` 不行。
3. **`elem_size` 必须传 `sizeof(元素类型)`**，传小了元素会错位。
4. **解码时自动 memset 每个元素**，等价于 `*_init_zero`。编码时不会动你的数据。
5. **解码后 `ctx.count` 可能小于数据里的条数**（容量不足时）。这时 `ctx.err` 是 `FULL`，
   前面的元素是有效的，后面的没解出来。
6. **不分配内存**，数组由调用方提供。

## 测试

```bash
NB=/path/to/nanopb

gcc -std=c99 -Wall -Wextra -Werror -I"$NB" -I. \
    test_pb_repeated.c pb_repeated.c test_repeated.pb.c \
    "$NB/pb_common.c" "$NB/pb_encode.c" "$NB/pb_decode.c" \
    -o test_pb_repeated.exe

./test_pb_repeated.exe
```

覆盖 5 项：

| # | 场景 | 验证点 |
|---|------|--------|
| 1 | 3 条 message 往返 | 字段值、条数 |
| 2 | 空 repeated（count=0） | 编码长度 0，解码得 0 条 |
| 3 | 目标数组容量不足 | 返回 `FULL`，已填部分保留 |
| 4 | 与手工逐个编码对比 | **字节完全一致**（证明 wire format 正确） |
| 5 | 参数校验 | array 为 NULL、elem_size=0、count>max_count |

实测输出（gcc 7.3 / MinGW，nanopb 0.3.9.1）：

```
[1] repeated message 编解码往返
  3 条共编码 32 字节
[3] 目标数组容量不足
  错误: array full（已填 2 条）
[4] 与手工逐个编码的字节一致
  两种写法都是 19 字节，内容相同
================================
全部 5 项通过
```

第 4 项是最关键的：手工按 `pb_encode_tag_for_field` + `pb_encode_submessage` 写一遍，
和回调输出逐字节比对，确认格式没跑偏。

## 和 pb_array 的区别

| 模块 | 解决什么 |
|---|---|
| `extras/pb_repeated`（本模块） | **一条消息内部**的 repeated 字段怎么编解码 |
| `extras/pb_array` | **多条独立消息**拼进一个缓冲区后怎么切分（带 varint 长度前缀） |

两者解决的是不同问题，可以同时用：
`pb_array` 负责消息之间，`pb_repeated` 负责消息内部。
