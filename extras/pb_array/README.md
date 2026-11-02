# pb_array — nanopb 消息与数组的通用编解码封装

把 nanopb 那套「建 stream → 判断返回值 → 取字节数」的样板代码收进两个函数，
并解决**多条消息拼进一个缓冲区后无法切分**的问题。

## 为什么需要

nanopb 原生的写法，每编解码一次都要重复：

```c
pb_ostream_t os = pb_ostream_from_buffer(buf, sizeof(buf));
if (!pb_encode(&os, Msg_fields, &msg)) {
    log("failed: %s", PB_GET_ERROR(&os));
    return FAIL;
}
uint16_t len = (uint16_t)os.bytes_written;
```

单条还好，数组就麻烦了：protobuf 编码后的长度是**可变**的，
把 `Msg arr[10]` 依次编码进缓冲区后，接收方没有任何依据判断第一条在哪里结束。

## 线格式

每条消息前加一个 varint 长度前缀：

```
[len1:varint][payload1][len2:varint][payload2]...
```

- 单条消息通常几十到几百字节，前缀只占 **1~2 字节**
- 自描述，接收方逐条切分，不需要提前知道条数和每条的边界
- varint 与 protobuf 自身的编码一致，不引入新概念

## 接口

```c
/* 单条 */
int32_t pb_msg_encode(uint8_t *dst, uint32_t dst_cap,
                      const void *msg, const pb_field_t fields[],
                      uint32_t *out_len, pb_array_err_t *err);

int32_t pb_msg_decode(const uint8_t *src, uint32_t src_len,
                      void *msg, const pb_field_t fields[],
                      pb_array_err_t *err);

/* 数组 */
int32_t pb_array_encode(uint8_t *dst, uint32_t dst_cap,
                        const void *arr, uint16_t count, uint16_t elem_size,
                        const pb_field_t fields[],
                        uint32_t *out_len, pb_array_err_t *err);

int32_t pb_array_decode(const uint8_t *src, uint32_t src_len,
                        void *arr, uint16_t max_count, uint16_t elem_size,
                        const pb_field_t fields[],
                        uint16_t *out_count, pb_array_err_t *err);
```

`err` 可以传 `NULL`。传了的话，出错时能拿到错误码、出错的元素下标、以及 nanopb 的原始错误描述：

```c
pb_array_err_t err;
if (pb_array_encode(buf, sizeof(buf), arr, 10, sizeof(arr[0]),
                    Msg_fields, &len, &err) != PB_ARRAY_OK) {
    printf("第 %u 条失败: %s\n", err.index, err.message);
}
```

### 错误码

| 码 | 含义 |
|---|---|
| `PB_ARRAY_OK` (0) | 成功 |
| `PB_ARRAY_ERR_PARAM` (-1) | 入参非法（空指针、`elem_size` 为 0） |
| `PB_ARRAY_ERR_BUF_SMALL` (-2) | 连长度前缀都放不下 |
| `PB_ARRAY_ERR_ENCODE` (-3) | `pb_encode` 失败（多半是缓冲区不足） |
| `PB_ARRAY_ERR_DECODE` (-4) | `pb_decode` 失败（数据格式非法） |
| `PB_ARRAY_ERR_TRUNCATED` (-5) | 数据被截断 |
| `PB_ARRAY_ERR_TOO_MANY` (-6) | 解出的条数超过目标数组容量 |

`pb_array_strerror(code)` 返回可读描述。

### 便捷宏

```c
PB_ARRAY_ENCODE(buf, sizeof(buf), arr, Msg_fields, &len);
PB_ARRAY_DECODE(buf, len, out, Msg_fields, &count);
```

⚠️ **只能用于真正的数组**。如果 `arr` 是函数参数（已经退化成指针），
`sizeof(arr)/sizeof(arr[0])` 会算成指针大小，导致只编码 1 条且长度错误。
这种场景请直接调用 `pb_array_encode` 并显式传 `count`。

## 用法

```c
#include "pb_array.h"
#include "my_msg.pb.h"

void demo(void)
{
    MyMsg_t arr[10];
    uint8_t  buf[1024];
    uint32_t len = 0;
    pb_array_err_t err;

    /* ... 填充 arr ... */

    /* 编码 */
    if (pb_array_encode(buf, sizeof(buf), arr, 10, sizeof(arr[0]),
                        MyMsg_fields, &len, &err) != PB_ARRAY_OK) {
        printf("编码失败: 第 %u 条 %s\n", err.index, err.message);
        return;
    }

    /* 解码 */
    MyMsg_t out[10];
    uint16_t count = 0;
    if (pb_array_decode(buf, len, out, 10, sizeof(out[0]),
                        MyMsg_fields, &count, &err) != PB_ARRAY_OK) {
        printf("解码失败: 第 %u 条 %s\n", err.index, err.message);
        return;
    }
    printf("收到 %u 条\n", count);
}
```

