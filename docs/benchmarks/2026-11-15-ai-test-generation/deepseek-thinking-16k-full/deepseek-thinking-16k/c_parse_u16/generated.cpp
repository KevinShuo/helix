#include <gtest/gtest.h>

extern "C" {
#include "subject.h"
}

TEST(HelixAi0BDFF2059B49C097, ParseZero) {
    unsigned short out = 0xFFFF;
    EXPECT_EQ(parse_u16("0", &out), 1);
    EXPECT_EQ(out, 0);
}

TEST(HelixAi0BDFF2059B49C097, ParseOne) {
    unsigned short out = 0;
    EXPECT_EQ(parse_u16("1", &out), 1);
    EXPECT_EQ(out, 1);
}

TEST(HelixAi0BDFF2059B49C097, ParseSimple) {
    unsigned short out = 0;
    EXPECT_EQ(parse_u16("42", &out), 1);
    EXPECT_EQ(out, 42);
}

TEST(HelixAi0BDFF2059B49C097, ParseMaxU16) {
    unsigned short out = 0;
    EXPECT_EQ(parse_u16("65535", &out), 1);
    EXPECT_EQ(out, 65535);
}

TEST(HelixAi0BDFF2059B49C097, ParseBoundaryBelowMax) {
    unsigned short out = 0;
    EXPECT_EQ(parse_u16("65534", &out), 1);
    EXPECT_EQ(out, 65534);
}

TEST(HelixAi0BDFF2059B49C097, ParseLeadingZeros) {
    unsigned short out = 0;
    EXPECT_EQ(parse_u16("000123", &out), 1);
    EXPECT_EQ(out, 123);
}

TEST(HelixAi0BDFF2059B49C097, ParseLeadingZerosZero) {
    unsigned short out = 0xFFFF;
    EXPECT_EQ(parse_u16("00000", &out), 1);
    EXPECT_EQ(out, 0);
}

TEST(HelixAi0BDFF2059B49C097, ParseLeadingZerosMax) {
    unsigned short out = 0;
    EXPECT_EQ(parse_u16("00065535", &out), 1);
    EXPECT_EQ(out, 65535);
}

TEST(HelixAi0BDFF2059B49C097, ParseManyLeadingZeros) {
    unsigned short out = 0xFFFF;
    EXPECT_EQ(parse_u16("000000000000000000000000000000000000", &out), 1);
    EXPECT_EQ(out, 0);
}

TEST(HelixAi0BDFF2059B49C097, RejectOverflowBoundary) {
    unsigned short out = 777;
    EXPECT_EQ(parse_u16("65536", &out), 0);
    EXPECT_EQ(out, 777);
}

TEST(HelixAi0BDFF2059B49C097, RejectOverflowLarge) {
    unsigned short out = 777;
    EXPECT_EQ(parse_u16("99999999999999999999", &out), 0);
    EXPECT_EQ(out, 777);
}

TEST(HelixAi0BDFF2059B49C097, RejectOverflowWithLeadingZeros) {
    unsigned short out = 777;
    EXPECT_EQ(parse_u16("00065536", &out), 0);
    EXPECT_EQ(out, 777);
}

TEST(HelixAi0BDFF2059B49C097, RejectNullText) {
    unsigned short out = 777;
    EXPECT_EQ(parse_u16(nullptr, &out), 0);
    EXPECT_EQ(out, 777);
}

TEST(HelixAi0BDFF2059B49C097, RejectNullOut) {
    EXPECT_EQ(parse_u16("42", nullptr), 0);
}

TEST(HelixAi0BDFF2059B49C097, RejectEmptyString) {
    unsigned short out = 777;
    EXPECT_EQ(parse_u16("", &out), 0);
    EXPECT_EQ(out, 777);
}

TEST(HelixAi0BDFF2059B49C097, RejectSigns) {
    unsigned short out = 777;
    EXPECT_EQ(parse_u16("+42", &out), 0);
    EXPECT_EQ(out, 777);
    EXPECT_EQ(parse_u16("-42", &out), 0);
    EXPECT_EQ(out, 777);
}

TEST(HelixAi0BDFF2059B49C097, RejectWhitespace) {
    unsigned short out = 777;
    EXPECT_EQ(parse_u16(" 42", &out), 0);
    EXPECT_EQ(out, 777);
    EXPECT_EQ(parse_u16("42 ", &out), 0);
    EXPECT_EQ(out, 777);
    EXPECT_EQ(parse_u16("4 2", &out), 0);
    EXPECT_EQ(out, 777);
}

TEST(HelixAi0BDFF2059B49C097, RejectNonDigits) {
    unsigned short out = 777;
    EXPECT_EQ(parse_u16("42a", &out), 0);
    EXPECT_EQ(out, 777);
    EXPECT_EQ(parse_u16("a42", &out), 0);
    EXPECT_EQ(out, 777);
    EXPECT_EQ(parse_u16("1.0", &out), 0);
    EXPECT_EQ(out, 777);
    EXPECT_EQ(parse_u16("0x10", &out), 0);
    EXPECT_EQ(out, 777);
}
