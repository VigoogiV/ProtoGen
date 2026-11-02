/**
 * @file    test_pb_array.c
 * @brief   pb_array 的往返测试
 *
 * 编译（MinGW / gcc）：
 *   gcc -std=c99 -Wall -Wextra \
 *       -I<nanopb目录> -I<生成的.pb.h所在目录> \
 *       test_pb_array.c pb_array.c \
 *       <nanopb>/pb_common.c <nanopb>/pb_encode.c <nanopb>/pb_decode.c \
 *       -o test_pb_array.exe
 *
 * 用 tests/golden/edge 生成的 edge.pb.h 里的 edge_test_Inner 做样本，
 * 它同时含定长标量（uint32）和变长字段（string[8]），能验证长度前缀的变长处理。
 */

#include <stdio.h>
#include <string.h>

#include <pb.h>
#include <pb_encode.h>
#include <pb_decode.h>

#include "pb_array.h"
#include "edge.pb.h"

static int g_fail = 0;
static int g_case = 0;

#define CHECK(cond, ...)                                        \
    do {                                                        \
        if (!(cond)) {                                          \
            printf("  [FAIL] ");                                \
            printf(__VA_ARGS__);                                \
            printf("\n");                                       \
            g_fail++;                                           \
        }                                                       \
    } while (0)

#define CASE(name)                                              \
    do {                                                        \
        g_case++;                                               \
        printf("\n[%d] %s\n", g_case, name);                    \
    } while (0)

/** 填一条样本 */
static void fill(edge_test_Inner *m, uint32_t a, const char *s)
{
    memset(m, 0, sizeof(*m));
    m->a = a;
    if (s != NULL) {
        m->has_s = true;
        strncpy(m->s, s, sizeof(m->s) - 1);
        m->s[sizeof(m->s) - 1] = '\0';
    }
}

/** 比较两条样本是否一致 */
static int same(const edge_test_Inner *x, const edge_test_Inner *y)
{
    if (x->a != y->a) return 0;
    if (x->has_s != y->has_s) return 0;
    if (x->has_s && strcmp(x->s, y->s) != 0) return 0;
    return 1;
}

/* ------------------------------------------------------------------ */

static void test_msg_roundtrip(void)
{
    uint8_t buf[64];
    uint32_t len = 0;
    pb_array_err_t err;
    edge_test_Inner src, dst;

    CASE("单条消息编解码往返");

    fill(&src, 42u, "hello");
    CHECK(pb_msg_encode(buf, sizeof(buf), &src, edge_test_Inner_fields, &len, &err) == PB_ARRAY_OK,
          "encode 失败: %s", err.message);
    CHECK(len > 0u, "编码长度为 0");

    fill(&dst, 0u, NULL);
    CHECK(pb_msg_decode(buf, len, &dst, edge_test_Inner_fields, &err) == PB_ARRAY_OK,
          "decode 失败: %s", err.message);
    CHECK(same(&src, &dst), "往返后内容不一致 (a=%u s=%s)", dst.a, dst.has_s ? dst.s : "(null)");

    printf("  编码 %u 字节, a=%u, s=%s\n", len, dst.a, dst.has_s ? dst.s : "(none)");
}

