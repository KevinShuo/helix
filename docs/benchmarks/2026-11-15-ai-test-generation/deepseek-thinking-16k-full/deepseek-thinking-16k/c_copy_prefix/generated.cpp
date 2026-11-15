#include <gtest/gtest.h>
#include <cstddef>
#include <cstring>

extern "C" {
#include "subject.h"
}

// 正常情况：容量足够时复制完整字符串并返回源长度
TEST(HelixAiCC55F06B8AD1074F, CopiesWholeStringWhenCapacityIsSufficient) {
    char dst[16];
    std::memset(dst, 'X', sizeof dst);
    size_t required = copy_prefix(dst, sizeof dst, "hello");
    EXPECT_EQ(required, size_t{5});
    EXPECT_STREQ(dst, "hello");
}

// 边界：容量恰好为源长度加一（刚好容纳 NUL）
TEST(HelixAiCC55F06B8AD1074F, ExactFitCapacityCopiesWholeString) {
    char dst[6];
    std::memset(dst, 'X', sizeof dst);
    size_t required = copy_prefix(dst, sizeof dst, "hello");
    EXPECT_EQ(required, size_t{5});
    EXPECT_STREQ(dst, "hello");
}

// 正常截断：容量不足时只复制 capacity-1 字节，返回完整源长度
TEST(HelixAiCC55F06B8AD1074F, TruncatesWhenCapacityIsTooSmall) {
    char dst[4];
    std::memset(dst, 'X', sizeof dst);
    size_t required = copy_prefix(dst, sizeof dst, "hello");
    EXPECT_EQ(required, size_t{5});
    EXPECT_EQ(dst[0], 'h');
    EXPECT_EQ(dst[1], 'e');
    EXPECT_EQ(dst[2], 'l');
    EXPECT_EQ(dst[3], 0);
}

// 边界：容量为 2 时仅复制一个字符并以 NUL 结尾
TEST(HelixAiCC55F06B8AD1074F, CapacityTwoCopiesSingleCharacter) {
    char dst[2] = {'X', 'Y'};
    size_t required = copy_prefix(dst, 2, "hello");
    EXPECT_EQ(required, size_t{5});
    EXPECT_EQ(dst[0], 'h');
    EXPECT_EQ(dst[1], 0);
}

// 边界：容量为 1 时只写入 NUL 终止符
TEST(HelixAiCC55F06B8AD1074F, CapacityOneWritesOnlyTerminator) {
    char dst[1] = {'X'};
    size_t required = copy_prefix(dst, 1, "hello");
    EXPECT_EQ(required, size_t{5});
    EXPECT_EQ(dst[0], 0);
}

// 边界：容量为 0 时不写入任何内容
TEST(HelixAiCC55F06B8AD1074F, CapacityZeroLeavesDestinationUntouched) {
    char dst[8];
    std::memset(dst, 'Z', sizeof dst);
    size_t required = copy_prefix(dst, 0, "hello");
    EXPECT_EQ(required, size_t{5});
    for (size_t i = 0; i < sizeof dst; ++i) {
        EXPECT_EQ(dst[i], 'Z') << "index " << i;
    }
}

// 目标指针为 NULL：不写入，仍返回完整源长度
TEST(HelixAiCC55F06B8AD1074F, NullDestinationReturnsRequiredLength) {
    size_t required = copy_prefix(nullptr, 8, "hello");
    EXPECT_EQ(required, size_t{5});
}

// 源指针为 NULL：按空字符串处理，写入空字符串并返回 0
TEST(HelixAiCC55F06B8AD1074F, NullSourceIsTreatedAsEmptyString) {
    char dst[8];
    std::memset(dst, 'X', sizeof dst);
    size_t required = copy_prefix(dst, sizeof dst, nullptr);
    EXPECT_EQ(required, size_t{0});
    EXPECT_STREQ(dst, "");
}

// 源为 NULL 且容量为 1：只写入 NUL 终止符
TEST(HelixAiCC55F06B8AD1074F, NullSourceWithCapacityOneWritesTerminator) {
    char dst[1] = {'X'};
    size_t required = copy_prefix(dst, 1, nullptr);
    EXPECT_EQ(required, size_t{0});
    EXPECT_EQ(dst[0], 0);
}

// 源和目标均为 NULL：返回 0 且不崩溃
TEST(HelixAiCC55F06B8AD1074F, NullSourceAndNullDestinationReturnsZero) {
    size_t required = copy_prefix(nullptr, 5, nullptr);
    EXPECT_EQ(required, size_t{0});
}

// 源为空字符串：写入空字符串并返回 0
TEST(HelixAiCC55F06B8AD1074F, EmptySourceWritesEmptyString) {
    char dst[4] = {'A', 'B', 'C', 'D'};
    size_t required = copy_prefix(dst, sizeof dst, "");
    EXPECT_EQ(required, size_t{0});
    EXPECT_STREQ(dst, "");
}

// 边界：源长度恰好比容量少一（刚好放不下完整内容）
TEST(HelixAiCC55F06B8AD1074F, CapacityExactlySourceLengthTruncatesLastCharacter) {
    char dst[5];
    std::memset(dst, 'X', sizeof dst);
    size_t required = copy_prefix(dst, sizeof dst, "hello");
    EXPECT_EQ(required, size_t{5});
    EXPECT_EQ(dst[0], 'h');
    EXPECT_EQ(dst[1], 'e');
    EXPECT_EQ(dst[2], 'l');
    EXPECT_EQ(dst[3], 'l');
    EXPECT_EQ(dst[4], 0);
}
