/**
 * @file    test_pb_repeated.c
 * @brief   pb_repeated 的往返测试
 *
 * 编译（MinGW / gcc）：
 *   gcc -std=c99 -Wall -Wextra \
 *       -I<nanopb目录> -I<test_repeated.pb.h所在目录> \
 *       test_pb_repeated.c pb_repeated.c test_repeated.pb.c \
 *       <nanopb>/pb_common.c <nanopb>/pb_encode.c <nanopb>/pb_decode.c \
 *       -o test_pb_repeated.exe
 */

#include <stdio.h>
#include <string.h>

#include <pb.h>
#include <pb_encode.h>
#include <pb_decode.h>

#include "pb_repeated.h"
#include "test_repeated.pb.h"

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

static void fill_item(Item *it, uint32_t id, const char *name)
{
    memset(it, 0, sizeof(*it));
    it->id = id;
    if (name != NULL) {
        strncpy(it->name, name, sizeof(it->name) - 1);
        it->name[sizeof(it->name) - 1] = '\0';
    }
}

static int same_item(const Item *a, const Item *b)
{
    if (a->id != b->id) return 0;
    return strcmp(a->name, b->name) == 0;
}

/* ------------------------------------------------------------------ */

static void test_roundtrip(void)
{
    enum { N = 3 };
    uint8_t buf[256];
    uint32_t len = 0;
    pb_repeated_t ctx;
    Container msg = Container_init_zero;
    Container out = Container_init_zero;
    Item src[N], dst[N];
    pb_ostream_t os;
    pb_istream_t is;
    int i;

    CASE("repeated message 编解码往返");

    fill_item(&src[0], 1u, "alpha");
    fill_item(&src[1], 2u, "beta");
    fill_item(&src[2], 3u, "gamma");

    /* 编码 */
    pb_repeated_init(&ctx, src, sizeof(Item), N);
    ctx.count = N;
    PB_REPEATED_BIND_ENCODE(msg, items, &ctx);

    os = pb_ostream_from_buffer(buf, sizeof(buf));
    CHECK(pb_encode(&os, Container_fields, &msg), "encode 失败: %s", PB_GET_ERROR(&os));
    CHECK(ctx.err == PB_REPEATED_OK, "编码回调记录错误: %s", pb_repeated_strerror(ctx.err));
    len = (uint32_t)os.bytes_written;
    printf("  %d 条共编码 %u 字节\n", N, len);

    /* 解码 */
    memset(dst, 0, sizeof(dst));
    pb_repeated_init(&ctx, dst, sizeof(Item), N);
    PB_REPEATED_BIND_DECODE(out, items, &ctx);

    is = pb_istream_from_buffer(buf, len);
    CHECK(pb_decode(&is, Container_fields, &out), "decode 失败: %s", PB_GET_ERROR(&is));
    CHECK(ctx.err == PB_REPEATED_OK, "解码回调记录错误: %s", pb_repeated_strerror(ctx.err));
    CHECK(ctx.count == N, "解出 %u 条，期望 %d 条", ctx.count, N);

    for (i = 0; i < N && i < (int)ctx.count; i++) {
        CHECK(same_item(&src[i], &dst[i]),
              "第 %d 条不一致 (id=%u name=%s)", i, dst[i].id, dst[i].name);
    }
}

static void test_empty(void)
{
    uint8_t buf[64];
    uint32_t len = 0;
    pb_repeated_t ctx;
    Container msg = Container_init_zero;
    Container out = Container_init_zero;
    Item one;
    pb_ostream_t os;
    pb_istream_t is;

    CASE("空 repeated（count = 0）");

    fill_item(&one, 9u, "x");

    pb_repeated_init(&ctx, &one, sizeof(Item), 4);
    ctx.count = 0u;
    PB_REPEATED_BIND_ENCODE(msg, items, &ctx);

    os = pb_ostream_from_buffer(buf, sizeof(buf));
    CHECK(pb_encode(&os, Container_fields, &msg), "空数组编码应成功");
    len = (uint32_t)os.bytes_written;
    CHECK(len == 0u, "空数组编码长度应为 0，实际 %u", len);

    pb_repeated_init(&ctx, &one, sizeof(Item), 4);
    PB_REPEATED_BIND_DECODE(out, items, &ctx);
    is = pb_istream_from_buffer(buf, len);
    CHECK(pb_decode(&is, Container_fields, &out), "空数据解码应成功");
    CHECK(ctx.count == 0u, "空数据应解出 0 条，实际 %u", ctx.count);
}

