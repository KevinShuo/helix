use super::*;
use helix_stdx::path;
use helix_view::{doc, document::Mode};

#[tokio::test(flavor = "multi_thread")]
async fn shortcut_opens_sibling_buffer_without_writing() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("original.txt");
    let target = directory.path().join("新建 文件.hpp");
    test_key_sequence(
        &mut AppBuilder::new().with_file(&source, None).build()?,
        Some("<space>n新建 文件.hpp<ret>"),
        Some(&|app| {
            assert_eq!(
                doc!(app.editor).path(),
                Some(path::normalize(&target).as_path())
            );
            assert_eq!(app.editor.mode(), Mode::Insert);
            assert_eq!(app.editor.documents().count(), 2);
            assert!(app
                .editor
                .document_by_path(path::normalize(&source))
                .is_some());
            assert_eq!(doc!(app.editor).language_name(), Some("cpp"));
            assert!(!target.exists());
            assert!(!app.editor.is_err());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn shortcut_writes_only_when_saved() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("original.txt");
    std::fs::write(&source, "original")?;
    let target = directory.path().join("sibling.txt");
    test_key_sequences(
        &mut AppBuilder::new().with_file(&source, None).build()?,
        vec![
            (
                Some("<space>nsibling.txt<ret>hello<esc>"),
                Some(&|app| {
                    assert_eq!(doc!(app.editor).text().to_string().trim_end(), "hello");
                    assert!(!target.exists());
                }),
            ),
            (
                Some(":w<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err());
                    assert_eq!(
                        std::fs::read_to_string(&target).unwrap().trim_end(),
                        "hello"
                    );
                    assert_eq!(std::fs::read_to_string(&source).unwrap(), "original");
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_preserves_original_buffer() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("original.txt");
    let target = directory.path().join("cancelled.txt");
    test_key_sequence(
        &mut AppBuilder::new().with_file(&source, None).build()?,
        Some("<space>ncancelled.txt<esc>"),
        Some(&|app| {
            assert_eq!(
                doc!(app.editor).path(),
                Some(path::normalize(&source).as_path())
            );
            assert_eq!(app.editor.mode(), Mode::Normal);
            assert_eq!(app.editor.documents().count(), 1);
            assert!(!source.exists());
            assert!(!target.exists());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_empty_names_and_directory_targets() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("original.txt");
    for name in ["", "   ", ".", "missing/"] {
        test_key_sequence(
            &mut AppBuilder::new().with_file(&source, None).build()?,
            Some(&format!("<space>n{name}<ret>")),
            Some(&|app| {
                assert!(app.editor.is_err(), "should reject {name:?}");
                assert_eq!(
                    doc!(app.editor).path(),
                    Some(path::normalize(&source).as_path())
                );
                assert_eq!(app.editor.mode(), Mode::Normal);
                assert_eq!(app.editor.documents().count(), 1);
                assert!(!directory.path().join("missing").exists());
            }),
            false,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_existing_file_without_switching_or_overwriting() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("original.txt");
    let target = directory.path().join("existing.txt");
    std::fs::write(&target, "keep this")?;
    for keys in ["<space>nexisting.txt<ret>", ":new-file existing.txt<ret>"] {
        test_key_sequence(
            &mut AppBuilder::new().with_file(&source, None).build()?,
            Some(keys),
            Some(&|app| {
                assert!(app.editor.is_err());
                assert_eq!(
                    doc!(app.editor).path(),
                    Some(path::normalize(&source).as_path())
                );
                assert_eq!(app.editor.documents().count(), 1);
                assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep this");
            }),
            false,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_existing_unsaved_buffer() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("original.txt");
    let target = directory.path().join("unsaved.txt");
    test_key_sequence(
        &mut AppBuilder::new().with_file(&source, None).build()?,
        Some("<space>nunsaved.txt<ret>keep this<esc>:new-file ./unsaved.txt<ret>"),
        Some(&|app| {
            assert!(app.editor.is_err());
            assert_eq!(
                doc!(app.editor).path(),
                Some(path::normalize(&target).as_path())
            );
            assert_eq!(doc!(app.editor).text().to_string().trim_end(), "keep this");
            assert_eq!(app.editor.mode(), Mode::Normal);
            assert_eq!(app.editor.documents().count(), 2);
            assert!(!target.exists());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn repeat_in_new_file_does_not_replay_previous_buffer_insertion() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("original.txt");
    for keys in [
        "iold<esc>:new<ret>inew<esc>.",
        "iold<esc><space>nnew.txt<ret>new<esc>.",
        "iold<esc>:new-file new.txt<ret>new<esc>.",
    ] {
        test_key_sequence(
            &mut AppBuilder::new().with_file(&source, None).build()?,
            Some(keys),
            Some(&|app| {
                assert_eq!(doc!(app.editor).text().to_string().trim_end(), "newnew");
                assert_eq!(app.editor.documents().count(), 2);
            }),
            false,
        )
        .await?;
    }
    Ok(())
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn rejects_case_alias_of_unsaved_buffer_on_windows() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("original.txt");
    for (original, alias) in [("Unsaved.txt", "unsaved.txt"), ("École.txt", "école.txt")] {
        test_key_sequence(
            &mut AppBuilder::new().with_file(&source, None).build()?,
            Some(&format!(
                "<space>n{original}<ret>keep this<esc>:new-file {alias}<ret>"
            )),
            Some(&|app| {
                assert!(app.editor.is_err(), "should reject {alias}");
                assert_eq!(app.editor.documents().count(), 2);
                assert_eq!(doc!(app.editor).text().to_string().trim_end(), "keep this");
                assert_eq!(app.editor.mode(), Mode::Normal);
            }),
            false,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn scratch_buffer_uses_working_directory() -> anyhow::Result<()> {
    let unique = tempfile::tempdir()?;
    let name = format!(
        "helix-new-file-{}.txt",
        unique.path().file_name().unwrap().to_string_lossy()
    );
    let target = helix_stdx::env::current_working_dir().join(&name);
    test_key_sequence(
        &mut AppBuilder::new().with_input_text("#[original|]#").build()?,
        Some(&format!("<space>n{name}<ret>")),
        Some(&|app| {
            assert_eq!(
                doc!(app.editor).path(),
                Some(path::normalize(&target).as_path())
            );
            assert_eq!(app.editor.mode(), Mode::Insert);
            assert_eq!(app.editor.documents().count(), 2);
            assert!(!target.exists());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn typed_command_supports_nested_quoted_paths_without_creating_directories(
) -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("original.txt");
    let target = directory.path().join("nested/新建 文件.txt");
    test_key_sequences(
        &mut AppBuilder::new().with_file(&source, None).build()?,
        vec![
            (
                Some(":new-file 'nested/新建 文件.txt'<ret>hello<esc>"),
                Some(&|app| {
                    assert_eq!(
                        doc!(app.editor).path(),
                        Some(path::normalize(&target).as_path())
                    );
                    assert!(!app.editor.is_err());
                    assert!(!directory.path().join("nested").exists());
                }),
            ),
            (
                Some(":w<ret>"),
                Some(&|app| {
                    assert!(app.editor.is_err());
                    assert!(!directory.path().join("nested").exists());
                }),
            ),
            (
                Some(":w!<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err());
                    assert_eq!(
                        std::fs::read_to_string(&target).unwrap().trim_end(),
                        "hello"
                    );
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn new_command_still_opens_scratch_buffer() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().with_input_text("#[original|]#").build()?,
        Some(":new<ret>"),
        Some(&|app| {
            assert!(doc!(app.editor).path().is_none());
            assert_eq!(app.editor.mode(), Mode::Normal);
            assert_eq!(app.editor.documents().count(), 2);
        }),
        false,
    )
    .await
}
