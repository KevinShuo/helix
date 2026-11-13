//! Parse immutable snapshots off the editor thread before showing predictions.

use helix_core::{
    syntax::{Loader, Syntax},
    Language, Rope,
};
use std::{collections::HashSet, ops::RangeInclusive};

pub(super) const MAX_SOURCE_BYTES: usize = 128 * 1024;

fn valid(syntax: &Syntax, changed: RangeInclusive<u32>) -> Option<bool> {
    let mut layers = HashSet::new();
    // Insertion text is capped separately. Inspect every affected byte so a
    // newly introduced script/style injection is checked even when its tags
    // and the cursor itself belong to the surrounding language.
    for point in changed {
        layers.extend(syntax.layers_for_byte_range(point, point));
        if layers.len() > 16 {
            return None;
        }
    }
    let mut pending = Vec::new();
    for layer in layers {
        pending.push(syntax.layer(layer).tree()?.root_node());
    }
    for _ in 0..20_000 {
        let Some(node) = pending.pop() else {
            return Some(true);
        };
        if node.kind() == "ERROR" || node.is_missing() {
            return Some(false);
        }
        // Bound both traversal time and temporary storage for wide trees.
        for child in node.children().take(20_001) {
            if pending.len() >= 20_000 {
                return None;
            }
            pending.push(child);
        }
    }
    None
}

pub(super) fn check(
    mut source: Rope,
    cursor: usize,
    insertion: &str,
    language: Language,
    loader: &Loader,
    predicting: bool,
) -> bool {
    if source.len_bytes() > MAX_SOURCE_BYTES || cursor > source.len_chars() {
        return true;
    }
    let baseline = if predicting {
        None
    } else {
        Syntax::new(source.slice(..), language, loader)
            .ok()
            .and_then(|syntax| {
                valid(
                    &syntax,
                    source.char_to_byte(cursor) as u32..=source.char_to_byte(cursor) as u32,
                )
            })
    };
    let start = source.char_to_byte(cursor) as u32;
    source.insert(cursor, insertion);
    match Syntax::new(source.slice(..), language, loader)
        .ok()
        .and_then(|syntax| valid(&syntax, start..=start + insertion.len() as u32))
    {
        Some(true) => true,
        Some(false) => !predicting && baseline != Some(true),
        None => !predicting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_guard_rejects_new_errors_without_blocking_partial_input() {
        let loader = helix_core::config::default_lang_loader();
        for (language, prefix, suffix, insertion, predicting, expected) in [
            ("python", "def positives(values):\n    result = []\n    # Use an explicit for loop to append strictly positive values to result, then return result.\n    ", "\n", "for value in values:\n        if value > 0:\n            result.append(value)\n    return result", true, true),
            (
                "html",
                "<div>hi</div>\n",
                "\n",
                "<script>function f(){return + * 2;}</script>",
                false,
                false,
            ),
            (
                "html",
                "<script>\nfunction f(x) {\n    return x ",
                ";\n}\n</script>\n",
                "+ * 2",
                false,
                false,
            ),
            ("python", "x = 1 ", "\n", "+ * 2", false, false),
            ("python", "x = 1 ", "\n", "+ 2", false, true),
            ("python", "x = ", "\n", "1", false, true),
            ("python", "def f(x):\n    ", "\n", "return x", true, true),
            (
                "python",
                "def f(x):\n    ",
                "\n",
                "return + * 2",
                true,
                false,
            ),
            ("cpp", "int f(){ return 1 ", "; }\n", "+ / 2", false, false),
            ("cpp", "int f(){ return 1 ", "; }\n", "+ 2", false, true),
        ] {
            let source = Rope::from(format!("{prefix}{suffix}"));
            assert_eq!(
                check(
                    source,
                    prefix.chars().count(),
                    insertion,
                    loader.language_for_name(language).unwrap(),
                    &loader,
                    predicting
                ),
                expected,
                "{language}: {prefix}{insertion}{suffix}"
            );
        }
    }
}