static void test_array_roundtrip(void)
{
    enum { N = 5 };
    uint8_t buf[256];
    uint32_t len = 0;
    pb_array_err_t err;
    edge_test_Inner src[N], dst[N];
    uint16_t count = 0;
    int i;

    CASE("数组编解码往返（变长元素）");

    /* 字符串长度递增，保证每条编码后的长度不同，能验证前缀是逐条自适应的 */
    fill(&src[0], 1u, "a");
    fill(&src[1], 2u, "bb");
    fill(&src[2], 300u, "ccc");
    fill(&src[3], 40000u, "dddd");
    fill(&src[4], 5u, NULL);

    CHECK(pb_array_encode(buf, sizeof(buf), src, N, sizeof(src[0]),
                          edge_test_Inner_fields, &len, &err) == PB_ARRAY_OK,
          "array encode 失败: 第 %u 条 %s", err.index, err.message);
    printf("  %d 条共编码 %u 字节\n", N, len);

    memset(dst, 0, sizeof(dst));
    CHECK(pb_array_decode(buf, len, dst, N, sizeof(dst[0]),
                          edge_test_Inner_fields, &count, &err) == PB_ARRAY_OK,
          "array decode 失败: 第 %u 条 %s", err.index, err.message);
    CHECK(count == N, "解出 %u 条，期望 %d 条", count, N);

    for (i = 0; i < N && i < (int)count; i++) {
        CHECK(same(&src[i], &dst[i]), "第 %d 条不一致", i);
    }
}

static void test_empty_array(void)
{
    uint8_t buf[16];
    uint32_t len = 12345;
    uint16_t count = 12345;
    pb_array_err_t err;
    edge_test_Inner one;

    CASE("空数组");

    fill(&one, 1u, "x");
    CHECK(pb_array_encode(buf, sizeof(buf), &one, 0, sizeof(one),
                          edge_test_Inner_fields, &len, &err) == PB_ARRAY_OK,
          "空数组编码应成功");
    CHECK(len == 0u, "空数组编码长度应为 0，实际 %u", len);

    CHECK(pb_array_decode(buf, 0u, &one, 1, sizeof(one),
                          edge_test_Inner_fields, &count, &err) == PB_ARRAY_OK,
          "空数据解码应成功");
    CHECK(count == 0u, "空数据应解出 0 条，实际 %u", count);
}

static void test_buf_small(void)
{
    uint8_t buf[8];
    uint32_t len = 0;
    pb_array_err_t err;
    edge_test_Inner one;

    CASE("目标缓冲区不足");

    fill(&one, 1000u, "abcdefg");   /* 编码后明显超过 8 字节 */
    CHECK(pb_array_encode(buf, sizeof(buf), &one, 1, sizeof(one),
                          edge_test_Inner_fields, &len, &err) != PB_ARRAY_OK,
          "缓冲区不足时应失败");
    printf("  错误码 %d: %s\n", err.code, err.message);
    CHECK(err.code == PB_ARRAY_ERR_ENCODE || err.code == PB_ARRAY_ERR_BUF_SMALL,
          "错误码应为 ENCODE 或 BUF_SMALL，实际 %d", err.code);
}

static void test_truncated(void)
{
    uint8_t buf[64];
    uint32_t len = 0;
    uint16_t count = 0;
    pb_array_err_t err;
    edge_test_Inner src, dst;

    CASE("数据被截断");

    fill(&src, 7u, "abc");
    CHECK(pb_array_encode(buf, sizeof(buf), &src, 1, sizeof(src),
                          edge_test_Inner_fields, &len, &err) == PB_ARRAY_OK,
          "encode 失败");
    CHECK(len >= 2u, "编码太短，无法测试截断");

    memset(&dst, 0, sizeof(dst));
    CHECK(pb_array_decode(buf, len - 1u, &dst, 1, sizeof(dst),
                          edge_test_Inner_fields, &count, &err) != PB_ARRAY_OK,
          "截断数据应解码失败");
    printf("  错误码 %d: %s\n", err.code, err.message);
    CHECK(err.code == PB_ARRAY_ERR_TRUNCATED || err.code == PB_ARRAY_ERR_DECODE,
          "错误码应为 TRUNCATED 或 DECODE，实际 %d", err.code);
}

