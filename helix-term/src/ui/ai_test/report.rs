use crate::ui::ai_preview_text::review::{ReviewLines, Role};
use helix_view::ai_test::{RunReport, RunStage};
use std::collections::HashSet;

fn pytest_counts(line: &str) -> Option<Vec<(Role, String)>> {
    let summary = line.trim_matches(|c: char| c == '=' || c.is_whitespace());
    let (counts, duration) = summary.split_once(" in ")?;
    let seconds = duration
        .split_whitespace()
        .next()?
        .strip_suffix('s')?
        .parse::<f64>()
        .ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let mut parts = Vec::new();
    for pair in counts.split(", ") {
        let (amount, state) = pair.split_once(' ')?;
        let n = amount.parse::<u32>().ok()?;
        let label = match state {
            "passed" => "通过",
            "failed" => "失败",
            "error" | "errors" => "错误",
            "skipped" => "跳过",
            "xfailed" => "预期失败",
            "xpassed" => "意外通过",
            "deselected" => "未选中",
            "warning" | "warnings" => "警告",
            _ => return None,
        };
        let role = match state {
            "failed" | "error" | "errors" if n > 0 => Role::Failure,
            "passed" => Role::Success,
            _ => Role::Note,
        };
        parts.push((role, format!("{n} 项{label}")));
    }
    parts.sort_by_key(|(role, _)| match role {
        Role::Failure => 0,
        Role::Success => 1,
        _ => 2,
    });
    Some(parts)
}

fn ctest_counts(line: &str) -> Option<Vec<(Role, String)>> {
    let (percent, counts) = line.trim().split_once("% tests passed, ")?;
    let (failed, total) = counts.split_once(" tests failed out of ")?;
    let failed = failed.parse::<u32>().ok()?;
    let total = total.parse::<u32>().ok()?;
    if percent.parse::<u8>().ok()? > 100 || failed > total {
        return None;
    }
    let mut rows = Vec::new();
    if failed > 0 {
        rows.push((Role::Failure, format!("{failed} 项失败")));
    }
    rows.push((Role::Note, format!("共 {total} 项")));
    Some(rows)
}

fn assertion_pair<'a>(a: &'a str, b: &'a str) -> Option<(&'a str, &'a str)> {
    fn label<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
        let line = s.trim();
        line.strip_prefix("E ")
            .unwrap_or(line)
            .trim_start()
            .strip_prefix(tag)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
    let actual = |s| label(s, "Actual:").or_else(|| label(s, "Obtained:"));
    label(a, "Expected:")
        .zip(actual(b))
        .or_else(|| label(b, "Expected:").zip(actual(a)))
}

pub(super) fn summary(report: &RunReport, out: &mut ReviewLines) {
    let stage = match report.stage {
        RunStage::Configure => "配置",
        RunStage::Build => "构建",
        RunStage::Test => "测试",
        RunStage::Environment => "运行环境",
    };
    let status = if report.success { "通过" } else { "失败" };
    let role = if report.success {
        Role::Success
    } else {
        Role::Failure
    };
    out.push(role, &format!("{stage}{status}"));
    let lines: Vec<_> = report.log.lines().collect();
    if report.stage == RunStage::Test {
        if let Some(counts) = lines
            .iter()
            .rev()
            .find_map(|s| pytest_counts(s).or_else(|| ctest_counts(s)))
        {
            for (role, count) in counts {
                out.push(role, &count);
            }
        }
    }
    if report.success {
        return;
    }

    let mut seen = HashSet::new();
    for line in &lines {
        let line = line.trim();
        let test = line
            .strip_prefix("FAILED ")
            .filter(|test| test.contains("::"))
            .map(|test| test.split_once(" - ").map_or(test, |(id, _)| id))
            .or_else(|| {
                line.strip_prefix("[  FAILED  ] ")
                    .and_then(|s| s.split_whitespace().next())
                    .filter(|s| s.contains('.'))
            });
        if let Some(test) = test {
            if seen.insert(test) {
                out.push(Role::Failure, &format!("× {test}"));
            }
        }
    }
    if let Some((expected, actual)) = lines
        .windows(2)
        .find_map(|pair| assertion_pair(pair[0], pair[1]))
    {
        out.push(Role::Text, &format!("期望   {expected}"));
        out.push(Role::Text, &format!("实际   {actual}"));
        return;
    }
    if let Some(i) = lines
        .iter()
        .position(|s| s.trim() == "Expected equality of these values:")
    {
        out.push(Role::Heading, "相等断言");
        for line in lines
            .iter()
            .skip(i + 1)
            .take_while(|s| s.starts_with(char::is_whitespace) && !s.trim().is_empty())
            .take(6)
        {
            out.push(Role::Text, line);
        }
        return;
    }
    let diagnostic = lines
        .iter()
        .copied()
        .find(|s| {
            let line = s.trim_start();
            line.starts_with("E ")
                || line.contains("error:")
                || line.contains("error C")
                || line.starts_with("CMake Error")
                || line.starts_with("ERROR ")
                || line.contains("无法")
                || line.contains("超时")
        })
        .or_else(|| lines.iter().rev().copied().find(|s| !s.trim().is_empty()));
    if let Some(diagnostic) = diagnostic {
        out.push(Role::Text, diagnostic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(log: &str) -> String {
        let report = RunReport {
            stage: RunStage::Test,
            success: false,
            repairable: false,
            log: log.into(),
        };
        let mut lines = ReviewLines::default();
        summary(&report, &mut lines);
        lines.source
    }

    #[test]
    fn reports_only_observed_counts_and_explicit_comparison_labels() {
        let text = failure("Actual: 5\nExpected: 0\nFAILED test_x.py::test_wrap - AssertionError\n1 failed, 2 passed in 0.03s");
        for part in [
            "1 项失败",
            "2 项通过",
            "期望   0",
            "实际   5",
            "test_x.py::test_wrap",
        ] {
            assert!(text.contains(part), "{text}");
        }
        let text = failure("Expected equality of these values:\n  front_index()\n    Which is: 5\n  0\n[  FAILED  ] ArrayQueue.Wrap\n50% tests passed, 1 tests failed out of 2");
        assert!(
            text.contains("ArrayQueue.Wrap")
                && text.contains("Which is: 5")
                && text.contains("共 2 项")
        );
        assert!(!text.contains("期望") && !text.contains("实际"));
        let text = failure("E       assert 5 == 0");
        assert!(text.contains("assert 5 == 0"));
        assert!(!text.contains("项失败") && !text.contains("期望"));
        let text = failure("E       Obtained: 1.2\nE       Expected: 2.4 ± 0.1");
        assert!(text.contains("实际   1.2") && text.contains("期望   2.4 ± 0.1"));
    }
}
