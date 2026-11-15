#include "subject.hpp"
#include <gtest/gtest.h>
#include <vector>
#include <utility>

TEST(HelixAi8BA3EAF619E8EE73, EmptyInputReturnsEmpty) {
    std::vector<std::pair<int, int>> input = {};
    auto result = merge_ranges(input);
    EXPECT_TRUE(result.empty());
}

TEST(HelixAi8BA3EAF619E8EE73, SingleRangeUnchanged) {
    std::vector<std::pair<int, int>> input = {{1, 5}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 1u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 5);
}

TEST(HelixAi8BA3EAF619E8EE73, ReversedEndpointsNormalized) {
    std::vector<std::pair<int, int>> input = {{5, 1}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 1u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 5);
}

TEST(HelixAi8BA3EAF619E8EE73, OverlappingSharedEndpointMerged) {
    // [1,3] and [3,5] share endpoint 3 → overlap
    std::vector<std::pair<int, int>> input = {{1, 3}, {3, 5}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 1u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 5);
}

TEST(HelixAi8BA3EAF619E8EE73, AdjacentRangesNotMerged) {
    // [1,3] and [4,5] are adjacent (4 > 3) → not overlapping
    std::vector<std::pair<int, int>> input = {{1, 3}, {4, 5}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 2u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 3);
    EXPECT_EQ(result[1].first, 4);
    EXPECT_EQ(result[1].second, 5);
}

TEST(HelixAi8BA3EAF619E8EE73, NonOverlappingRangesStaySeparate) {
    std::vector<std::pair<int, int>> input = {{1, 2}, {10, 20}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 2u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 2);
    EXPECT_EQ(result[1].first, 10);
    EXPECT_EQ(result[1].second, 20);
}

TEST(HelixAi8BA3EAF619E8EE73, MultipleChainedOverlapsMerged) {
    // [1,4], [3,7], [6,9] chain together via shared/overlapping endpoints
    std::vector<std::pair<int, int>> input = {{1, 4}, {3, 7}, {6, 9}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 1u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 9);
}

TEST(HelixAi8BA3EAF619E8EE73, ContainedRangeMerged) {
    // [1,10] fully contains [3,5]
    std::vector<std::pair<int, int>> input = {{1, 10}, {3, 5}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 1u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 10);
}

TEST(HelixAi8BA3EAF619E8EE73, InputPreserved) {
    std::vector<std::pair<int, int>> input = {{5, 1}, {3, 7}};
    auto result = merge_ranges(input);
    // Input must remain unchanged
    ASSERT_EQ(input.size(), 2u);
    EXPECT_EQ(input[0].first, 5);
    EXPECT_EQ(input[0].second, 1);
    EXPECT_EQ(input[1].first, 3);
    EXPECT_EQ(input[1].second, 7);
    // Result: normalized [1,5] and [3,7] overlap → [1,7]
    ASSERT_EQ(result.size(), 1u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 7);
}

TEST(HelixAi8BA3EAF619E8EE73, MultipleReversedEndpoints) {
    // {9,1}→[1,9], {5,3}→[3,5]; they overlap
    std::vector<std::pair<int, int>> input = {{9, 1}, {5, 3}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 1u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 9);
}

TEST(HelixAi8BA3EAF619E8EE73, UnsortedInputProducesSortedMergedOutput) {
    // [10,20], [1,5], [7,12] → sorted: [1,5],[7,12],[10,20] → [1,5],[7,20]
    std::vector<std::pair<int, int>> input = {{10, 20}, {1, 5}, {7, 12}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 2u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 5);
    EXPECT_EQ(result[1].first, 7);
    EXPECT_EQ(result[1].second, 20);
}

TEST(HelixAi8BA3EAF619E8EE73, IdenticalRangesMerged) {
    std::vector<std::pair<int, int>> input = {{2, 5}, {2, 5}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 1u);
    EXPECT_EQ(result[0].first, 2);
    EXPECT_EQ(result[0].second, 5);
}

TEST(HelixAi8BA3EAF619E8EE73, NegativeAndZeroRanges) {
    // [-5,-1] and [-3,2] overlap; [4,6] is separate
    std::vector<std::pair<int, int>> input = {{-5, -1}, {-3, 2}, {4, 6}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 2u);
    EXPECT_EQ(result[0].first, -5);
    EXPECT_EQ(result[0].second, 2);
    EXPECT_EQ(result[1].first, 4);
    EXPECT_EQ(result[1].second, 6);
}

TEST(HelixAi8BA3EAF619E8EE73, SinglePointRangeOverlap) {
    // [1,3] and [3,3] share endpoint 3 → merge to [1,3]
    std::vector<std::pair<int, int>> input = {{1, 3}, {3, 3}};
    auto result = merge_ranges(input);
    ASSERT_EQ(result.size(), 1u);
    EXPECT_EQ(result[0].first, 1);
    EXPECT_EQ(result[0].second, 3);
}
