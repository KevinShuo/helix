#include <gtest/gtest.h>
#include "subject.hpp"
TEST(HelixAi8BA3EAF619E8EE73, Contract) {
    using R = std::vector<std::pair<int,int>>;
    const R input{{3, 1}, {3, 5}, {9, 10}, {2, 2}};
    const R saved = input;
    EXPECT_EQ(merge_ranges(input), (R{{1, 5}, {9, 10}}));
    EXPECT_EQ(input, saved);
    EXPECT_EQ(merge_ranges(R{{1, 10}, {2, 3}}), (R{{1, 10}}));
    EXPECT_EQ(merge_ranges(R{}), R{});
    EXPECT_EQ(merge_ranges(R{{1, 2}, {3, 4}}), (R{{1, 2}, {3, 4}}));
}
