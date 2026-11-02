/**
 * @file    pb_array.h
 * @brief   nanopb 消息与消息数组的通用编解码封装
 *
 * 解决的问题
 * ----------
 * nanopb 原生的 pb_encode / pb_decode 每次都要手工建 stream、判断返回值、
 * 从 stream 里取字节数，业务代码里重复且容易漏判。更麻烦的是「数组」：
 * protobuf 编码后的长度是可变的，把多条消息连续塞进一个缓冲区后，
 * 接收方无法判断第一条在哪里结束。
 *
 * 本模块提供：
 *   1. 单条消息的 encode / decode 封装（统一错误码，省掉样板代码）
 *   2. 消息数组的 encode / decode —— 每条消息前加 varint 长度前缀，
 *      接收方按前缀逐条切分，不需要提前知道每条多长
 *
 * 线格式（与 protobuf 的 varint 一致）
 * ------------------------------------
 *     [len1:varint][payload1][len2:varint][payload2]...
 *
 *   单条消息通常几十到几百字节，长度前缀只占 1~2 字节。
 *
 * 用法
 * ----
 *     // 数组编码
 *     Msg_t arr[10];
 *     uint8_t buf[1024];
 *     uint32_t len = 0;
 *     pb_array_err_t err;
 *     if (pb_array_encode(buf, sizeof(buf), arr, 10, sizeof(arr[0]),
 *                         Msg_fields, &len, &err) != PB_ARRAY_OK) {
 *         printf("失败: 第 %u 条, %s\n", err.index, err.message);
 *     }
 *
 *     // 数组解码
 *     Msg_t out[10];
 *     uint16_t count = 0;
 *     if (pb_array_decode(buf, len, out, 10, sizeof(out[0]),
 *                         Msg_fields, &count, &err) != PB_ARRAY_OK) { ... }
 *
 * 或者用宏（只能用于真正的数组，不能用于指针）：
 *     PB_ARRAY_ENCODE(buf, sizeof(buf), arr, Msg_fields, &len);
 *
 * 注意
 * ----
 *   - elem_size 必须传 sizeof(元素类型)，传小了会导致元素错位
 *   - 解码时会先把目标元素 memset 清零，等价于 *_init_zero
 *   - 本模块不分配内存，缓冲区全部由调用方提供
 *   - 依赖 nanopb 0.3.x 的 pb.h / pb_encode.h / pb_decode.h
 */

#ifndef PB_ARRAY_H
#define PB_ARRAY_H

#include <stdint.h>
#include <stddef.h>

#include <pb.h>
#include <pb_encode.h>
#include <pb_decode.h>

