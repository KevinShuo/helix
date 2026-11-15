#include <gtest/gtest.h>
#include "subject.hpp"
TEST(HelixAi79DA9FB5102F83D4, Contract) {
    Quota q(10);
    EXPECT_TRUE(q.consume(4)); EXPECT_EQ(q.used(), 4); EXPECT_EQ(q.remaining(), 6);
    EXPECT_FALSE(q.consume(7)); EXPECT_EQ(q.used(), 4); EXPECT_EQ(q.remaining(), 6);
    EXPECT_FALSE(q.consume(-1)); EXPECT_EQ(q.used(), 4);
    EXPECT_TRUE(q.consume(6)); EXPECT_EQ(q.used(), 10); EXPECT_EQ(q.remaining(), 0);
    EXPECT_TRUE(q.consume(0)); EXPECT_FALSE(q.consume(1)); EXPECT_EQ(q.used(), 10);
    Quota zero(-5); EXPECT_TRUE(zero.consume(0)); EXPECT_FALSE(zero.consume(1));
}
