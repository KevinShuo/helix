#include <gtest/gtest.h>
#include <cstring>
#include <string>

extern "C" {
#include "subject.h"
}

// 正常情况：容量足够，完整复制并返回源长度
TEST(HelixAiCC55F06B8AD1074F, CopiesFullStringWhenCapacitySufficient) {
    char dst[16];
    std::memset(dst, 'X', sizeof(dst));
    size_t n = copy_prefix(dst, sizeof(dst), "hello");
    EXPECT_EQ(n, 5u);
    EXPECT_STREQ(dst, "hello");
}

// 正常情况：容量恰好等于长度+1，完整复制
TEST(HelixAiCC55F06B8AD1074F, CopiesFullStringWhenCapacityExactlyFits) {
    char dst[6];
    std::memset(dst, 'X', sizeof(dst));
    size_t n = copy_prefix(dst, sizeof(dst), "hello");
    EXPECT_EQ(n, 5u);
    EXPECT_STREQ(dst, "hello");
}

// 边界：容量不足，截断为 capacity-1 字节并 NUL 结尾，返回完整长度
TEST(HelixAiCC55F06B8AD1074F, TruncatesWhenCapacityTooSmall) {
    char dst[4];
    std::memset(dst, 'X', sizeof(dst));
    size_t n = copy_prefix(dst, sizeof(dst), "hello");
    EXPECT_EQ(n, 5u);
    EXPECT_STREQ(dst, "hel");
    EXPECT_EQ(dst[3], '\0');
}

// 边界：容量为 1，只写入 NUL，返回完整长度
TEST(HelixAiCC55F06B8AD1074F, CapacityOneWritesOnlyNul) {
    char dst[1];
    dst[0] = 'X';
    size_t n = copy_prefix(dst, 1, "hello");
    EXPECT_EQ(n, 5u);
    EXPECT_EQ(dst[0], '\0');
}

// 边界：容量为 0，不写入任何内容，返回完整长度
TEST(HelixAiCC55F06B8AD1074F, CapacityZeroWritesNothing) {
    char dst[4];
    std::memset(dst, 'X', sizeof(dst));
    size_t n = copy_prefix(dst, 0, "hello");
    EXPECT_EQ(n, 5u);
    EXPECT_EQ(dst[0], 'X');
    EXPECT_EQ(dst[1], 'X');
    EXPECT_EQ(dst[2], 'X');
    EXPECT_EQ(dst[3], 'X');
}

// 边界：空源字符串，返回 0 并写入 NUL
TEST(HelixAiCC55F06B8AD1074F, EmptySourceReturnsZeroAndNulTerminates) {
    char dst[4];
    std::memset(dst, 'X', sizeof(dst));
    size_t n = copy_prefix(dst, sizeof(dst), "");
    EXPECT_EQ(n, 0u);
    EXPECT_EQ(dst[0], '\0');
}

// 边界：NULL 源视为空字符串，返回 0 并写入 NUL
TEST(HelixAiCC55F06B8AD1074F, NullSourceTreatedAsEmpty) {
    char dst[4];
    std::memset(dst, 'X', sizeof(dst));
    size_t n = copy_prefix(dst, sizeof(dst), nullptr);
    EXPECT_EQ(n, 0u);
    EXPECT_EQ(dst[0], '\0');
}

// 边界：NULL 目标且容量为 0，不写入，返回完整长度
TEST(HelixAiCC55F06B8AD1074F, NullDestinationReturnsRequiredLength) {
    size_t n = copy_prefix(nullptr, 0, "hello");
    EXPECT_EQ(n, 5u);
}

// 边界：NULL 目标但容量非 0，不写入，返回完整长度
TEST(HelixAiCC55F06B8AD1074F, NullDestinationWithCapacityReturnsRequiredLength) {
    size_t n = copy_prefix(nullptr, 8, "hello");
    EXPECT_EQ(n, 5u);
}

// 边界：NULL 源与 NULL 目标，返回 0
TEST(HelixAiCC55F06B8AD1074F, NullSourceAndNullDestinationReturnsZero) {
    size_t n = copy_prefix(nullptr, 0, nullptr);
    EXPECT_EQ(n, 0u);
}

// 正常情况：长字符串截断，返回完整长度
TEST(HelixAiCC55F06B8AD1074F, LongStringTruncatedReturnsFullLength) {
    std::string src(100, 'a');
    char dst[10];
    std::memset(dst, 'X', sizeof(dst));
    size_t n = copy_prefix(dst, sizeof(dst), src.c_str());
    EXPECT_EQ(n, 100u);
    EXPECT_EQ(std::strlen(dst), 9u);
    EXPECT_EQ(std::string(dst), std::string(9, 'a'));
}
