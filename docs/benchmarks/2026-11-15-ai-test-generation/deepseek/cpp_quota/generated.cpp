#include <gtest/gtest.h>
#include "subject.hpp"

// 正常消费：额度充足时成功并累加已用额度
TEST(HelixAi79DA9FB5102F83D4, ConsumeWithinLimitSucceeds) {
    Quota q(10);
    EXPECT_TRUE(q.consume(3));
    EXPECT_EQ(q.used(), 3);
    EXPECT_EQ(q.remaining(), 7);
}

// 多次消费累加
TEST(HelixAi79DA9FB5102F83D4, ConsumeAccumulates) {
    Quota q(10);
    EXPECT_TRUE(q.consume(4));
    EXPECT_TRUE(q.consume(5));
    EXPECT_EQ(q.used(), 9);
    EXPECT_EQ(q.remaining(), 1);
}

// 恰好用尽剩余额度应成功
TEST(HelixAi79DA9FB5102F83D4, ConsumeExactRemainingSucceeds) {
    Quota q(10);
    EXPECT_TRUE(q.consume(10));
    EXPECT_EQ(q.used(), 10);
    EXPECT_EQ(q.remaining(), 0);
}

// 消费 0 应成功且不改变状态
TEST(HelixAi79DA9FB5102F83D4, ConsumeZeroSucceeds) {
    Quota q(10);
    EXPECT_TRUE(q.consume(0));
    EXPECT_EQ(q.used(), 0);
    EXPECT_EQ(q.remaining(), 10);
}

// 超出剩余额度应失败且状态不变
TEST(HelixAi79DA9FB5102F83D4, ConsumeExceedingRemainingFails) {
    Quota q(10);
    EXPECT_TRUE(q.consume(7));
    EXPECT_FALSE(q.consume(4));
    EXPECT_EQ(q.used(), 7);
    EXPECT_EQ(q.remaining(), 3);
}

// 负数金额应失败且状态不变
TEST(HelixAi79DA9FB5102F83D4, ConsumeNegativeFails) {
    Quota q(10);
    EXPECT_FALSE(q.consume(-1));
    EXPECT_EQ(q.used(), 0);
    EXPECT_EQ(q.remaining(), 10);
}

// 额度为 0 时消费 0 成功，消费正数失败
TEST(HelixAi79DA9FB5102F83D4, ZeroLimitBehavior) {
    Quota q(0);
    EXPECT_TRUE(q.consume(0));
    EXPECT_FALSE(q.consume(1));
    EXPECT_EQ(q.used(), 0);
    EXPECT_EQ(q.remaining(), 0);
}

// 负额度被构造为 0，任何正数消费失败
TEST(HelixAi79DA9FB5102F83D4, NegativeLimitClampedToZero) {
    Quota q(-5);
    EXPECT_EQ(q.remaining(), 0);
    EXPECT_FALSE(q.consume(1));
    EXPECT_EQ(q.used(), 0);
}