#ifdef __cplusplus
extern "C" {
#endif

/* -------------------------------------------------------------------------
 * 错误码
 * ------------------------------------------------------------------------- */

/** 成功 */
#define PB_ARRAY_OK                 0
/** 入参非法（空指针、elem_size 为 0 等） */
#define PB_ARRAY_ERR_PARAM          (-1)
/** 目标缓冲区放不下 */
#define PB_ARRAY_ERR_BUF_SMALL      (-2)
/** pb_encode 失败（字段类型不支持、缓冲区不足等） */
#define PB_ARRAY_ERR_ENCODE         (-3)
/** pb_decode 失败（数据格式非法） */
#define PB_ARRAY_ERR_DECODE         (-4)
/** 输入数据被截断（长度前缀或 payload 不完整） */
#define PB_ARRAY_ERR_TRUNCATED      (-5)
/** 元素个数超过调用方给的容量 */
#define PB_ARRAY_ERR_TOO_MANY       (-6)

/**
 * @brief 错误详情。允许传 NULL，只在需要定位问题时传。
 */
typedef struct {
    /** 错误码，同函数返回值 */
    int32_t code;
    /** 出错的元素下标；非数组操作为 0 */
    uint16_t index;
    /** nanopb 给出的错误描述；无则为空串 */
    char message[64];
} pb_array_err_t;

/**
 * @brief 把错误码转成可读字符串
 */
const char *pb_array_strerror(int32_t code);

/* -------------------------------------------------------------------------
 * 单条消息
 * ------------------------------------------------------------------------- */

/**
 * @brief 把一条消息编码到缓冲区
 *
 * @param dst       目标缓冲区
 * @param dst_cap   目标缓冲区容量（字节）
 * @param msg       源结构体指针
 * @param fields    该消息的 pb_field_t 表，如 Msg_fields
 * @param out_len   输出：实际编码长度（字节）
 * @param err       可选，错误详情，可传 NULL
 *
 * @return PB_ARRAY_OK 或负的错误码
 */
int32_t pb_msg_encode(uint8_t *dst, uint32_t dst_cap,
                      const void *msg, const pb_field_t fields[],
                      uint32_t *out_len, pb_array_err_t *err);

/**
 * @brief 从缓冲区解码一条消息
 *
 * @param src       源缓冲区
 * @param src_len   源数据长度（字节），必须是这条消息的精确长度
 * @param msg       目标结构体指针（内部会先清零）
 * @param fields    该消息的 pb_field_t 表
 * @param err       可选，错误详情
 *
 * @return PB_ARRAY_OK 或负的错误码
 */
int32_t pb_msg_decode(const uint8_t *src, uint32_t src_len,
                      void *msg, const pb_field_t fields[],
                      pb_array_err_t *err);

/* -------------------------------------------------------------------------
 * 消息数组
 * ------------------------------------------------------------------------- */

/**
 * @brief 把一段结构体数组编码成「长度前缀 + payload」的连续流
 *
 * @param dst       目标缓冲区
 * @param dst_cap   目标缓冲区容量（字节）
 * @param arr       数组首地址（元素在内存中连续存放）
 * @param count     元素个数，可为 0（此时 out_len 输出 0）
 * @param elem_size 单个元素的 sizeof，必须等于结构体实际大小
 * @param fields    pb_field_t 表
 * @param out_len   输出：实际写入的总字节数
 * @param err       可选，错误详情（出错时 index 指出第几条）
 *
 * @return PB_ARRAY_OK 或负的错误码
 */
int32_t pb_array_encode(uint8_t *dst, uint32_t dst_cap,
                        const void *arr, uint16_t count, uint16_t elem_size,
                        const pb_field_t fields[],
                        uint32_t *out_len, pb_array_err_t *err);

/**
 * @brief 从连续流里解出结构体数组
 *
 * 会一直解到数据用完为止；如果解出的条数超过 max_count 返回 PB_ARRAY_ERR_TOO_MANY。
 *
 * @param src       源缓冲区
 * @param src_len   源数据长度
 * @param arr       目标数组首地址
 * @param max_count 目标数组最多能放多少条
 * @param elem_size 单个元素的 sizeof
 * @param fields    pb_field_t 表
 * @param out_count 输出：实际解出的条数
 * @param err       可选，错误详情
 *
 * @return PB_ARRAY_OK 或负的错误码
 */
int32_t pb_array_decode(const uint8_t *src, uint32_t src_len,
                        void *arr, uint16_t max_count, uint16_t elem_size,
                        const pb_field_t fields[],
                        uint16_t *out_count, pb_array_err_t *err);

/* -------------------------------------------------------------------------
 * 便捷宏（只能用于真正的数组，不能用于指针）
 * ------------------------------------------------------------------------- */

/** 数组元素个数 */
#define PB_ARRAY_COUNT(a)       ((uint16_t)(sizeof(a) / sizeof((a)[0])))
/** 数组单个元素大小 */
#define PB_ARRAY_ELEM_SIZE(a)   ((uint16_t)sizeof((a)[0]))

/**
 * @brief 编码整个数组，自动推导 count 与 elem_size
 *
 * 注意：a 必须是数组类型。如果 a 是函数参数退化成指针，sizeof 会得到指针大小，
 * 导致只编码 1 条且长度错误 —— 这种场景请直接调用 pb_array_encode。
 */
#define PB_ARRAY_ENCODE(dst, dst_cap, a, fields, out_len) \
    pb_array_encode((dst), (dst_cap), (a), PB_ARRAY_COUNT(a), \
                    PB_ARRAY_ELEM_SIZE(a), (fields), (out_len), NULL)

/**
 * @brief 解码到数组，自动推导 max_count 与 elem_size
 */
#define PB_ARRAY_DECODE(src, src_len, a, fields, out_count) \
    pb_array_decode((src), (src_len), (a), PB_ARRAY_COUNT(a), \
                    PB_ARRAY_ELEM_SIZE(a), (fields), (out_count), NULL)

#ifdef __cplusplus
}
#endif

#endif /* PB_ARRAY_H */
