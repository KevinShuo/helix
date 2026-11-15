//! Opt-in, synthetic live-provider evaluation using production request/transport,
//! output validation, CMake integration and execution. Never loaded in release.
use super::*;
use crate::handlers::ai_test::{project, runner};
use helix_view::ai_test::{RunReport, RunStage};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Deserialize)]
struct Case {
    id: String,
    language: String,
    source_file: String,
    needle: String,
    files: BTreeMap<String, String>,
    gold: String,
    mutants: Vec<Mutant>,
}

#[derive(Deserialize)]
struct Mutant {
    id: String,
    old: String,
    new: String,
    meaning: String,
}

fn apply_evaluation_output_budget(
    body: &mut Value,
    context_size: u32,
    budget: Option<u32>,
) -> Result<()> {
    if let Some(budget) = budget {
        ensure!(
            (512..=32768).contains(&budget),
            "Evaluation output budget must be 512..32768"
        );
        let input_reservation = serde_json::to_vec(&body["messages"])?.len() + 512;
        ensure!(
            input_reservation + budget as usize <= context_size as usize,
            "Evaluation output budget exceeds configured context reservation"
        );
        body["max_tokens"] = json!(budget);
    }
    Ok(())
}

#[test]
fn evaluation_output_budget_preserves_messages_and_reserves_context() {
    let messages = json!([{"role":"user","content":"fixed synthetic target"}]);
    let mut body = json!({"max_tokens":4096,"messages":messages});
    apply_evaluation_output_budget(&mut body, 32768, Some(16384)).unwrap();
    assert_eq!(body["max_tokens"], 16384);
    assert_eq!(body["messages"], messages);
    let before = body.clone();
    assert!(apply_evaluation_output_budget(&mut body, 16384, Some(16384)).is_err());
    assert_eq!(body, before);
}

fn json_file(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_string_pretty(value).unwrap() + "\n").unwrap();
}

fn report(report: &RunReport, elapsed_ms: u128) -> Value {
    json!({"stage":format!("{:?}", report.stage), "success":report.success,
        "repairable":report.repairable,"elapsed_ms":elapsed_ms,"log":report.log})
}

fn target(case: &Case, source: &str) -> Target {
    let text = Rope::from(source);
    let loader = default_lang_loader();
    let syntax = Syntax::new(
        text.slice(..),
        loader.language_for_name(case.language.as_str()).unwrap(),
        &loader,
    )
    .unwrap();
    crate::handlers::ai_doc::target::locate_for_test(
        text.slice(..),
        &syntax,
        source[..source.find(&case.needle).unwrap()].chars().count(),
        &case.language,
    )
    .unwrap()
}

