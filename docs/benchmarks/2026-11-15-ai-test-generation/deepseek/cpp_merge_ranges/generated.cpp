#include "subject.hpp"

#include <gtest/gtest.h>

#include <algorithm>
#include <utility>
#include <vector>

using Ranges = std::vector<std::pair<int, int>>;

TEST(HelixAi8BA3EAF619E8EE73, EmptyInputReturnsEmpty) {
    // 空输入返回空结果
    EXPECT_EQ(merge_ranges(Ranges{}), Ranges{});
}

TEST(HelixAi8BA3EAF619E8EE73, SingleRangeUnchanged) {
    // 单个区间原样返回
    EXPECT_EQ(merge_ranges(Ranges{{1, 5}}), Ranges{{1, 5}});
}

TEST(HelixAi8BA3EAF619E8EE73, SinglePointRange) {
    // 单点区间
    EXPECT_EQ(merge_ranges(Ranges{{3, 3}}), Ranges{{3, 3}});
}

TEST(HelixAi8BA3EAF619E8EE73, DisjointRangesStaySeparate) {
    // 不相交区间保持分离
    EXPECT_EQ(merge_ranges(Ranges{{1, 2}, {4, 5}}), (Ranges{{1, 2}, {4, 5}}));
}

TEST(HelixAi8BA3EAF619E8EE73, AdjacentRangesNotMerged) {
    // 仅相邻（端点不共享）不合并
    EXPECT_EQ(merge_ranges(Ranges{{1, 2}, {3, 4}}), (Ranges{{1, 2}, {3, 4}}));
}

TEST(HelixAi8BA3EAF619E8EE73, SharedEndpointMerged) {
    // 共享端点视为重叠，合并
    EXPECT_EQ(merge_ranges(Ranges{{1, 3}, {3, 5}}), Ranges{{1, 5}});
}

TEST(HelixAi8BA3EAF619E8EE73, OverlappingRangesMerged) {
    // 重叠区间合并
    EXPECT_EQ(merge_ranges(Ranges{{1, 4}, {2, 6}}), Ranges{{1, 6}});
}

TEST(HelixAi8BA3EAF619E8EE73, ContainedRangeMerged) {
    // 被包含区间合并到外层
    EXPECT_EQ(merge_ranges(Ranges{{1, 10}, {3, 5}}), Ranges{{1, 10}});
}

TEST(HelixAi8BA3EAF619E8EE73, UnsortedInputSorted) {
    // 乱序输入结果有序
    EXPECT_EQ(merge_ranges(Ranges{{5, 6}, {1, 2}, {3, 4}}), (Ranges{{1, 2}, {3, 4}, {5, 6}}));
}

TEST(HelixAi8BA3EAF619E8EE73, ReversedEndpointsNormalized) {
    // 反向端点被规范化
    EXPECT_EQ(merge_ranges(Ranges{{5, 1}}), Ranges{{1, 5}});
}

TEST(HelixAi8BA3EAF619E8EE73, ReversedEndpointsMerged) {
    // 反向端点规范化后参与合并
    EXPECT_EQ(merge_ranges(Ranges{{6, 2}, {3, 4}}), Ranges{{2, 6}});
}

TEST(HelixAi8BA3EAF619E8EE73, MultipleMerges) {
    // 多组区间合并
    EXPECT_EQ(merge_ranges(Ranges{{1, 3}, {2, 6}, {8, 10}, {15, 18}, {17, 20}}),
              (Ranges{{1, 6}, {8, 10}, {15, 20}}));
}

TEST(HelixAi8BA3EAF619E8EE73, InputPreserved) {
    // 输入不被修改
    Ranges input{{5, 1}, {2, 3}};
    const Ranges original = input;
    merge_ranges(input);
    EXPECT_EQ(input, original);
}

TEST(HelixAi8BA3EAF619E8EE73, NegativeValues) {
    // 负值区间合并
    EXPECT_EQ(merge_ranges(Ranges{{-5, -1}, {-3, 0}}), Ranges{{-5, 0}});
}

TEST(HelixAi8BA3EAF619E8EE73, DuplicateRanges) {
    // 重复区间合并为一个
    EXPECT_EQ(merge_ranges(Ranges{{1, 2}, {1, 2}}), Ranges{{1, 2}});
}

TEST(HelixAi8BA3EAF619E8EE73, ChainOverlap) {
    // 链式重叠合并
    EXPECT_EQ(merge_ranges(Ranges{{1, 2}, {2, 3}, {3, 4}}), Ranges{{1, 4}});
}