static void test_full(void)
{
    enum { N = 3, CAP = 2 };
    uint8_t buf[256];
    uint32_t len = 0;
    pb_repeated_t ctx;
    Container msg = Container_init_zero;
    Container out = Container_init_zero;
    Item src[N], dst[CAP];
    pb_ostream_t os;
    pb_istream_t is;
    int i;

    CASE("目标数组容量不足");

    for (i = 0; i < N; i++) {
        fill_item(&src[i], (uint32_t)(i + 1), "x");
    }

    pb_repeated_init(&ctx, src, sizeof(Item), N);
    ctx.count = N;
    PB_REPEATED_BIND_ENCODE(msg, items, &ctx);
    os = pb_ostream_from_buffer(buf, sizeof(buf));
    CHECK(pb_encode(&os, Container_fields, &msg), "encode 失败");
    len = (uint32_t)os.bytes_written;

    /* 只能放 2 条，数据里有 3 条 */
    pb_repeated_init(&ctx, dst, sizeof(Item), CAP);
    PB_REPEATED_BIND_DECODE(out, items, &ctx);
    is = pb_istream_from_buffer(buf, len);
    CHECK(!pb_decode(&is, Container_fields, &out), "容量不足时应解码失败");
    CHECK(ctx.err == PB_REPEATED_ERR_FULL, "错误码应为 FULL，实际 %s",
          pb_repeated_strerror(ctx.err));
    CHECK(ctx.count == CAP, "应已填入 %d 条，实际 %u", CAP, ctx.count);
    printf("  错误: %s（已填 %u 条）\n", pb_repeated_strerror(ctx.err), ctx.count);
}

static void test_format_matches_manual(void)
{
    enum { N = 2 };
    uint8_t buf[256], buf2[256];
    uint32_t len = 0, len2 = 0;
    pb_repeated_t ctx;
    Container msg = Container_init_zero;
    Item src[N];
    pb_ostream_t os, os2;
    int i;

    CASE("与手工逐个编码的字节一致");

    fill_item(&src[0], 100u, "one");
    fill_item(&src[1], 200u, "two");

    /* 本模块的输出 */
    pb_repeated_init(&ctx, src, sizeof(Item), N);
    ctx.count = N;
    PB_REPEATED_BIND_ENCODE(msg, items, &ctx);
    os = pb_ostream_from_buffer(buf, sizeof(buf));
    CHECK(pb_encode(&os, Container_fields, &msg), "encode 失败");
    len = (uint32_t)os.bytes_written;

    /* 手工按 repeated 的 wire format 写一遍 */
    os2 = pb_ostream_from_buffer(buf2, sizeof(buf2));
    for (i = 0; i < N; i++) {
        CHECK(pb_encode_tag_for_field(&os2, &Container_fields[0]), "写 tag 失败");
        CHECK(pb_encode_submessage(&os2, Item_fields, &src[i]), "写 submessage 失败");
    }
    len2 = (uint32_t)os2.bytes_written;

    CHECK(len == len2, "长度不一致: %u vs %u", len, len2);
    CHECK(len == len2 && memcmp(buf, buf2, len) == 0, "字节内容不一致");
    printf("  两种写法都是 %u 字节，内容相同\n", len);
}

static void test_param_errors(void)
{
    uint8_t buf[64];
    pb_repeated_t ctx;
    Container msg = Container_init_zero;
    Item one;
    pb_ostream_t os;

    CASE("参数校验");

    fill_item(&one, 1u, "x");

    /* array 为 NULL */
    pb_repeated_init(&ctx, NULL, sizeof(Item), 1);
    ctx.count = 1u;
    PB_REPEATED_BIND_ENCODE(msg, items, &ctx);
    os = pb_ostream_from_buffer(buf, sizeof(buf));
    CHECK(!pb_encode(&os, Container_fields, &msg), "array 为 NULL 应编码失败");
    CHECK(ctx.err == PB_REPEATED_ERR_PARAM, "错误码应为 PARAM，实际 %s",
          pb_repeated_strerror(ctx.err));

    /* elem_size 为 0 */
    msg = (Container)Container_init_zero;
    pb_repeated_init(&ctx, &one, 0u, 1);
    ctx.count = 1u;
    PB_REPEATED_BIND_ENCODE(msg, items, &ctx);
    os = pb_ostream_from_buffer(buf, sizeof(buf));
    CHECK(!pb_encode(&os, Container_fields, &msg), "elem_size 为 0 应编码失败");
    CHECK(ctx.err == PB_REPEATED_ERR_PARAM, "错误码应为 PARAM");

    /* count 超过 max_count */
    msg = (Container)Container_init_zero;
    pb_repeated_init(&ctx, &one, sizeof(Item), 1);
    ctx.count = 5u;
    PB_REPEATED_BIND_ENCODE(msg, items, &ctx);
    os = pb_ostream_from_buffer(buf, sizeof(buf));
    CHECK(!pb_encode(&os, Container_fields, &msg), "count > max_count 应编码失败");
    CHECK(ctx.err == PB_REPEATED_ERR_PARAM, "错误码应为 PARAM");
}

/* ------------------------------------------------------------------ */

int main(void)
{
    printf("pb_repeated 测试（nanopb pb.h v%d）\n", PB_PROTO_HEADER_VERSION);

    test_roundtrip();
    test_empty();
    test_full();
    test_format_matches_manual();
    test_param_errors();

    printf("\n================================\n");
    if (g_fail == 0) {
        printf("全部 %d 项通过\n", g_case);
        return 0;
    }
    printf("%d 项中有失败\n", g_case);
    return 1;
}