## 从现有封装迁移

如果工程里已经有类似 `xxx_pb_data_encode` / `xxx_rx_pkg_decode` 的函数：

| 原来 | 换成 |
|---|---|
| `mmi_ibcp_pb_data_encode(buf, len, &msg, Msg_fields, &out)` | `pb_msg_encode(buf, len, &msg, Msg_fields, &out, NULL)` |
| `mmi_ibcp_rx_pkg_decode(data, len, &msg, Msg_fields)` | `pb_msg_decode(data, len, &msg, Msg_fields, NULL)` |

返回值从 `SUCC/FAIL` 变成 `PB_ARRAY_OK` / 负错误码，判断方式：

```c
/* 原来 */
if (SUCC != mmi_ibcp_pb_data_encode(...)) { ... }

/* 换成 */
if (PB_ARRAY_OK != pb_msg_encode(...)) { ... }
```

如果想要原来的语义，包一层即可：

```c
#define SUCC 0
#define FAIL (-1)
static inline int32_t my_encode(uint8_t *d, uint16_t cap, const void *m,
                                const pb_field_t f[], uint16_t *out)
{
    uint32_t n = 0;
    if (pb_msg_encode(d, cap, m, f, &n, NULL) != PB_ARRAY_OK) return FAIL;
    *out = (uint16_t)n;
    return SUCC;
}
```

## 注意事项

1. **`elem_size` 必须传 `sizeof(元素类型)`**。传小了元素会错位，传大了读越界。
   用 `PB_ARRAY_ELEM_SIZE(arr)` 宏最稳。
2. **数组元素必须在内存中连续**。`MyMsg_t arr[10]` 可以，
   指针数组 `MyMsg_t *arr[10]` 不行。
3. **`pb_msg_decode` 不会清零目标结构体**——它不知道结构体多大。
   调用前请自己初始化：`MyMsg_t msg = MyMsg_init_zero;`
   `pb_array_decode` 知道 `elem_size`，会自动 memset，等价于 `*_init_zero`。
4. **编码时每条消息会做一次 `memmove`**（把 payload 前移覆盖预留的前缀空间）。
   量级是 payload 长度，几百字节的 `memmove` 在 MCU 上是微秒级，可忽略。
   如果在意，可以改成两遍编码，但代码复杂度不值得。
5. **本模块不分配内存**，缓冲区全部由调用方提供。
6. 类型用的是 `<stdint.h>` 的 `uint8_t` / `uint16_t` / `uint32_t` / `int32_t`。
   如果工程里有自己的 `uint8` / `uint16` 定义，在头文件里加一层 typedef 映射即可。

## 测试

```bash
# 需要 gcc（MinGW 也可以）和 nanopb 源码
NB=/path/to/nanopb

gcc -std=c99 -Wall -Wextra -I"$NB" -I<生成的.pb.h目录> \
    test_pb_array.c pb_array.c <某个>.pb.c \
    "$NB/pb_common.c" "$NB/pb_encode.c" "$NB/pb_decode.c" \
    -o test_pb_array.exe

./test_pb_array.exe
```

覆盖 8 项：

| # | 场景 | 验证点 |
|---|------|--------|
| 1 | 单条往返 | 字段值、变长字符串 |
| 2 | 数组往返（5 条，长度各不相同） | 前缀逐条自适应 |
| 3 | 空数组 | `count=0` 时长度为 0，不报错 |
| 4 | 目标缓冲区不足 | 返回 `ENCODE` / `BUF_SMALL` |
| 5 | 数据截断 | 返回 `TRUNCATED` |
| 6 | 目标数组容量不足 | 返回 `TOO_MANY` |
| 7 | 前缀布局 | 手工按 varint 走一遍，正好走到末尾 |
| 8 | 参数校验 | 空指针、`elem_size=0` 返回 `PARAM` |

实测输出（gcc 7.3 / MinGW，nanopb 0.3.9.1）：

```
[1] 单条消息编解码往返
  编码 9 字节, a=42, s=hello
[2] 数组编解码往返（变长元素）
  5 条共编码 36 字节
[4] 目标缓冲区不足
  错误码 -3: stream full
[5] 数据被截断
  错误码 -5: payload shorter than prefix says
[6] 目标数组容量不足
  错误码 -6: destination array full
[7] 长度前缀布局校验
  3 条前缀解析后正好走完 21 字节
================================
全部 8 项通过
```