async fn evaluate_case(
    case: &Case,
    arm: &str,
    output: &Path,
    work: &Path,
    model: Option<&AiProviderConfig>,
    config: &AiTestConfig,
) -> Value {
    let dir = work.join(arm).join(&case.id);
    fs::create_dir_all(&dir).unwrap();
    for (name, text) in &case.files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let artifacts = output.join(arm).join(&case.id);
    fs::create_dir_all(&artifacts).unwrap();
    let source = dir.join(&case.source_file);
    let original = &case.files[&case.source_file];
    let target = target(case, original);
    let project = project::discover(&source, &target, config).unwrap();
    let started = Instant::now();
    let generated: Result<Generated> = if let Some(model) = model {
        match request(model, config, &target, &project, None) {
            Err(error) => Err(error),
            Ok(mut body) => {
                let budget = std::env::var("HELIX_AI_TEST_EVAL_OUTPUT_TOKENS")
                    .ok()
                    .map(|v| v.parse::<u32>().expect("evaluation output budget"));
                apply_evaluation_output_budget(&mut body, model.context_size, budget)
                    .expect("evaluation context reservation");
                if let Some(reference) = std::env::var_os("HELIX_AI_TEST_EVAL_REFERENCE") {
                    let reference: Value = serde_json::from_slice(
                        &fs::read(PathBuf::from(reference).join(&case.id).join("request.json"))
                            .expect("reference request"),
                    )
                    .expect("reference JSON");
                    assert_eq!(
                        body["messages"], reference["messages"],
                        "Evaluation messages changed before dispatch"
                    );
                }
                json_file(&artifacts.join("request.json"), &body);
                match crate::handlers::ai::post(model, &body, config.timeout_ms).await {
                    Err(error) => Err(error),
                    Ok(mut response) => {
                        // Retain final output/usage, not reasoning traces.
                        if let Some(message) = response["choices"][0]["message"].as_object_mut() {
                            if let Some(reasoning) = message.remove("reasoning_content") {
                                message.insert(
                                    "reasoning_content_chars".into(),
                                    json!(reasoning.as_str().map(str::len)),
                                );
                            }
                        }
                        json_file(&artifacts.join("response.json"), &response);
                        let choice = &response["choices"][0];
                        let message = &choice["message"];
                        (|| -> Result<Generated> {
                            ensure!(
                                choice["finish_reason"].as_str() == Some("stop"),
                                "Model output incomplete: {}",
                                choice["finish_reason"]
                            );
                            ensure!(
                                message["tool_calls"].as_array().is_none_or(Vec::is_empty)
                                    && message["function_call"].is_null(),
                                "Unexpected tool call"
                            );
                            validate(
                                &case.language,
                                &project.test_filter,
                                message["content"].as_str().unwrap_or(""),
                            )
                        })()
                    }
                }
            }
        }
    } else {
        validate(
            &case.language,
            &project.test_filter,
            &json!({"code":case.gold.replace("__SUITE__", &project.test_filter),"assumptions":[]})
                .to_string(),
        )
    };
    let generation_ms = started.elapsed().as_millis();
    let mut row = json!({"arm":arm,"case":case.id,"language":case.language,"generation_ms":generation_ms,
        "accepted":false,"generation_error":null,"baseline":null,"mutants":[],"assumptions":[],"restored":null});
    let generated = match generated {
        Ok(generated) => generated,
        Err(error) => {
            row["generation_error"] = json!(format!("{error:#}"));
            return row;
        }
    };
    row["accepted"] = json!(true);
    row["assumptions"] = json!(generated.assumptions);
    fs::write(
        artifacts.join(if case.language == "python" {
            "generated.py"
        } else {
            "generated.cpp"
        }),
        &generated.code,
    )
    .unwrap();
    for change in project::changes(&project, &generated.code).unwrap() {
        fs::create_dir_all(change.before.path.parent().unwrap()).unwrap();
        fs::write(change.before.path, change.after).unwrap();
    }
    let started = Instant::now();
    let baseline = runner::run(&project, config).await;
    row["baseline"] = report(&baseline, started.elapsed().as_millis());
    if !baseline.success {
        return row;
    }
    let mut results = Vec::new();
    for mutant in &case.mutants {
        assert_eq!(original.matches(&mutant.old).count(), 1);
        fs::write(&source, original.replacen(&mutant.old, &mutant.new, 1)).unwrap();
        let started = Instant::now();
        let result = runner::run(&project, config).await;
        let killed =
            !result.success && result.stage == RunStage::Test && !result.log.contains("超时");
        results.push(json!({"id":mutant.id,"meaning":mutant.meaning,"killed":killed,
            "viable":result.stage == RunStage::Test,"run":report(&result, started.elapsed().as_millis())}));
    }
    fs::write(&source, original).unwrap();
    let started = Instant::now();
    let restored = runner::run(&project, config).await;
    row["restored"] = report(&restored, started.elapsed().as_millis());
    row["mutants"] = json!(results);
    row
}

#[tokio::test]
#[ignore = "live 27B/DeepSeek evaluation; requires HELIX_AI_TEST_EVAL_*; sends synthetic fixtures only"]
async fn ai_test_quality_evaluation() {
    let arm = std::env::var("HELIX_AI_TEST_EVAL_ARM").unwrap();
    let output = PathBuf::from(std::env::var_os("HELIX_AI_TEST_EVAL_OUTPUT").unwrap());
    let work = PathBuf::from(std::env::var_os("HELIX_AI_TEST_EVAL_WORK").unwrap());
    let cases: Vec<Case> =
        serde_json::from_slice(&fs::read(output.join("cases.json")).unwrap()).unwrap();
    let model: Option<AiProviderConfig> = if arm == "gold" {
        None
    } else {
        Some(
            serde_json::from_slice(&fs::read(output.join(format!("{arm}.provider.json"))).unwrap())
                .unwrap(),
        )
    };
    let config = AiTestConfig {
        enabled: true,
        python: Some(std::env::var_os("HELIX_AI_TEST_PYTHON").unwrap().into()),
        allow_fetch_content: false,
        ..Default::default()
    };
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output.join(format!("{arm}.jsonl")))
        .unwrap();
    for case in cases {
        if std::env::var("HELIX_AI_TEST_EVAL_CASE")
            .ok()
            .is_some_and(|selected| !selected.split(',').any(|id| id == case.id))
        {
            continue;
        }
        let row = evaluate_case(&case, &arm, &output, &work, model.as_ref(), &config).await;
        json_file(&output.join(&arm).join(&case.id).join("result.json"), &row);
        writeln!(file, "{row}").unwrap();
        file.flush().unwrap();
        println!(
            "EVAL {arm}/{}: accepted={} pass={} generation={}ms mutants={}/{}",
            case.id,
            row["accepted"],
            row["baseline"]["success"],
            row["generation_ms"],
            row["mutants"]
                .as_array()
                .map_or(0, |m| m.iter().filter(|m| m["killed"] == true).count()),
            case.mutants.len()
        );
        if arm == "gold" {
            assert_eq!(
                row["baseline"]["success"], true,
                "gold control failed: {row}"
            );
            assert_eq!(
                row["restored"]["success"], true,
                "control restoration failed"
            );
            assert!(
                row["mutants"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|m| m["killed"] == true && m["viable"] == true),
                "invalid mutation controls: {row}"
            );
        }
    }
}
