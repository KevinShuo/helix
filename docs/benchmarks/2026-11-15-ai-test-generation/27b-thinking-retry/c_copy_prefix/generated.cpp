#include "subject.h"

#include <cstring>
#include <gtest/gtest.h>

extern "C" {
size_t copy_prefix(char *dst, size_t capacity, const char *src);
}

// 正常情况：源字符串完整放入目标缓冲区
TEST(HelixAiCC55F06B8AD1074F, 完整复制无截断) {
    char buf[16] = {0};
    size_t ret = copy_prefix(buf, sizeof(buf), "hello");
    EXPECT_EQ(ret, 5u);
    EXPECT_STREQ(buf, "hello");
}

// 正常情况：源字符串远小于容量
TEST(HelixAiCC55F06B8AD1074F, 短字符串大缓冲区) {
    char buf[32] = {0xAA};
    size_t ret = copy_prefix(buf, sizeof(buf), "hi");
    EXPECT_EQ(ret, 2u);
    EXPECT_STREQ(buf, "hi");
}

// 截断：源长度大于 capacity-1，只复制 capacity-1 字节
TEST(HelixAiCC55F06B8AD1074F, 截断返回完整长度) {
    char buf[6] = {0};
    size_t ret = copy_prefix(buf, sizeof(buf), "hello world");
    EXPECT_EQ(ret, 11u);
    EXPECT_STREQ(buf, "hello");
}

// 边界：源长度恰好等于 capacity-1，完整复制
TEST(HelixAiCC55F06B8AD1074F, 源长度等于容量减一) {
    char buf[4] = {0};
    size_t ret = copy_prefix(buf, sizeof(buf), "abc");
    EXPECT_EQ(ret, 3u);
    EXPECT_STREQ(buf, "abc");
}

// 边界：源长度等于 capacity，截断一个字符
TEST(HelixAiCC55F06B8AD1074F, 源长度等于容量截断一个字符) {
    char buf[3] = {0};
    size_t ret = copy_prefix(buf, sizeof(buf), "abc");
    EXPECT_EQ(ret, 3u);
    EXPECT_STREQ(buf, "ab");
}

// 边界：capacity 为 1，只能写入 NUL
TEST(HelixAiCC55F06B8AD1074F, 容量为一仅写NUL) {
    char buf[1] = {'X'};
    size_t ret = copy_prefix(buf, 1, "hello");
    EXPECT_EQ(ret, 5u);
    EXPECT_EQ(buf[0], '\0');
}

// 空源字符串：返回 0，目标写入 NUL
TEST(HelixAiCC55F06B8AD1074F, 空源字符串) {
    char buf[8] = {'X', 'X', 'X', 'X', 'X', 'X', 'X', 'X'};
    size_t ret = copy_prefix(buf, sizeof(buf), "");
    EXPECT_EQ(ret, 0u);
    EXPECT_EQ(buf[0], '\0');
}

// NULL 源：视为空字符串，返回 0
TEST(HelixAiCC55F06B8AD1074F, NULL源视为空串) {
    char buf[8] = {'X', 'X', 'X', 'X', 'X', 'X', 'X', 'X'};
    size_t ret = copy_prefix(buf, sizeof(buf), nullptr);
    EXPECT_EQ(ret, 0u);
    EXPECT_EQ(buf[0], '\0');
}

// NULL 目标：不写入任何内容，仍返回源长度
TEST(HelixAiCC55F06B8AD1074F, NULL目标不写入) {
    size_t ret = copy_prefix(nullptr, 16, "hello");
    EXPECT_EQ(ret, 5u);
}

// capacity 为 0：不写入任何内容，仍返回源长度
TEST(HelixAiCC55F06B8AD1074F, 容量为零不写入) {
    char buf[8] = {'X', 'X', 'X', 'X', 'X', 'X', 'X', 'X'};
    size_t ret = copy_prefix(buf, 0, "hello");
    EXPECT_EQ(ret, 5u);
    EXPECT_EQ(buf[0], 'X');
}

// NULL 源且 NULL 目标：返回 0，不写入
TEST(HelixAiCC55F06B8AD1074F, NULL源且NULL目标) {
    size_t ret = copy_prefix(nullptr, 10, nullptr);
    EXPECT_EQ(ret, 0u);
}

// 截断后缓冲区末尾必须为 NUL（验证无残留）
TEST(HelixAiCC55F06B8AD1074F, 截断后NUL终止) {
    char buf[5] = {'a', 'b', 'c', 'd', 'e'};
    size_t ret = copy_prefix(buf, sizeof(buf), "abcdefgh");
    EXPECT_EQ(ret, 8u);
    EXPECT_STREQ(buf, "abcd");
}
