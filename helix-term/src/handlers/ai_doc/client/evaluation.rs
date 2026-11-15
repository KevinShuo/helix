use super::*;
use crate::handlers::ai_doc::target::tests::find;
use std::time::Instant;
mod support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/quality_eval_support.rs"
    ));
}

#[tokio::test]
#[ignore = "controlled local-model quality evaluation; requires HELIX_AI_EVAL_* environment"]
async fn ai_doc_quality_evaluation() {
    let model = support::model();
    let config = AiDocConfig {
        enabled: true,
        max_tokens: 1024,
        timeout_ms: 120000,
        ..Default::default()
    };
    for (id, language, source, needle, core, edge) in [
        ("doc_mean", "python", "def mean(values: list[float]) -> float:\n    if not values:\n        return 0.0\n    return sum(values) / len(values)\n", "return", "计算输入数值的算术平均值", "空输入返回0，不抛异常"),
        ("doc_port", "python", "def parse_port(text: str) -> int:\n    port = int(text)\n    if not 1 <= port <= 65535:\n        raise ValueError('port out of range')\n    return port\n", "return", "将文本转换为端口整数", "允许1至65535，越界抛ValueError；不声称执行网络操作"),
        ("doc_async", "python", "async def count_after(awaitable) -> int:\n    items = await awaitable\n    return len(items)\n", "return", "等待传入对象并返回所得对象的元素数量", "返回值是等待完成后的计数；不声称网络、超时或重试"),
        ("doc_generator", "python", "def positive(values):\n    for value in values:\n        if value > 0:\n            yield value\n", "yield", "逐个产生输入中的正数", "过滤0和负数并保留相对顺序；不返回列表、不修改输入"),
        ("doc_pointer", "c", "void reset_count(int *value) { if (value) { *value = 0; } }", "*value =", "将指针指向的整数置零", "空指针不操作；这是修改原值的副作用，无返回值、无分配"),
        ("doc_template", "cpp", "template<typename T>\nT clamp_value(T value, T lower, T upper) {\n    if (value < lower) return lower;\n    if (upper < value) return upper;\n    return value;\n}", "return", "将value按上下界截断返回，保留类型T", "低于lower返回lower，高于upper返回upper，其余返回value；不声称自动校验上下界顺序"),
    ] {
        if !support::selected_case(id) { continue; }
        let started = Instant::now();
        let result = match find(language, source, needle) {
            Ok(target) => generate(&model, &config, &target).await.map(Value::String),
            Err(error) => Err(error),
        };
        support::record(&model,"ai-doc",id,json!({"source":source,"language":language,"core":core,"edge":edge}),result,started.elapsed());
    }
}
