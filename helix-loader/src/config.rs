use std::str::from_utf8;

use crate::workspace_trust::{TrustQuery, WorkspaceTrust};

/// Default built-in languages.toml.
pub fn default_lang_config() -> toml::Value {
    let default_config = include_bytes!("../../languages.toml");
    toml::from_str(from_utf8(default_config).unwrap())
        .expect("Could not parse built-in languages.toml to valid toml")
}

/// User configured languages.toml file, merged with the default config.
///
/// Workspace-local `.helix/languages.toml` is merged in only when the current
/// workspace is trusted for [`TrustQuery::LocalConfig`].
pub fn user_lang_config(trust: &WorkspaceTrust) -> Result<toml::Value, toml::de::Error> {
    let global_config = crate::lang_config_file();
    let workspace_config = crate::workspace_lang_config_file();

    let files = if trust.query_current(TrustQuery::LocalConfig).is_trusted() {
        vec![global_config, workspace_config]
    } else {
        vec![global_config]
    };

    let mut config = files
        .iter()
        .filter_map(|file| {
            std::fs::read_to_string(file)
                .map(|config| toml::from_str(&config))
                .ok()
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .fold(default_lang_config(), |a, b| {
            crate::merge_toml_values(a, b, 3)
        });

    expand_language_environment(&mut config, &|name| std::env::var(name).ok())?;
    Ok(config)
}

/// Resolve explicit environment references only in process settings, after trusted
/// files have been merged. Parsing TOML first keeps substituted values as data.
fn expand_language_environment(
    config: &mut toml::Value,
    resolve: &impl Fn(&str) -> Option<String>,
) -> Result<(), toml::de::Error> {
    if let Some(servers) = config.get_mut("language-server") {
        expand_environment_value(servers, resolve, false)?;
    }
    if let Some(languages) = config
        .get_mut("language")
        .and_then(toml::Value::as_array_mut)
    {
        for language in languages {
            if let Some(formatter) = language.get_mut("formatter") {
                if let Some(command) = formatter.get_mut("command") {
                    expand_environment_value(command, resolve, false)?;
                }
                if let Some(args) = formatter.get_mut("args") {
                    expand_environment_value(args, resolve, true)?;
                }
            }
        }
    }
    Ok(())
}

fn expand_environment_value(
    value: &mut toml::Value,
    resolve: &impl Fn(&str) -> Option<String>,
    escape_percent: bool,
) -> Result<(), toml::de::Error> {
    match value {
        toml::Value::String(value) if value.contains("${env:") => {
            let mut rest = value.as_str();
            let mut result = String::with_capacity(value.len());
            while let Some(start) = rest.find("${env:") {
                let name_start = start + "${env:".len();
                let end = rest[name_start..].find('}').ok_or_else(|| {
                    <toml::de::Error as serde::de::Error>::custom(
                        "Unclosed environment reference in languages.toml; use ${env:NAME}",
                    )
                })? + name_start;
                if start > 0 && rest.as_bytes()[start - 1] == b'$' {
                    result.push_str(&rest[..start - 1]);
                    result.push_str(&rest[start..=end]);
                } else {
                    let name = &rest[name_start..end];
                    if name.is_empty()
                        || name
                            .chars()
                            .any(|c| c.is_whitespace() || matches!(c, '{' | ':' | '=' | '\0'))
                    {
                        return Err(<toml::de::Error as serde::de::Error>::custom(
                            "Invalid environment reference in languages.toml; use ${env:NAME}",
                        ));
                    }
                    let replacement = resolve(name).ok_or_else(|| {
                        <toml::de::Error as serde::de::Error>::custom(format!(
                            "Missing or non-Unicode environment variable '{name}' in languages.toml"
                        ))
                    })?;
                    result.push_str(&rest[..start]);
                    if escape_percent {
                        // Formatter args subsequently pass through command-mode
                        // expansion. Environment values must remain literal data.
                        result.push_str(&replacement.replace('%', "%%"));
                    } else {
                        result.push_str(&replacement);
                    }
                }
                rest = &rest[end + 1..];
            }
            result.push_str(rest);
            *value = result;
        }
        toml::Value::Array(values) => {
            for value in values {
                expand_environment_value(value, resolve, escape_percent)?;
            }
        }
        toml::Value::Table(table) => {
            for (_, value) in table.iter_mut() {
                expand_environment_value(value, resolve, escape_percent)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_environment_resolves_each_computers_paths() {
        let source = r#"
            [language-server.demo]
            command = "${env:USERPROFILE}/tools/server"
            args = ["${env:LOCALAPPDATA}/server.js", "--stdio"]
            environment = { MODULE_ROOT = "${env:USERPROFILE}/modules" }
            [language-server.demo.config]
            sdk = "${env:LOCALAPPDATA}/typescript/lib"
            plugins = [{ location = "${env:APPDATA}/plugins", enabled = true }]
            [[language]]
            name = "demo"
            file-types = ["${env:NOT_A_PATH}"]
            formatter = { command = "${env:USERPROFILE}/node", args = ["${env:APPDATA}/format.js", "%{buffer_name}"] }
        "#;
        for home in ["C:/Users/Home User", "D:/Users/公司用户"] {
            let mut config: toml::Value = toml::from_str(source).unwrap();
            expand_language_environment(&mut config, &|name| match name {
                "USERPROFILE" => Some(home.to_owned()),
                "LOCALAPPDATA" => Some(format!("{home}/AppData/Local")),
                "APPDATA" => Some(format!("{home}/AppData/Roaming")),
                _ => None,
            })
            .unwrap();
            let server = &config["language-server"]["demo"];
            assert_eq!(
                server["command"].as_str().unwrap(),
                format!("{home}/tools/server")
            );
            assert_eq!(
                server["args"][0].as_str().unwrap(),
                format!("{home}/AppData/Local/server.js")
            );
            assert_eq!(
                server["environment"]["MODULE_ROOT"].as_str().unwrap(),
                format!("{home}/modules")
            );
            assert_eq!(
                server["config"]["sdk"].as_str().unwrap(),
                format!("{home}/AppData/Local/typescript/lib")
            );
            assert_eq!(
                server["config"]["plugins"][0]["location"].as_str().unwrap(),
                format!("{home}/AppData/Roaming/plugins")
            );
            assert_eq!(
                server["config"]["plugins"][0]["enabled"].as_bool(),
                Some(true)
            );
            let language = &config["language"][0];
            assert_eq!(
                language["formatter"]["command"].as_str().unwrap(),
                format!("{home}/node")
            );
            assert_eq!(
                language["formatter"]["args"][0].as_str().unwrap(),
                format!("{home}/AppData/Roaming/format.js")
            );
            assert_eq!(
                language["formatter"]["args"][1].as_str(),
                Some("%{buffer_name}")
            );
            assert_eq!(
                language["file-types"][0].as_str(),
                Some("${env:NOT_A_PATH}")
            );
        }
    }

    #[test]
    fn language_environment_preserves_literals_and_does_not_reparse_values() {
        let mut config: toml::Value = toml::toml! {
            [language-server.demo]
            command = "node"
            args = ["${env:ROOT}/server.js", "${INPUT}", "$0", "%{buffer_name}", "$${env:UNKNOWN}"]
        }
        .into();
        expand_language_environment(&mut config, &|name| {
            assert_eq!(name, "ROOT");
            Some("C:\\Company User\\\"tools\"\\${env:UNKNOWN}".to_owned())
        })
        .unwrap();
        let args = config["language-server"]["demo"]["args"]
            .as_array()
            .unwrap();
        let expected = [
            "C:\\Company User\\\"tools\"\\${env:UNKNOWN}/server.js",
            "${INPUT}",
            "$0",
            "%{buffer_name}",
            "${env:UNKNOWN}",
        ];
        assert_eq!(
            args.iter()
                .map(|arg| arg.as_str().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn language_environment_reports_missing_or_malformed_variables() {
        for reference in [
            "${env:MISSING}",
            "${env:}",
            "${env:BAD NAME}",
            "${env:UNCLOSED",
        ] {
            let mut config: toml::Value = toml::toml! {
                [language-server.demo]
                command = reference
            }
            .into();
            let error = expand_language_environment(&mut config, &|_| None).unwrap_err();
            assert!(error.to_string().contains("environment"), "{error}");
            if reference == "${env:MISSING}" {
                assert!(error.to_string().contains("MISSING"), "{error}");
            }
        }
    }

    #[test]
    fn formatter_environment_values_are_literal_to_runtime_expansion() {
        let mut config: toml::Value = toml::toml! {
            [language-server.demo]
            command = "${env:ROOT}/server"
            [[language]]
            name = "demo"
            formatter = { command = "${env:ROOT}/node", args = ["${env:ROOT}/format.js", "%{buffer_name}"] }
        }.into();
        expand_language_environment(&mut config, &|_| {
            Some("C:/Users/100%/%{buffer_name}/%sh{echo must-not-run}".to_owned())
        })
        .unwrap();
        assert_eq!(
            config["language-server"]["demo"]["command"].as_str(),
            Some("C:/Users/100%/%{buffer_name}/%sh{echo must-not-run}/server")
        );
        let formatter = &config["language"][0]["formatter"];
        assert_eq!(
            formatter["command"].as_str(),
            Some("C:/Users/100%/%{buffer_name}/%sh{echo must-not-run}/node")
        );
        assert_eq!(
            formatter["args"][0].as_str(),
            Some("C:/Users/100%%/%%{buffer_name}/%%sh{echo must-not-run}/format.js")
        );
        assert_eq!(formatter["args"][1].as_str(), Some("%{buffer_name}"));
    }
}