static void test_too_many(void)
{
    uint8_t buf[256];
    uint32_t len = 0;
    uint16_t count = 0;
    pb_array_err_t err;
    edge_test_Inner src[3], dst[2];
    int i;

    CASE("目标数组容量不足");

    for (i = 0; i < 3; i++) {
        fill(&src[i], (uint32_t)(i + 1), "x");
    }
    CHECK(pb_array_encode(buf, sizeof(buf), src, 3, sizeof(src[0]),
                          edge_test_Inner_fields, &len, &err) == PB_ARRAY_OK,
          "encode 失败");

    CHECK(pb_array_decode(buf, len, dst, 2, sizeof(dst[0]),
                          edge_test_Inner_fields, &count, &err) != PB_ARRAY_OK,
          "容量 2 解 3 条应失败");
    CHECK(err.code == PB_ARRAY_ERR_TOO_MANY, "错误码应为 TOO_MANY，实际 %d", err.code);
    printf("  错误码 %d: %s\n", err.code, err.message);
}

static void test_prefix_layout(void)
{
    uint8_t buf[256];
    uint32_t len = 0;
    pb_array_err_t err;
    edge_test_Inner src[3];
    uint32_t off = 0;
    int i;

    CASE("长度前缀布局校验");

    fill(&src[0], 1u, "a");
    fill(&src[1], 2u, "bb");
    fill(&src[2], 3u, "ccc");

    CHECK(pb_array_encode(buf, sizeof(buf), src, 3, sizeof(src[0]),
                          edge_test_Inner_fields, &len, &err) == PB_ARRAY_OK,
          "encode 失败");

    /* 手工按 [varint len][payload] 走一遍，确认能正好走到末尾 */
    for (i = 0; i < 3; i++) {
        uint32_t v = 0;
        uint32_t shift = 0;
        uint32_t plen;

        do {
            uint8_t b = buf[off++];
            v |= (uint32_t)(b & 0x7Fu) << shift;
            shift += 7u;
            if ((b & 0x80u) == 0u) break;
        } while (off < len);

        plen = v;
        CHECK(plen > 0u, "第 %d 条前缀为 0", i);
        CHECK(off + plen <= len, "第 %d 条 payload 越界", i);
        off += plen;
    }

    CHECK(off == len, "手工解析走到 %u，总长 %u，前缀布局不一致", off, len);
    printf("  3 条前缀解析后正好走完 %u 字节\n", len);
}

static void test_param_errors(void)
{
    uint8_t buf[16];
    uint32_t len = 0;
    uint16_t count = 0;
    pb_array_err_t err;
    edge_test_Inner one;

    CASE("参数校验");

    fill(&one, 1u, "x");

    CHECK(pb_array_encode(NULL, sizeof(buf), &one, 1, sizeof(one),
                          edge_test_Inner_fields, &len, &err) == PB_ARRAY_ERR_PARAM,
          "dst 为 NULL 应返回 PARAM");
    CHECK(pb_array_encode(buf, sizeof(buf), &one, 1, 0,
                          edge_test_Inner_fields, &len, &err) == PB_ARRAY_ERR_PARAM,
          "elem_size 为 0 应返回 PARAM");
    CHECK(pb_array_encode(buf, sizeof(buf), &one, 1, sizeof(one),
                          edge_test_Inner_fields, NULL, &err) == PB_ARRAY_ERR_PARAM,
          "out_len 为 NULL 应返回 PARAM");
    CHECK(pb_array_decode(NULL, 10, &one, 1, sizeof(one),
                          edge_test_Inner_fields, &count, &err) == PB_ARRAY_ERR_PARAM,
          "src 为 NULL 应返回 PARAM");
}

/* ------------------------------------------------------------------ */

int main(void)
{
    printf("pb_array 测试（nanopb pb.h v%d）\n", PB_PROTO_HEADER_VERSION);

    test_msg_roundtrip();
    test_array_roundtrip();
    test_empty_array();
    test_buf_small();
    test_truncated();
    test_too_many();
    test_prefix_layout();
    test_param_errors();

    printf("\n================================\n");
    if (g_fail == 0) {
        printf("全部 %d 项通过\n", g_case);
        return 0;
    }
    printf("%d 项中有失败\n", g_case);
    return 1;
}
