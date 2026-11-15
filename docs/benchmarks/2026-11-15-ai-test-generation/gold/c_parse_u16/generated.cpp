#include <gtest/gtest.h>
extern "C" {
#include "subject.h"
}
TEST(HelixAi0BDFF2059B49C097, Contract) {
    unsigned short out = 123;
    EXPECT_EQ(parse_u16("0", &out), 1); EXPECT_EQ(out, 0);
    EXPECT_EQ(parse_u16("65535", &out), 1); EXPECT_EQ(out, 65535);
    out = 123; EXPECT_EQ(parse_u16("12x", &out), 0); EXPECT_EQ(out, 123);
    EXPECT_EQ(parse_u16("", &out), 0); EXPECT_EQ(out, 123);
    EXPECT_EQ(parse_u16("65536", &out), 0); EXPECT_EQ(out, 123);
    EXPECT_EQ(parse_u16(nullptr, &out), 0); EXPECT_EQ(out, 123);
    EXPECT_EQ(parse_u16("1", nullptr), 0);
}
