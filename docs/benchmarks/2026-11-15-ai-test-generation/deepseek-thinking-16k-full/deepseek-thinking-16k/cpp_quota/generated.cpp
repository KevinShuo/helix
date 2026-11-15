#include <gtest/gtest.h>

#include "subject.hpp"

// 正数上限：初始未使用额度与剩余额度一致。
TEST(HelixAi79DA9FB5102F83D4, InitialStateReflectsPositiveLimit) {
    Quota quota(5);
    EXPECT_EQ(quota.used(), 0);
    EXPECT_EQ(quota.remaining(), 5);
}

// 额度内的正数消费成功并更新已用/剩余额度。
TEST(HelixAi79DA9FB5102F83D4, ConsumePositiveWithinQuotaSucceeds) {
    Quota quota(10);
    EXPECT_TRUE(quota.consume(3));
    EXPECT_EQ(quota.used(), 3);
    EXPECT_EQ(quota.remaining(), 7);
}

// 恰好消费完剩余额度应成功。
TEST(HelixAi79DA9FB5102F83D4, ConsumeExactRemainingSucceeds) {
    Quota quota(5);
    EXPECT_TRUE(quota.consume(5));
    EXPECT_EQ(quota.used(), 5);
    EXPECT_EQ(quota.remaining(), 0);
}

// 消费 0 应成功且不改变状态。
TEST(HelixAi79DA9FB5102F83D4, ConsumeZeroSucceedsWithoutChangingState) {
    Quota quota(4);
    EXPECT_TRUE(quota.consume(0));
    EXPECT_EQ(quota.used(), 0);
    EXPECT_EQ(quota.remaining(), 4);
    EXPECT_TRUE(quota.consume(0));
    EXPECT_EQ(quota.used(), 0);
    EXPECT_EQ(quota.remaining(), 4);
}

// 负数消费应被拒绝且状态不变。
TEST(HelixAi79DA9FB5102F83D4, ConsumeNegativeIsRejectedWithoutStateChange) {
    Quota quota(5);
    EXPECT_TRUE(quota.consume(2));
    EXPECT_FALSE(quota.consume(-1));
    EXPECT_EQ(quota.used(), 2);
    EXPECT_EQ(quota.remaining(), 3);
    EXPECT_FALSE(quota.consume(-100));
    EXPECT_EQ(quota.used(), 2);
    EXPECT_EQ(quota.remaining(), 3);
}

// 超过剩余额度的消费应被拒绝且状态不变。
TEST(HelixAi79DA9FB5102F83D4, ConsumeMoreThanRemainingIsRejectedWithoutStateChange) {
    Quota quota(5);
    EXPECT_TRUE(quota.consume(2));
    EXPECT_FALSE(quota.consume(4));
    EXPECT_EQ(quota.used(), 2);
    EXPECT_EQ(quota.remaining(), 3);
}

// 部分消费后再恰好消费剩余额度应成功。
TEST(HelixAi79DA9FB5102F83D4, ConsumeExactRemainingAfterPartialUseSucceeds) {
    Quota quota(7);
    EXPECT_TRUE(quota.consume(2));
    EXPECT_TRUE(quota.consume(5));
    EXPECT_EQ(quota.used(), 7);
    EXPECT_EQ(quota.remaining(), 0);
}

// 额度耗尽后仅 0 仍可消费，正数消费被拒绝。
TEST(HelixAi79DA9FB5102F83D4, ConsumeWhenExhaustedAcceptsOnlyZero) {
    Quota quota(5);
    EXPECT_TRUE(quota.consume(5));
    EXPECT_TRUE(quota.consume(0));
    EXPECT_EQ(quota.used(), 5);
    EXPECT_EQ(quota.remaining(), 0);
    EXPECT_FALSE(quota.consume(1));
    EXPECT_EQ(quota.used(), 5);
    EXPECT_EQ(quota.remaining(), 0);
}

// 0 额度等价于空额度：0 可消费，正数不可消费。
TEST(HelixAi79DA9FB5102F83D4, ZeroLimitBehavesAsEmptyQuota) {
    Quota quota(0);
    EXPECT_EQ(quota.used(), 0);
    EXPECT_EQ(quota.remaining(), 0);
    EXPECT_TRUE(quota.consume(0));
    EXPECT_EQ(quota.used(), 0);
    EXPECT_EQ(quota.remaining(), 0);
    EXPECT_FALSE(quota.consume(1));
    EXPECT_EQ(quota.used(), 0);
    EXPECT_EQ(quota.remaining(), 0);
}

// 负上限构造时按契约钳制为 0。
TEST(HelixAi79DA9FB5102F83D4, NegativeLimitIsClampedToZero) {
    Quota quota(-3);
    EXPECT_EQ(quota.used(), 0);
    EXPECT_EQ(quota.remaining(), 0);
    EXPECT_TRUE(quota.consume(0));
    EXPECT_EQ(quota.used(), 0);
    EXPECT_EQ(quota.remaining(), 0);
    EXPECT_FALSE(quota.consume(1));
    EXPECT_EQ(quota.used(), 0);
    EXPECT_EQ(quota.remaining(), 0);
}

// 多次成功消费应连续累加已用额度。
TEST(HelixAi79DA9FB5102F83D4, RepeatedSuccessfulConsumptionUpdatesState) {
    Quota quota(10);
    EXPECT_TRUE(quota.consume(3));
    EXPECT_EQ(quota.used(), 3);
    EXPECT_EQ(quota.remaining(), 7);
    EXPECT_TRUE(quota.consume(4));
    EXPECT_EQ(quota.used(), 7);
    EXPECT_EQ(quota.remaining(), 3);
    EXPECT_TRUE(quota.consume(3));
    EXPECT_EQ(quota.used(), 10);
    EXPECT_EQ(quota.remaining(), 0);
}
