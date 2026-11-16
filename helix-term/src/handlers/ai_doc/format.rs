use super::target::Target;
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Description {
    name: String,
    description: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    field: String,
    quote: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Documentation {
    summary: String,
    #[serde(default)]
    details: Option<String>,
    params: Vec<Description>,
    #[serde(default)]
    templates: Vec<Description>,
    #[serde(default)]
    returns: Option<String>,
    #[serde(default)]
    raises: Vec<Description>,
    #[serde(default)]
    evidence: Vec<Evidence>,
}

fn grounded(target: &Target, doc: &Documentation, context: &[&str]) -> Result<()> {
    let mut required = std::collections::BTreeSet::from(["summary".to_owned()]);
    if doc.details.is_some() {
        required.insert("details".into());
    }
    if doc.returns.is_some() {
        required.insert("returns".into());
    }
    for (prefix, descriptions) in [
        ("param", &doc.params),
        ("template", &doc.templates),
        ("raises", &doc.raises),
    ] {
        for description in descriptions {
            required.insert(format!("{prefix}:{}", description.name));
        }
    }
    ensure!(doc.evidence.len() <= 128, "evidence 证据条目过多");
    let mut covered = std::collections::HashSet::new();
    for evidence in &doc.evidence {
        ensure!(
            required.contains(&evidence.field),
            "evidence 引用了不存在的说明字段：{}",
            evidence.field
        );
        ensure!(
            !evidence.quote.trim().is_empty()
                && evidence.quote.len() <= 2048
                && (target.source.contains(&evidence.quote)
                    || context.iter().any(|s| s.contains(&evidence.quote))),
            "evidence 的引用必须是本次提供源码中的原文片段：{}",
            evidence.field
        );
        covered.insert(evidence.field.as_str());
    }
    for field in required {
        ensure!(
            covered.contains(field.as_str()),
            "evidence 缺少说明字段的源码依据：{field}"
        );
    }
    ensure!(
        !(target.facts.value_return || target.facts.generator)
            || !target.returns
            || doc.returns.is_some(),
        "有返回值或生成值的函数缺少 returns 说明"
    );
    Ok(())
}

fn plain(value: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= 4096,
        "模型返回了空白或过长的说明"
    );
    ensure!(
        !value.chars().any(|c| c.is_control()) && !value.contains("*/"),
        "说明包含不安全的换行或注释定界符"
    );
    ensure!(
        !(value.trim_start().starts_with(":") || value.trim_start().starts_with(".. ")),
        "说明必须是纯文本，不能包含 reST 字段或指令"
    );
    Ok(())
}

fn descriptions(values: Vec<Description>, names: &[&str]) -> Result<HashMap<String, String>> {
    ensure!(values.len() == names.len(), "模型返回的参数数量与签名不符");
    let mut result = HashMap::new();
    for value in values {
        ensure!(
            names.contains(&value.name.as_str()),
            "模型返回了签名中不存在的参数"
        );
        plain(&value.description)?;
        ensure!(
            result.insert(value.name, value.description).is_none(),
            "模型返回了重复参数"
        );
    }
    Ok(result)
}

#[cfg(test)]
pub(crate) fn render(target: &Target, json: &str) -> Result<String> {
    render_with_context(target, json, &[])
}

pub(crate) fn render_with_context(target: &Target, json: &str, context: &[&str]) -> Result<String> {
    ensure!(json.len() <= 32768, "模型返回的注释过长");
    let doc: Documentation =
        serde_json::from_str(json).context("模型未返回有效的文档 JSON，请重新生成")?;
    grounded(target, &doc, context)?;
    plain(&doc.summary)?;
    let params = descriptions(
        doc.params,
        &target
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
    )?;
    let templates = descriptions(
        doc.templates,
        &target
            .templates
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    )?;
    let mut lines = vec![doc.summary];
    if let Some(details) = doc.details {
        plain(&details)?;
        lines.extend([String::new(), details]);
    }
    let mut fields = Vec::new();
    for name in &target.templates {
        fields.push(format!(":tparam {name}: {}", templates[name]));
    }
    for param in &target.parameters {
        fields.push(format!(":param {}: {}", param.name, params[&param.name]));
        if let Some(ty) = &param.ty {
            fields.push(format!(":type {}: {ty}", param.name));
        }
    }
    if let Some(returns) = doc.returns {
        plain(&returns)?;
        ensure!(target.returns, "模型为无返回值函数生成了返回说明");
        fields.push(format!(":returns: {returns}"));
    }
    if target.language == "python" {
        if let Some(ty) = target
            .return_type
            .as_ref()
            .filter(|ty| !(target.returns && ty.as_str() == "None"))
        {
            fields.push(format!(":rtype: {ty}"));
        }
    }
    let mut exceptions = std::collections::HashSet::new();
    for error in doc.raises {
        // Source-derived C++ names may contain string literals in template arguments.
        plain(&error.name)?;
        ensure!(
            target.exceptions.contains(&error.name) && exceptions.insert(error.name.clone()),
            "模型返回了无依据或重复的异常类型"
        );
        plain(&error.description)?;
        let field = if target.language == "python" {
            "raises"
        } else {
            "throws"
        };
        fields.push(format!(":{field} {}: {}", error.name, error.description));
    }
    if !fields.is_empty() {
        lines.push(String::new());
        lines.extend(fields);
    }
    ensure!(
        target.language == "python" || lines.iter().all(|line| !line.contains("*/")),
        "注释正文包含块注释结束符"
    );
    let mut result = String::new();
    let nl = &target.newline;
    let indent = &target.indent;
    if target.language == "python" {
        for (index, line) in lines.iter().enumerate() {
            let escaped = line.replace('\\', "\\\\").replace('"', "\\\"");
            if !line.is_empty() {
                result.push_str(indent);
            }
            if index == 0 {
                result.push_str("\"\"\"");
            }
            result.push_str(&escaped);
            result.push_str(nl);
        }
        result.push_str(&format!("{indent}\"\"\"{nl}"));
    } else {
        result.push_str(&format!("{indent}/*{nl}"));
        for line in &lines {
            result.push_str(indent);
            result.push_str(" *");
            if !line.is_empty() {
                result.push(' ');
                result.push_str(line);
            }
            result.push_str(nl);
        }
        result.push_str(&format!("{indent} */{nl}"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
