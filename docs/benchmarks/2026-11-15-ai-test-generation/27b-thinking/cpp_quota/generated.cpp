#include "subject.hpp"
#include <gtest/gtest.h>

TEST(HelixAi79DA9FB5102F83D4, ConsumeZeroOnFreshQuota) {
    Quota q(10);
    EXPECT_TRUE(q.consume(0));
    EXPECT_EQ(q.used(), 0);
    EXPECT_EQ(q.remaining(), 10);
}

TEST(HelixAi79DA9FB5102F83D4, ConsumeValidAmountWithinQuota) {
    Quota q(10);
    EXPECT_TRUE(q.consume(3));
    EXPECT_EQ(q.used(), 3);
    EXPECT_EQ(q.remaining(), 7);
}

TEST(HelixAi79DA9FB5102F83D4, ConsumeExactFit) {
    Quota q(10);
    EXPECT_TRUE(q.consume(10));
    EXPECT_EQ(q.used(), 10);
    EXPECT_EQ(q.remaining(), 0);
}

TEST(HelixAi79DA9FB5102F83D4, ConsumeExactFitOnRemaining) {
    Quota q(10);
    EXPECT_TRUE(q.consume(4));
    EXPECT_TRUE(q.consume(6));
    EXPECT_EQ(q.used(), 10);
    EXPECT_EQ(q.remaining(), 0);
}

TEST(HelixAi79DA9FB5102F83D4, ConsumeExceedsRemainingRejected) {
    Quota q(10);
    EXPECT_TRUE(q.consume(5));
    EXPECT_FALSE(q.consume(6));
    EXPECT_EQ(q.used(), 5);
    EXPECT_EQ(q.remaining(), 5);
}

TEST(HelixAi79DA9FB5102F83D4, ConsumeNegativeRejected) {
    Quota q(10);
    EXPECT_FALSE(q.consume(-1));
    EXPECT_EQ(q.used(), 0);
    EXPECT_EQ(q.remaining(), 10);
}

TEST(HelixAi79DA9FB5102F83D4, ConsumeExceedsFullQuotaRejected) {
    Quota q(10);
    EXPECT_FALSE(q.consume(11));
    EXPECT_EQ(q.used(), 0);
    EXPECT_EQ(q.remaining(), 10);
}

TEST(HelixAi79DA9FB5102F83D4, ConsumeOnZeroLimit) {
    Quota q(0);
    EXPECT_TRUE(q.consume(0));
    EXPECT_EQ(q.used(), 0);
    EXPECT_FALSE(q.consume(1));
    EXPECT_EQ(q.used(), 0);
}

TEST(HelixAi79DA9FB5102F83D4, ConsumeAfterFullQuotaExhausted) {
    Quota q(5);
    EXPECT_TRUE(q.consume(5));
    EXPECT_FALSE(q.consume(1));
    EXPECT_EQ(q.used(), 5);
    EXPECT_FALSE(q.consume(0));
    EXPECT_EQ(q.used(), 5);
}

TEST(HelixAi79DA9FB5102F83D4, NegativeLimitClampedToZero) {
    Quota q(-7);
    EXPECT_EQ(q.remaining(), 0);
    EXPECT_TRUE(q.consume(0));
    EXPECT_FALSE(q.consume(1));
    EXPECT_EQ(q.used(), 0);
}

TEST(HelixAi79DA9FB5102F83D4, MultipleSequentialConsumesAccumulate) {
    Quota q(20);
    EXPECT_TRUE(q.consume(2));
    EXPECT_TRUE(q.consume(3));
    EXPECT_TRUE(q.consume(5));
    EXPECT_EQ(q.used(), 10);
    EXPECT_EQ(q.remaining(), 10);
}

TEST(HelixAi79DA9FB5102F83D4, RejectionLeavesStateUnchanged) {
    Quota q(8);
    EXPECT_TRUE(q.consume(3));
    EXPECT_FALSE(q.consume(6));
    EXPECT_EQ(q.used(), 3);
    EXPECT_EQ(q.remaining(), 5);
    EXPECT_TRUE(q.consume(5));
    EXPECT_EQ(q.used(), 8);
}
