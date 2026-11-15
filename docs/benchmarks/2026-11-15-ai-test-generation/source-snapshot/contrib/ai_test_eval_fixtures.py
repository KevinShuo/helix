"""Fixed synthetic contracts, independently written control tests and mutants."""

from __future__ import annotations

CASES = [
    {
        "id": "c_parse_u16",
        "language": "c",
        "source_file": "subject.c",
        "needle": "if (!text",
        "files": {
            "subject.h": """#pragma once
/* Parse a nonempty sequence of ASCII decimal digits, including leading zeros.
   Accept 0..65535 inclusive. Return 1 on success, otherwise 0. Reject NULL text,
   NULL output, signs, whitespace, non-digits and overflow. On failure leave
   the output value unchanged. */
int parse_u16(const char *text, unsigned short *out);
""",
            "subject.c": """#include "subject.h"
int parse_u16(const char *text, unsigned short *out) {
    if (!text || !out || !*text) return 0;
    unsigned long value = 0;
    for (const char *p = text; *p; ++p) {
        if (*p < '0' || *p > '9') return 0;
        value = value * 10 + (unsigned long)(*p - '0');
        if (value > 65535UL) return 0;
    }
    *out = (unsigned short)value;
    return 1;
}
""",
        },
        "gold": """#include <gtest/gtest.h>
extern "C" {
#include "subject.h"
}
TEST(__SUITE__, Contract) {
    unsigned short out = 123;
    EXPECT_EQ(parse_u16("0", &out), 1); EXPECT_EQ(out, 0);
    EXPECT_EQ(parse_u16("65535", &out), 1); EXPECT_EQ(out, 65535);
    out = 123; EXPECT_EQ(parse_u16("12x", &out), 0); EXPECT_EQ(out, 123);
    EXPECT_EQ(parse_u16("", &out), 0); EXPECT_EQ(out, 123);
    EXPECT_EQ(parse_u16("65536", &out), 0); EXPECT_EQ(out, 123);
    EXPECT_EQ(parse_u16(nullptr, &out), 0); EXPECT_EQ(out, 123);
    EXPECT_EQ(parse_u16("1", nullptr), 0);
}
""",
        "mutants": [
            {"id": "upper_endpoint", "old": "value > 65535UL", "new": "value >= 65535UL", "meaning": "错误拒绝65535"},
            {"id": "failure_state", "old": "if (*p < '0' || *p > '9') return 0;", "new": "if (*p < '0' || *p > '9') { *out = 0; return 0; }", "meaning": "非法字符失败时破坏输出值"},
            {"id": "empty_input", "old": "!text || !out || !*text", "new": "!text || !out", "meaning": "错误接受空字符串"},
        ],
        "review": "检查成功值、65535/溢出、空输入/非法字符和失败后输出不变；不要把合法NULL参数当成未定义行为。",
    },
    {
        "id": "c_copy_prefix",
        "language": "c",
        "source_file": "subject.c",
        "needle": "size_t required",
        "files": {
            "subject.h": """#pragma once
#include <stddef.h>
/* Return the full source length (excluding NUL), even when truncated.
   NULL source means an empty string. If dst is non-NULL and capacity > 0,
   copy at most capacity-1 bytes and always NUL terminate. Otherwise do not
   write anything. The caller provides a destination of at least capacity bytes. */
size_t copy_prefix(char *dst, size_t capacity, const char *src);
""",
            "subject.c": """#include "subject.h"
#include <string.h>
size_t copy_prefix(char *dst, size_t capacity, const char *src) {
    size_t required = src ? strlen(src) : 0;
    if (dst && capacity > 0) {
        size_t copied = required < capacity - 1 ? required : capacity - 1;
        if (copied) memcpy(dst, src, copied);
        dst[copied] = '\\0';
    }
    return required;
}
""",
        },
        "gold": """#include <gtest/gtest.h>
extern "C" {
#include "subject.h"
}
TEST(__SUITE__, Contract) {
    char out[8] = "xxxxxxx";
    EXPECT_EQ(copy_prefix(out, 4, "abcdef"), 6u); EXPECT_STREQ(out, "abc");
    out[0] = 'x'; EXPECT_EQ(copy_prefix(out, 1, "abc"), 3u); EXPECT_EQ(out[0], '\\0');
    out[0] = 'x'; EXPECT_EQ(copy_prefix(out, 0, "abc"), 3u); EXPECT_EQ(out[0], 'x');
    EXPECT_EQ(copy_prefix(nullptr, 9, "abc"), 3u);
    EXPECT_EQ(copy_prefix(out, 8, nullptr), 0u); EXPECT_EQ(out[0], '\\0');
}
""",
        "mutants": [
            {"id": "required_length", "old": "return required;", "new": "return dst && capacity && required >= capacity ? capacity - 1 : required;", "meaning": "截断时错误返回实际复制长度"},
            {"id": "termination", "old": "dst[copied] = '\\0';", "new": "dst[copied] = 'X';", "meaning": "遗漏NUL终止"},
            {"id": "capacity_one", "old": "dst && capacity > 0", "new": "dst && capacity > 1", "meaning": "容量1时未写入终止符"},
        ],
        "review": "检查截断返回完整长度、NUL终止、容量0/1、NULL源/目标，避免越界读写测试自身。",
    },
    {
        "id": "cpp_merge_ranges",
        "language": "cpp",
        "source_file": "subject.cpp",
        "needle": "auto sorted",
        "files": {
            "subject.hpp": """#pragma once
#include <utility>
#include <vector>
// Return sorted inclusive ranges with overlapping ranges merged. Shared endpoints
// overlap. Non-overlapping ranges stay separate (adjacency alone is not overlap).
// Normalize reversed endpoints. Preserve the input, return empty for empty input.
std::vector<std::pair<int, int>> merge_ranges(const std::vector<std::pair<int, int>>& ranges);
""",
            "subject.cpp": """#include "subject.hpp"
#include <algorithm>
std::vector<std::pair<int, int>> merge_ranges(const std::vector<std::pair<int, int>>& ranges) {
    auto sorted = ranges;
    for (auto &range : sorted) {
        if (range.first > range.second) std::swap(range.first, range.second);
    }
    std::sort(sorted.begin(), sorted.end());
    std::vector<std::pair<int, int>> result;
    for (const auto &range : sorted) {
        if (result.empty() || range.first > result.back().second) result.push_back(range);
        else result.back().second = std::max(result.back().second, range.second);
    }
    return result;
}
""",
        },
        "gold": """#include <gtest/gtest.h>
#include "subject.hpp"
TEST(__SUITE__, Contract) {
    using R = std::vector<std::pair<int,int>>;
    const R input{{3, 1}, {3, 5}, {9, 10}, {2, 2}};
    const R saved = input;
    EXPECT_EQ(merge_ranges(input), (R{{1, 5}, {9, 10}}));
    EXPECT_EQ(input, saved);
    EXPECT_EQ(merge_ranges(R{{1, 10}, {2, 3}}), (R{{1, 10}}));
    EXPECT_EQ(merge_ranges(R{}), R{});
    EXPECT_EQ(merge_ranges(R{{1, 2}, {3, 4}}), (R{{1, 2}, {3, 4}}));
}
""",
        "mutants": [
            {"id": "shared_endpoint", "old": "range.first > result.back().second", "new": "range.first >= result.back().second", "meaning": "共享端点不再合并"},
            {"id": "reversed_range", "old": "if (range.first > range.second) std::swap(range.first, range.second);", "new": "if (range.first > range.second) range.first = range.second;", "meaning": "倒序区间错误缩成单点"},
            {"id": "nested_range", "old": "std::max(result.back().second, range.second)", "new": "range.second", "meaning": "嵌套区间错误缩小外部区间"},
        ],
        "review": "检查共享端点、倒序区间、嵌套区间、排序、空输入及输入不变；断言宏内模板/初始化列表逗号需正确处理。",
    },
    {
        "id": "cpp_quota",
        "language": "cpp",
        "source_file": "subject.cpp",
        "needle": "if (amount",
        "files": {
            "subject.hpp": """#pragma once
class Quota {
public:
    explicit Quota(int limit) : limit_(limit < 0 ? 0 : limit), used_(0) {}
    // Accept nonnegative amount iff it fits remaining quota, including exact fit
    // and zero. On success add to used. On rejection leave all state unchanged.
    bool consume(int amount);
    int used() const { return used_; }
    int remaining() const { return limit_ - used_; }
private:
    int limit_;
    int used_;
};
""",
            "subject.cpp": """#include "subject.hpp"
bool Quota::consume(int amount) {
    if (amount < 0 || amount > limit_ - used_) return false;
    used_ += amount;
    return true;
}
""",
        },
        "gold": """#include <gtest/gtest.h>
#include "subject.hpp"
TEST(__SUITE__, Contract) {
    Quota q(10);
    EXPECT_TRUE(q.consume(4)); EXPECT_EQ(q.used(), 4); EXPECT_EQ(q.remaining(), 6);
    EXPECT_FALSE(q.consume(7)); EXPECT_EQ(q.used(), 4); EXPECT_EQ(q.remaining(), 6);
    EXPECT_FALSE(q.consume(-1)); EXPECT_EQ(q.used(), 4);
    EXPECT_TRUE(q.consume(6)); EXPECT_EQ(q.used(), 10); EXPECT_EQ(q.remaining(), 0);
    EXPECT_TRUE(q.consume(0)); EXPECT_FALSE(q.consume(1)); EXPECT_EQ(q.used(), 10);
    Quota zero(-5); EXPECT_TRUE(zero.consume(0)); EXPECT_FALSE(zero.consume(1));
}
""",
        "mutants": [
            {"id": "exact_fit", "old": "amount > limit_ - used_", "new": "amount >= limit_ - used_", "meaning": "错误拒绝刚好用完额度/剩余额度为0时的零请求"},
            {"id": "negative", "old": "amount < 0", "new": "false", "meaning": "错误接受负额度"},
            {"id": "rejection_state", "old": "if (amount < 0 || amount > limit_ - used_) return false;", "new": "if (amount < 0 || amount > limit_ - used_) { used_ = 0; return false; }", "meaning": "拒绝时重置已用额度"},
        ],
        "review": "检查构造/连续消费、精确额度、负数、失败前后状态不变、零请求，不能访问private字段。",
    },
    {
        "id": "python_parse_port",
        "language": "python",
        "source_file": "subject.py",
        "needle": "value = text",
        "files": {
            "subject.py": """def parse_port(text: str) -> int:
    \"\"\"Strip whitespace, then parse ASCII decimal digits in range 1..65535.

    Leading zeros are accepted. Empty, signed, non-ASCII/nondecimal and
    out-of-range values raise ValueError. No network operation is performed.
    \"\"\"
    value = text.strip()
    if not value or not value.isascii() or not value.isdecimal():
        raise ValueError('expected ASCII digits')
    port = int(value)
    if not 1 <= port <= 65535:
        raise ValueError('port out of range')
    return port
""",
        },
        "gold": """import pytest
from subject import parse_port

def test_contract():
    assert parse_port('1') == 1
    assert parse_port('65535') == 65535
    assert parse_port(' 00080\\t') == 80
    for value in ['', '+1', '-1', '0', '65536', '１２', 'abc']:
        with pytest.raises(ValueError):
            parse_port(value)
""",
        "mutants": [
            {"id": "upper_endpoint", "old": "1 <= port <= 65535", "new": "1 <= port < 65535", "meaning": "错误拒绝65535"},
            {"id": "ascii_only", "old": "not value or not value.isascii() or not value.isdecimal()", "new": "not value or not value.isdecimal()", "meaning": "错误接受全角等Unicode数字"},
            {"id": "whitespace", "old": "value = text.strip()", "new": "value = text", "meaning": "不再接受首尾空白"},
        ],
        "review": "检查上下边界、首尾空白、前导零、Unicode数字/符号拒绝及正确ValueError；不依赖具体错误消息。",
    },
    {
        "id": "python_unique_by",
        "language": "python",
        "source_file": "subject.py",
        "needle": "seen = set()",
        "files": {
            "subject.py": """def unique_by(items, key):
    \"\"\"Return the first item for each distinct hashable key, preserving order.

    Do not mutate items. Call key exactly once per item, including duplicates.
    Propagate exceptions raised by key. The returned value is always a list.
    \"\"\"
    seen = set()
    result = []
    for item in items:
        marker = key(item)
        if marker not in seen:
            seen.add(marker)
            result.append(item)
    return result
""",
        },
        "gold": """import pytest
from subject import unique_by

def test_contract():
    values = ['a', 'A', 'b', 'B', 'c']
    before = list(values)
    calls = []
    def key(value):
        calls.append(value)
        return value.lower()
    assert unique_by(values, key) == ['a', 'b', 'c']
    assert values == before
    assert calls == values
    assert unique_by([], key) == []
    def broken(value):
        raise LookupError('sentinel')
    with pytest.raises(LookupError):
        unique_by([1], broken)
""",
        "mutants": [
            {"id": "duplicates", "old": "if marker not in seen:", "new": "if True:", "meaning": "不再去重"},
            {"id": "order", "old": "result.append(item)", "new": "result.insert(0, item)", "meaning": "逆序返回结果"},
            {"id": "key_calls", "old": "seen.add(marker)", "new": "seen.add(key(item))", "meaning": "对首次出现的元素重复调用key"},
        ],
        "review": "检查第一项保留、稳定顺序、输入不变、key调用次数（含重复项）、异常传播和空输入。",
    },
]


def materialize() -> list[dict]:
    """Add standard project files and reject malformed mutant declarations."""
    import copy

    cases = copy.deepcopy(CASES)
    for case in cases:
        if case["language"] == "python":
            case["files"]["pyproject.toml"] = "[tool.pytest.ini_options]\ntestpaths = ['tests']\n"
        else:
            case["files"]["CMakeLists.txt"] = (
                "cmake_minimum_required(VERSION 3.20)\nproject(ai_test_eval C CXX)\n"
                f"add_library(subject {case['source_file']})\n"
            )
        original = case["files"][case["source_file"]]
        for mutant in case["mutants"]:
            assert original.count(mutant["old"]) == 1, (case["id"], mutant["id"])
    return cases
