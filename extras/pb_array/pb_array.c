/**
 * @file    pb_array.c
 * @brief   nanopb 消息与消息数组的通用编解码封装（实现）
 */

#include "pb_array.h"

#include <string.h>

/** 32 位长度前缀最多占 5 字节 */
#define PB_ARRAY_MAX_PREFIX     5u

/* -------------------------------------------------------------------------
 * 内部工具
 * ------------------------------------------------------------------------- */

static void _copy_str(char *dst, size_t cap, const char *src)
{
    size_t i = 0;

    if (cap == 0u) {
        return;
    }
    if (src == NULL) {
        dst[0] = '\0';
        return;
    }
    while ((i + 1u) < cap && src[i] != '\0') {
        dst[i] = src[i];
        i++;
    }
    dst[i] = '\0';
}

/** 统一填写错误详情，err 允许为 NULL */
static void _set_err(pb_array_err_t *err, int32_t code, uint16_t index, const char *msg)
{
    if (err == NULL) {
        return;
    }
    err->code = code;
    err->index = index;
    _copy_str(err->message, sizeof(err->message), msg);
}

/** 32 位无符号数编码成 varint 需要几个字节 */
static uint32_t _varint_size(uint32_t value)
{
    uint32_t n = 1u;

    while (value >= 0x80u) {
        value >>= 7;
        n++;
    }
    return n;
}

/** 写入 varint，返回写完后的位置 */
static uint8_t *_varint_write(uint8_t *p, uint32_t value)
{
    while (value >= 0x80u) {
        *p++ = (uint8_t)((value & 0x7Fu) | 0x80u);
        value >>= 7;
    }
    *p++ = (uint8_t)value;
    return p;
}

/**
 * 读取 varint
 * @return 读完后的位置；数据不完整或长度溢出返回 NULL
 */
static const uint8_t *_varint_read(const uint8_t *p, const uint8_t *end, uint32_t *out)
{
    uint32_t value = 0u;
    uint32_t shift = 0u;

    while (p < end && shift < 32u) {
        uint8_t b = *p++;
        value |= (uint32_t)(b & 0x7Fu) << shift;
        if ((b & 0x80u) == 0u) {
            *out = value;
            return p;
        }
        shift += 7u;
    }
    return NULL;
}

/* -------------------------------------------------------------------------
 * 错误码描述
 * ------------------------------------------------------------------------- */

const char *pb_array_strerror(int32_t code)
{
    switch (code) {
    case PB_ARRAY_OK:               return "ok";
    case PB_ARRAY_ERR_PARAM:        return "invalid parameter";
    case PB_ARRAY_ERR_BUF_SMALL:    return "destination buffer too small";
    case PB_ARRAY_ERR_ENCODE:       return "pb_encode failed";
    case PB_ARRAY_ERR_DECODE:       return "pb_decode failed";
    case PB_ARRAY_ERR_TRUNCATED:    return "input data truncated";
    case PB_ARRAY_ERR_TOO_MANY:     return "too many elements";
    default:                        return "unknown error";
    }
}

/* -------------------------------------------------------------------------
 * 单条消息
 * ------------------------------------------------------------------------- */

int32_t pb_msg_encode(uint8_t *dst, uint32_t dst_cap,
                      const void *msg, const pb_field_t fields[],
                      uint32_t *out_len, pb_array_err_t *err)
{
    pb_ostream_t os;

    if (dst == NULL || msg == NULL || fields == NULL || out_len == NULL) {
        _set_err(err, PB_ARRAY_ERR_PARAM, 0u, "null pointer argument");
        return PB_ARRAY_ERR_PARAM;
    }

    os = pb_ostream_from_buffer(dst, (size_t)dst_cap);
    if (!pb_encode(&os, fields, msg)) {
        _set_err(err, PB_ARRAY_ERR_ENCODE, 0u, PB_GET_ERROR(&os));
        return PB_ARRAY_ERR_ENCODE;
    }

    *out_len = (uint32_t)os.bytes_written;
    _set_err(err, PB_ARRAY_OK, 0u, "");
    return PB_ARRAY_OK;
}

int32_t pb_msg_decode(const uint8_t *src, uint32_t src_len,
                      void *msg, const pb_field_t fields[],
                      pb_array_err_t *err)
{
    pb_istream_t is;

    if (src == NULL || msg == NULL || fields == NULL) {
        _set_err(err, PB_ARRAY_ERR_PARAM, 0u, "null pointer argument");
        return PB_ARRAY_ERR_PARAM;
    }

    /*
     * 这里无法 memset：函数不知道目标结构体多大。
     * nanopb 不会初始化未出现在数据流里的字段，所以调用方必须先自己清零，
     * 标准写法是 `Msg_t msg = Msg_init_zero;`。
     * 数组版本知道 elem_size，会自动清零，见 pb_array_decode。
     */
    is = pb_istream_from_buffer(src, (size_t)src_len);
    if (!pb_decode(&is, fields, msg)) {
        _set_err(err, PB_ARRAY_ERR_DECODE, 0u, PB_GET_ERROR(&is));
        return PB_ARRAY_ERR_DECODE;
    }
    if (is.bytes_left != 0u) {
        _set_err(err, PB_ARRAY_ERR_DECODE, 0u, "trailing bytes after message");
        return PB_ARRAY_ERR_DECODE;
    }

    _set_err(err, PB_ARRAY_OK, 0u, "");
    return PB_ARRAY_OK;
}

