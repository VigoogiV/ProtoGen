/**
 * @file    pb_repeated.c
 * @brief   nanopb repeated 字段的通用编解码回调（实现）
 */

#include "pb_repeated.h"

#include <string.h>

/* -------------------------------------------------------------------------
 * 辅助
 * ------------------------------------------------------------------------- */

void pb_repeated_init(pb_repeated_t *ctx, void *array,
                      uint16_t elem_size, uint16_t max_count)
{
    if (ctx == NULL) {
        return;
    }
    ctx->array = array;
    ctx->elem_size = elem_size;
    ctx->max_count = max_count;
    ctx->count = 0u;
    ctx->err = PB_REPEATED_OK;
}

const char *pb_repeated_strerror(int32_t code)
{
    switch (code) {
    case PB_REPEATED_OK:            return "ok";
    case PB_REPEATED_ERR_PARAM:     return "invalid parameter";
    case PB_REPEATED_ERR_FULL:      return "array full";
    case PB_REPEATED_ERR_ENCODE:    return "encode failed";
    case PB_REPEATED_ERR_DECODE:    return "decode failed";
    default:                        return "unknown error";
    }
}

/* -------------------------------------------------------------------------
 * 回调
 * ------------------------------------------------------------------------- */

bool pb_repeated_encode_cb(pb_ostream_t *stream, const pb_field_t *field,
                           void * const *arg)
{
    pb_repeated_t *ctx;
    const pb_field_t *sub_fields;
    uint16_t i;

    if (stream == NULL || field == NULL || arg == NULL || *arg == NULL) {
        return false;
    }

    ctx = (pb_repeated_t *)(*arg);
    if (ctx->array == NULL || ctx->elem_size == 0u) {
        ctx->err = PB_REPEATED_ERR_PARAM;
        return false;
    }
    if (ctx->count > ctx->max_count) {
        ctx->err = PB_REPEATED_ERR_PARAM;
        return false;
    }

    /*
     * repeated message 字段的 ptr 指向子消息的 pb_field_t 表。
     * 标量 repeated 字段的 ptr 为 NULL，本模块不处理那种情况。
     */
    sub_fields = (const pb_field_t *)field->ptr;
    if (sub_fields == NULL) {
        ctx->err = PB_REPEATED_ERR_PARAM;
        return false;
    }

    for (i = 0u; i < ctx->count; i++) {
        const uint8_t *elem =
            (const uint8_t *)ctx->array + (size_t)i * (size_t)ctx->elem_size;

        if (!pb_encode_tag_for_field(stream, field)) {
            ctx->err = PB_REPEATED_ERR_ENCODE;
            return false;
        }
        if (!pb_encode_submessage(stream, sub_fields, elem)) {
            ctx->err = PB_REPEATED_ERR_ENCODE;
            return false;
        }
    }

    ctx->err = PB_REPEATED_OK;
    return true;
}

bool pb_repeated_decode_cb(pb_istream_t *stream, const pb_field_t *field,
                           void **arg)
{
    pb_repeated_t *ctx;
    const pb_field_t *sub_fields;

    if (stream == NULL || field == NULL || arg == NULL || *arg == NULL) {
        return false;
    }

    ctx = (pb_repeated_t *)(*arg);
    if (ctx->array == NULL || ctx->elem_size == 0u) {
        ctx->err = PB_REPEATED_ERR_PARAM;
        return false;
    }

    sub_fields = (const pb_field_t *)field->ptr;
    if (sub_fields == NULL) {
        ctx->err = PB_REPEATED_ERR_PARAM;
        return false;
    }

    /*
     * unpacked 编码下 nanopb 每条元素调用一次回调，流里只有一个元素；
     * 这里用循环兼容 packed 形式（子消息不会被 packed，但循环无副作用）。
     */
    while (stream->bytes_left > 0u) {
        uint8_t *elem;

        if (ctx->count >= ctx->max_count) {
            ctx->err = PB_REPEATED_ERR_FULL;
            return false;
        }

        elem = (uint8_t *)ctx->array + (size_t)ctx->count * (size_t)ctx->elem_size;

        /* 等价于 *_init_zero：nanopb 不碰数据里没出现的字段，必须先清零 */
        memset(elem, 0, (size_t)ctx->elem_size);

        if (!pb_decode(stream, sub_fields, elem)) {
            ctx->err = PB_REPEATED_ERR_DECODE;
            return false;
        }
        ctx->count++;
    }

    ctx->err = PB_REPEATED_OK;
    return true;
}
