#include "subject.hpp"

#include <gtest/gtest.h>

#include <utility>
#include <vector>

using Range = std::pair<int, int>;
using Ranges = std::vector<Range>;

// 空输入返回空结果
TEST(HelixAi8BA3EAF619E8EE73, EmptyInputReturnsEmpty) {
    EXPECT_EQ(merge_ranges(Ranges{}), Ranges{});
}

// 单个区间原样返回
TEST(HelixAi8BA3EAF619E8EE73, SingleRangeUnchanged) {
    EXPECT_EQ(merge_ranges(Ranges{{1, 3}}), (Ranges{{1, 3}}));
}

// 重叠区间合并（经典示例）
TEST(HelixAi8BA3EAF619E8EE73, OverlappingRangesMerged) {
    const Ranges input{{1, 3}, {2, 6}, {8, 10}, {15, 18}};
    EXPECT_EQ(merge_ranges(input), (Ranges{{1, 6}, {8, 10}, {15, 18}}));
}

// 共享端点视为重叠
TEST(HelixAi8BA3EAF619E8EE73, SharedEndpointCountsAsOverlap) {
    EXPECT_EQ(merge_ranges(Ranges{{1, 4}, {4, 5}}), (Ranges{{1, 5}}));
}

// 仅相邻（不共享端点）不合并
TEST(HelixAi8BA3EAF619E8EE73, AdjacentButNotOverlappingStaySeparate) {
    EXPECT_EQ(merge_ranges(Ranges{{1, 2}, {3, 4}}), (Ranges{{1, 2}, {3, 4}}));
}

// 无序输入按起点排序输出
TEST(HelixAi8BA3EAF619E8EE73, UnsortedInputIsSorted) {
    const Ranges input{{8, 10}, {1, 3}, {2, 6}};
    EXPECT_EQ(merge_ranges(input), (Ranges{{1, 6}, {8, 10}}));
}

// 被包含区间并入外层区间
TEST(HelixAi8BA3EAF619E8EE73, NestedRangeAbsorbed) {
    EXPECT_EQ(merge_ranges(Ranges{{1, 10}, {2, 3}}), (Ranges{{1, 10}}));
}

// 链式重叠整体合并
TEST(HelixAi8BA3EAF619E8EE73, ChainOverlapMergedIntoOne) {
    EXPECT_EQ(merge_ranges(Ranges{{1, 2}, {2, 5}, {4, 8}}), (Ranges{{1, 8}}));
}

// 反向端点被规范化
TEST(HelixAi8BA3EAF619E8EE73, ReversedEndpointsNormalized) {
    EXPECT_EQ(merge_ranges(Ranges{{5, 3}}), (Ranges{{3, 5}}));
}

// 反向端点规范化后再参与合并
TEST(HelixAi8BA3EAF619E8EE73, ReversedEndpointsThenMerge) {
    EXPECT_EQ(merge_ranges(Ranges{{7, 2}, {4, 5}}), (Ranges{{2, 7}}));
}

// 重复区间只保留一份
TEST(HelixAi8BA3EAF619E8EE73, DuplicateRangesCollapsed) {
    EXPECT_EQ(merge_ranges(Ranges{{2, 4}, {2, 4}}), (Ranges{{2, 4}}));
}

// 单点区间：相同点合并，不同点分离
TEST(HelixAi8BA3EAF619E8EE73, SinglePointRanges) {
    EXPECT_EQ(merge_ranges(Ranges{{1, 1}, {1, 1}}), (Ranges{{1, 1}}));
    EXPECT_EQ(merge_ranges(Ranges{{1, 1}, {2, 2}}), (Ranges{{1, 1}, {2, 2}}));
}

// 负值区间同样遵循合并规则
TEST(HelixAi8BA3EAF619E8EE73, NegativeValuesHandled) {
    EXPECT_EQ(merge_ranges(Ranges{{-10, -5}, {-6, -1}}), (Ranges{{-10, -1}}));
    EXPECT_EQ(merge_ranges(Ranges{{-3, -2}, {-1, 0}}), (Ranges{{-3, -2}, {-1, 0}}));
}

// 不修改输入
TEST(HelixAi8BA3EAF619E8EE73, InputIsNotModified) {
    const Ranges input{{9, 3}, {1, 2}, {5, 4}};
    const Ranges expected = input;
    merge_ranges(input);
    EXPECT_EQ(input, expected);
}