/* -------------------------------------------------------------------------
 * 消息数组
 * ------------------------------------------------------------------------- */

int32_t pb_array_encode(uint8_t *dst, uint32_t dst_cap,
                        const void *arr, uint16_t count, uint16_t elem_size,
                        const pb_field_t fields[],
                        uint32_t *out_len, pb_array_err_t *err)
{
    uint8_t *p;
    uint8_t *end;
    uint16_t i;

    if (dst == NULL || arr == NULL || fields == NULL || out_len == NULL) {
        _set_err(err, PB_ARRAY_ERR_PARAM, 0u, "null pointer argument");
        return PB_ARRAY_ERR_PARAM;
    }
    if (count > 0u && elem_size == 0u) {
        _set_err(err, PB_ARRAY_ERR_PARAM, 0u, "elem_size is zero");
        return PB_ARRAY_ERR_PARAM;
    }

    p = dst;
    end = dst + dst_cap;

    for (i = 0u; i < count; i++) {
        const uint8_t *elem = (const uint8_t *)arr + (size_t)i * (size_t)elem_size;
        uint8_t *payload;
        pb_ostream_t os;
        uint32_t payload_len;
        uint32_t prefix_len;

        /* 先给长度前缀留出最大空间，编码完再按实际前缀长度前移压缩 */
        if ((size_t)(end - p) <= (size_t)PB_ARRAY_MAX_PREFIX) {
            _set_err(err, PB_ARRAY_ERR_BUF_SMALL, i, "no room for length prefix");
            return PB_ARRAY_ERR_BUF_SMALL;
        }

        payload = p + PB_ARRAY_MAX_PREFIX;
        os = pb_ostream_from_buffer(payload, (size_t)(end - payload));
        if (!pb_encode(&os, fields, elem)) {
            _set_err(err, PB_ARRAY_ERR_ENCODE, i, PB_GET_ERROR(&os));
            return PB_ARRAY_ERR_ENCODE;
        }

        payload_len = (uint32_t)os.bytes_written;
        prefix_len = _varint_size(payload_len);

        if (prefix_len != PB_ARRAY_MAX_PREFIX) {
            memmove(p + prefix_len, payload, (size_t)payload_len);
        }
        (void)_varint_write(p, payload_len);
        p += prefix_len + payload_len;
    }

    *out_len = (uint32_t)(p - dst);
    _set_err(err, PB_ARRAY_OK, 0u, "");
    return PB_ARRAY_OK;
}

int32_t pb_array_decode(const uint8_t *src, uint32_t src_len,
                        void *arr, uint16_t max_count, uint16_t elem_size,
                        const pb_field_t fields[],
                        uint16_t *out_count, pb_array_err_t *err)
{
    const uint8_t *p;
    const uint8_t *end;
    uint16_t n = 0u;

    if (src == NULL || arr == NULL || fields == NULL || out_count == NULL) {
        _set_err(err, PB_ARRAY_ERR_PARAM, 0u, "null pointer argument");
        return PB_ARRAY_ERR_PARAM;
    }
    if (elem_size == 0u) {
        _set_err(err, PB_ARRAY_ERR_PARAM, 0u, "elem_size is zero");
        return PB_ARRAY_ERR_PARAM;
    }

    p = src;
    end = src + src_len;

    while (p < end) {
        uint32_t payload_len = 0u;
        const uint8_t *payload;
        uint8_t *elem;
        pb_istream_t is;

        if (n >= max_count) {
            _set_err(err, PB_ARRAY_ERR_TOO_MANY, n, "destination array full");
            return PB_ARRAY_ERR_TOO_MANY;
        }

        payload = _varint_read(p, end, &payload_len);
        if (payload == NULL) {
            _set_err(err, PB_ARRAY_ERR_TRUNCATED, n, "bad length prefix");
            return PB_ARRAY_ERR_TRUNCATED;
        }
        if (payload_len > (uint32_t)(end - payload)) {
            _set_err(err, PB_ARRAY_ERR_TRUNCATED, n, "payload shorter than prefix says");
            return PB_ARRAY_ERR_TRUNCATED;
        }

        elem = (uint8_t *)arr + (size_t)n * (size_t)elem_size;

        /* 等价于 *_init_zero：nanopb 不碰数据流里没出现的字段，必须先清零 */
        memset(elem, 0, (size_t)elem_size);

        is = pb_istream_from_buffer(payload, (size_t)payload_len);
        if (!pb_decode(&is, fields, elem)) {
            _set_err(err, PB_ARRAY_ERR_DECODE, n, PB_GET_ERROR(&is));
            return PB_ARRAY_ERR_DECODE;
        }
        if (is.bytes_left != 0u) {
            _set_err(err, PB_ARRAY_ERR_DECODE, n, "trailing bytes in payload");
            return PB_ARRAY_ERR_DECODE;
        }

        p = payload + payload_len;
        n++;
    }

    *out_count = n;
    _set_err(err, PB_ARRAY_OK, 0u, "");
    return PB_ARRAY_OK;
}
