#include <gtest/gtest.h>
extern "C" {
#include "subject.h"
}
TEST(HelixAiCC55F06B8AD1074F, Contract) {
    char out[8] = "xxxxxxx";
    EXPECT_EQ(copy_prefix(out, 4, "abcdef"), 6u); EXPECT_STREQ(out, "abc");
    out[0] = 'x'; EXPECT_EQ(copy_prefix(out, 1, "abc"), 3u); EXPECT_EQ(out[0], '\0');
    out[0] = 'x'; EXPECT_EQ(copy_prefix(out, 0, "abc"), 3u); EXPECT_EQ(out[0], 'x');
    EXPECT_EQ(copy_prefix(nullptr, 9, "abc"), 3u);
    EXPECT_EQ(copy_prefix(out, 8, nullptr), 0u); EXPECT_EQ(out[0], '\0');
}
