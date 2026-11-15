#include <gtest/gtest.h>
#include <cstdint>

extern "C" {
#include "subject.h"
}

// 正常解析：最小值 0
TEST(HelixAi0BDFF2059B49C097, ParseZero) {
    unsigned short out = 0xFFFF;
    int rc = parse_u16("0", &out);
    EXPECT_EQ(rc, 1);
    EXPECT_EQ(out, 0);
}

// 正常解析：最大值 65535
TEST(HelixAi0BDFF2059B49C097, ParseMaxValue) {
    unsigned short out = 0;
    int rc = parse_u16("65535", &out);
    EXPECT_EQ(rc, 1);
    EXPECT_EQ(out, 65535);
}

// 正常解析：中间值
TEST(HelixAi0BDFF2059B49C097, ParseMiddleValue) {
    unsigned short out = 0;
    int rc = parse_u16("12345", &out);
    EXPECT_EQ(rc, 1);
    EXPECT_EQ(out, 12345);
}

// 正常解析：前导零
TEST(HelixAi0BDFF2059B49C097, ParseLeadingZeros) {
    unsigned short out = 0;
    int rc = parse_u16("007", &out);
    EXPECT_EQ(rc, 1);
    EXPECT_EQ(out, 7);
}

// 正常解析：全零字符串
TEST(HelixAi0BDFF2059B49C097, ParseAllZeros) {
    unsigned short out = 0xFFFF;
    int rc = parse_u16("000", &out);
    EXPECT_EQ(rc, 1);
    EXPECT_EQ(out, 0);
}

// 正常解析：单位数
TEST(HelixAi0BDFF2059B49C097, ParseSingleDigit) {
    unsigned short out = 0;
    int rc = parse_u16("9", &out);
    EXPECT_EQ(rc, 1);
    EXPECT_EQ(out, 9);
}

// 边界：65535 是合法上限
TEST(HelixAi0BDFF2059B49C097, BoundaryAt65535) {
    unsigned short out = 0;
    int rc = parse_u16("65535", &out);
    EXPECT_EQ(rc, 1);
    EXPECT_EQ(out, 65535);
}

// 边界：65536 溢出，应拒绝
TEST(HelixAi0BDFF2059B49C097, Overflow65536) {
    unsigned short out = 0xABCD;
    int rc = parse_u16("65536", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0xABCD);
}

// 溢出：99999
TEST(HelixAi0BDFF2059B49C097, Overflow99999) {
    unsigned short out = 0x1234;
    int rc = parse_u16("99999", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0x1234);
}

// 错误：NULL text
TEST(HelixAi0BDFF2059B49C097, NullText) {
    unsigned short out = 0x5555;
    int rc = parse_u16(nullptr, &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0x5555);
}

// 错误：NULL out
TEST(HelixAi0BDFF2059B49C097, NullOut) {
    int rc = parse_u16("123", nullptr);
    EXPECT_EQ(rc, 0);
}

// 错误：空字符串
TEST(HelixAi0BDFF2059B49C097, EmptyString) {
    unsigned short out = 0x7777;
    int rc = parse_u16("", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0x7777);
}

// 错误：负号
TEST(HelixAi0BDFF2059B49C097, NegativeSign) {
    unsigned short out = 0x1111;
    int rc = parse_u16("-1", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0x1111);
}

// 错误：正号
TEST(HelixAi0BDFF2059B49C097, PositiveSign) {
    unsigned short out = 0x2222;
    int rc = parse_u16("+1", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0x2222);
}

// 错误：前导空格
TEST(HelixAi0BDFF2059B49C097, LeadingSpace) {
    unsigned short out = 0x3333;
    int rc = parse_u16(" 1", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0x3333);
}

// 错误：尾随空格
TEST(HelixAi0BDFF2059B49C097, TrailingSpace) {
    unsigned short out = 0x4444;
    int rc = parse_u16("1 ", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0x4444);
}

// 错误：包含字母
TEST(HelixAi0BDFF2059B49C097, ContainsLetter) {
    unsigned short out = 0x5555;
    int rc = parse_u16("1a", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0x5555);
}

// 错误：以字母开头
TEST(HelixAi0BDFF2059B49C097, StartsWithLetter) {
    unsigned short out = 0x6666;
    int rc = parse_u16("a1", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0x6666);
}

// 错误：包含小数点
TEST(HelixAi0BDFF2059B49C097, ContainsDot) {
    unsigned short out = 0x7777;
    int rc = parse_u16("12.3", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0x7777);
}

// 失败时 out 保持不变（哨兵值验证）
TEST(HelixAi0BDFF2059B49C097, OutUnchangedOnFailure) {
    unsigned short out = 0xBEEF;
    int rc = parse_u16("xyz", &out);
    EXPECT_EQ(rc, 0);
    EXPECT_EQ(out, 0xBEEF);
}

// 正常解析：前导零加多位数
TEST(HelixAi0BDFF2059B49C097, LeadingZerosMultiDigit) {
    unsigned short out = 0;
    int rc = parse_u16("000123", &out);
    EXPECT_EQ(rc, 1);
    EXPECT_EQ(out, 123);
}
