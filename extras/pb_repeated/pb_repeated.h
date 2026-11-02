/**
 * @file    pb_repeated.h
 * @brief   nanopb repeated 字段的通用编解码回调
 *
 * 解决的问题
 * ----------
 * proto 里的 repeated 字段，nanopb 会按有没有 max_count 生成两种形态：
 *
 *   1. 有 max_count → 静态数组，编解码全自动：
 *          pb_size_t items_count;
 *          Item_t items[MAX];
 *      这种不需要本模块，直接用 pb_encode / pb_decode 即可。
 *
 *   2. 没有 max_count → pb_callback_t，必须自己实现两个回调：
 *          pb_callback_t items;
 *      每个 repeated 字段都要手写一遍「遍历数组逐个 pb_encode_submessage」
 *      和「循环 pb_decode 直到流结束」，重复且容易漏掉边界判断。
 *
 * 本模块就是第 2 种情况的通用实现：用 pb_repeated_t 描述「数组在哪、元素多大、
 * 最多几条」，然后把它绑到字段的 arg 上，编解码逻辑复用同一套回调。
 *
 * 用法
 * ----
 *     Item_t items[8];
 *     pb_repeated_t ctx;
 *     Container msg = Container_init_zero;
 *
 *     // 编码：先填数据，再指定条数
 *     items[0].id = 1;  strcpy(items[0].name, "a");
 *     items[1].id = 2;  strcpy(items[1].name, "b");
 *     pb_repeated_init(&ctx, items, sizeof(Item_t), 8);
 *     ctx.count = 2;
 *
 *     PB_REPEATED_BIND_ENCODE(msg, items, &ctx);
 *     pb_msg_encode(buf, sizeof(buf), &msg, Container_fields, &len, NULL);
 *
 *     // 解码：绑定后 count 会被填成实际条数
 *     Container out = Container_init_zero;
 *     pb_repeated_init(&ctx, items, sizeof(Item_t), 8);
 *     PB_REPEATED_BIND_DECODE(out, items, &ctx);
 *     pb_msg_decode(buf, len, &out, Container_fields, NULL);
 *     // ctx.count 即解出的条数
 *
 * 注意
 * ----
 *   - 只适用于 **repeated message** 字段（子消息）。标量 repeated（如
 *     repeated uint32）的 callback 形态需要自己处理 varint，建议直接给
 *     proto 加 max_count 让它变成静态数组，没必要走回调。
 *   - 数组必须在内存中连续（Item_t arr[N]），不能是指针数组。
 *   - 解码时会自动 memset 每个元素，等价于 *_init_zero。
 *   - elem_size 必须传 sizeof(元素类型)，传小了元素会错位。
 */

#ifndef PB_REPEATED_H
#define PB_REPEATED_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include <string.h>   /* PB_REPEATED_SET 用到 memcpy */

#include <pb.h>
#include <pb_encode.h>
#include <pb_decode.h>

#ifdef __cplusplus
extern "C" {
#endif

/* -------------------------------------------------------------------------
 * 错误码
 * ------------------------------------------------------------------------- */

#define PB_REPEATED_OK              0
/** 入参非法（空指针、elem_size 为 0、count 超过 max_count） */
#define PB_REPEATED_ERR_PARAM       (-1)
/** 解码时元素条数超过 max_count */
#define PB_REPEATED_ERR_FULL        (-2)
/** 编码回调失败 */
#define PB_REPEATED_ERR_ENCODE      (-3)
/** 解码回调失败（数据格式非法） */
#define PB_REPEATED_ERR_DECODE      (-4)

/**
 * @brief repeated 字段的数组描述。绑到 pb_callback_t 的 arg 上。
 */
typedef struct {
    /** 元素数组首地址（连续存放） */
    void     *array;
    /** 单个元素的 sizeof */
    uint16_t  elem_size;
    /** 数组最多能放多少条（解码时的上限） */
    uint16_t  max_count;
    /** 实际条数：编码前由调用方设置，解码后由回调填回 */
    uint16_t  count;
    /** 回调失败时记录原因，成功为 PB_REPEATED_OK */
    int32_t   err;
} pb_repeated_t;

/**
 * @brief 初始化上下文（count 清 0，err 清 OK）
 */
void pb_repeated_init(pb_repeated_t *ctx, void *array,
                      uint16_t elem_size, uint16_t max_count);

/**
 * @brief 编码回调：把 ctx->array 里的 ctx->count 条逐个写出去
 *
 * 绑定方式：msg.items.funcs.encode = pb_repeated_encode_cb;
 *          msg.items.arg = &ctx;
 */
bool pb_repeated_encode_cb(pb_ostream_t *stream, const pb_field_t *field,
                           void * const *arg);

/**
 * @brief 解码回调：把流里的元素依次解到 ctx->array，最多 ctx->max_count 条
 *
 * 绑定方式：msg.items.funcs.decode = pb_repeated_decode_cb;
 *          msg.items.arg = &ctx;
 *
 * 结束后读 ctx->count 得到实际条数；失败时 ctx->err 指出原因。
 */
bool pb_repeated_decode_cb(pb_istream_t *stream, const pb_field_t *field,
                           void **arg);

/**
 * @brief 错误码转字符串
 */
const char *pb_repeated_strerror(int32_t code);

/* -------------------------------------------------------------------------
 * 绑定宏
 * ------------------------------------------------------------------------- */

/** 绑定编码回调到 repeated 字段 */
#define PB_REPEATED_BIND_ENCODE(msg, field, ctx)            \
    do {                                                    \
        (msg).field.funcs.encode = pb_repeated_encode_cb;   \
        (msg).field.arg = (ctx);                            \
    } while (0)

/** 绑定解码回调到 repeated 字段 */
#define PB_REPEATED_BIND_DECODE(msg, field, ctx)            \
    do {                                                    \
        (msg).field.funcs.decode = pb_repeated_decode_cb;   \
        (msg).field.arg = (ctx);                            \
    } while (0)

/* -------------------------------------------------------------------------
 * 静态 repeated 字段的便捷宏
 *
 * 如果 proto 里给了 max_count，生成的是 `pb_size_t field_count; T field[N];`，
 * 不需要回调，但手工填 count 和逐个赋值也麻烦，这两个宏能省点事。
 * ------------------------------------------------------------------------- */

/** 一次性把 n 个元素拷进静态 repeated 字段 */
#define PB_REPEATED_SET(msg, field, src, n)                              \
    do {                                                                 \
        (msg).field##_count = (pb_size_t)(n);                            \
        if ((n) > 0u) {                                                  \
            memcpy((msg).field, (src),                                   \
                   sizeof((msg).field[0]) * (size_t)(n));                \
        }                                                                \
    } while (0)

/** 追加一条，成功返回 true；数组满返回 false（不会越界写） */
#define PB_REPEATED_PUSH(msg, field, val)                                \
    (((msg).field##_count <                                              \
      (pb_size_t)(sizeof((msg).field) / sizeof((msg).field[0])))         \
         ? (((msg).field[(msg).field##_count++] = (val)), true)          \
         : false)

/** 静态 repeated 字段的容量 */
#define PB_REPEATED_CAP(msg, field)                                      \
    ((uint16_t)(sizeof((msg).field) / sizeof((msg).field[0])))

/** 静态 repeated 字段当前条数 */
#define PB_REPEATED_COUNT(msg, field)   ((msg).field##_count)

#ifdef __cplusplus
}
#endif

#endif /* PB_REPEATED_H */
