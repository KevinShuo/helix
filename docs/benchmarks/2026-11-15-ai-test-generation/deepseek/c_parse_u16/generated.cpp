#include <gtest/gtest.h>
#include "subject.h"

#include <cstdint>
#include <limits>

// 正常用例：解析普通十进制数字
TEST(HelixAi0BDFF2059B49C097, ParsesSimpleValue) {
    unsigned short out = 0;
    ASSERT_EQ(parse_u16("123", &out), 1);
    EXPECT_EQ(out, 123);
}

// 边界：解析 0
TEST(HelixAi0BDFF2059B49C097, ParsesZero) {
    unsigned short out = 999;
    ASSERT_EQ(parse_u16("0", &out), 1);
    EXPECT_EQ(out, 0);
}

// 边界：解析最大值 65535
TEST(HelixAi0BDFF2059B49C097, ParsesMaxU16) {
    unsigned short out = 0;
    ASSERT_EQ(parse_u16("65535", &out), 1);
    EXPECT_EQ(out, 65535);
}

// 边界：解析 65534
TEST(HelixAi0BDFF2059B49C097, ParsesMaxMinusOne) {
    unsigned short out = 0;
    ASSERT_EQ(parse_u16("65534", &out), 1);
    EXPECT_EQ(out, 65534);
}

// 前导零应被接受
TEST(HelixAi0BDFF2059B49C097, AcceptsLeadingZeros) {
    unsigned short out = 0;
    ASSERT_EQ(parse_u16("000123", &out), 1);
    EXPECT_EQ(out, 123);
}

// 前导零且值为 0
TEST(HelixAi0BDFF2059B49C097, AcceptsAllZeros) {
    unsigned short out = 7;
    ASSERT_EQ(parse_u16("0000", &out), 1);
    EXPECT_EQ(out, 0);
}

// 溢出：65536 应被拒绝，输出保持不变
TEST(HelixAi0BDFF2059B49C097, RejectsOverflow65536) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16("65536", &out), 0);
    EXPECT_EQ(out, 4242);
}

// 溢出：远超范围应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsLargeOverflow) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16("99999999999999999999", &out), 0);
    EXPECT_EQ(out, 4242);
}

// 空字符串应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsEmptyString) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16("", &out), 0);
    EXPECT_EQ(out, 4242);
}

// NULL 文本应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsNullText) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16(nullptr, &out), 0);
    EXPECT_EQ(out, 4242);
}

// NULL 输出指针应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsNullOut) {
    EXPECT_EQ(parse_u16("123", nullptr), 0);
}

// 负号应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsNegativeSign) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16("-1", &out), 0);
    EXPECT_EQ(out, 4242);
}

// 正号应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsPlusSign) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16("+1", &out), 0);
    EXPECT_EQ(out, 4242);
}

// 前导空白应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsLeadingWhitespace) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16(" 123", &out), 0);
    EXPECT_EQ(out, 4242);
}

// 尾随空白应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsTrailingWhitespace) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16("123 ", &out), 0);
    EXPECT_EQ(out, 4242);
}

// 中间非数字字符应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsEmbeddedNonDigit) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16("12a3", &out), 0);
    EXPECT_EQ(out, 4242);
}

// 字母应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsLetters) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16("abc", &out), 0);
    EXPECT_EQ(out, 4242);
}

// 小数点应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsDecimalPoint) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16("1.5", &out), 0);
    EXPECT_EQ(out, 4242);
}

// 十六进制前缀应被拒绝
TEST(HelixAi0BDFF2059B49C097, RejectsHexPrefix) {
    unsigned short out = 4242;
    EXPECT_EQ(parse_u16("0x10", &out), 0);
    EXPECT_EQ(out, 4242);
}
