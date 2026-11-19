use super::*;
use helix_view::{doc, editor::Action};

use super::send_keys_and_wait as keys;

#[tokio::test(flavor = "multi_thread")]
async fn confirmation_keeps_input_visible_for_long_paths() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let parent = directory.path().join("a-long-directory-name-".repeat(6));
    std::fs::create_dir(&parent)?;
    let source = parent.join("target.txt");
    std::fs::write(&source, "original\n")?;
    let mut app = AppBuilder::new().with_file(&source, None).build()?;
    app.test_resize(80, 24).await?;
    keys(&mut app, ":delete-file!<ret>").await?;
    assert!(!app.editor.is_err());
    for width in [80, 40] {
        app.test_resize(width, 24).await?;
        let buffer = app.test_screen();
        let rows: Vec<String> = buffer
            .content
            .chunks(width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol.as_str()).collect())
            .collect();
        assert!(
            rows.iter().any(|row| row.contains("Delete file? Type yes")),
            "{rows:?}"
        );
        // The temporary directory length can make the basename wrap. Check
        // the path actually displayed inside the bordered help text.
        let help: String = rows
            .iter()
            .filter_map(|row| {
                row.trim()
                    .strip_prefix('│')?
                    .strip_suffix('│')
                    .map(str::trim)
            })
            .collect();
        assert!(help.contains(source.to_str().unwrap()), "{rows:?}");
    }
    assert!(source.exists());
    test_key_sequence(
        &mut app,
        Some("yes<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err());
            assert!(!source.exists());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn confirmed_deletion_closes_buffer_without_exiting() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("删除 文件.txt");
    std::fs::write(&source, "original\n")?;
    let mut app = AppBuilder::new().with_file(&source, None).build()?;
    let id = doc!(app.editor).id();
    test_key_sequences(
        &mut app,
        vec![
            (
                Some(":delete-file<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err());
                    assert!(source.exists());
                    assert!(app.editor.document(id).is_some());
                }),
            ),
            (
                Some("yes<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err());
                    assert!(!source.exists());
                    assert!(app.editor.document(id).is_none());
                    assert!(doc!(app.editor).path().is_none());
                    assert_eq!(app.editor.tree.views().count(), 1);
                }),
            ),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_and_nonaffirmative_answers_preserve_file() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("keep.txt");
    std::fs::write(&source, "original\n")?;
    for answer in ["<esc>", "<ret>", "no<ret>", "y<ret>", "yes<esc>"] {
        let mut app = AppBuilder::new().with_file(&source, None).build()?;
        keys(&mut app, ":delete-file<ret>").await?;
        assert!(!app.editor.is_err());
        test_key_sequence(
            &mut app,
            Some(answer),
            Some(&|app| {
                assert!(!app.editor.is_err());
                assert_eq!(std::fs::read_to_string(&source).unwrap(), "original\n");
                assert!(doc!(app.editor).path().is_some());
            }),
            false,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn unsaved_changes_require_force_and_force_still_requires_confirmation() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("dirty.txt");
    std::fs::write(&source, "original\n")?;
    let mut app = AppBuilder::new().with_file(&source, None).build()?;
    keys(&mut app, "iedit<esc>:delete-file<ret>").await?;
    assert!(app.editor.is_err());
    assert!(doc!(app.editor).is_modified());
    assert!(doc!(app.editor).text().to_string().starts_with("edit"));
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "original\n");
    keys(&mut app, ":delete-file!<ret>").await?;
    assert!(!app.editor.is_err());
    keys(&mut app, "<esc>").await?;
    assert!(!app.editor.is_err());
    assert!(source.exists());
    assert!(doc!(app.editor).is_modified());
    keys(&mut app, ":delete-file!<ret>").await?;
    test_key_sequence(
        &mut app,
        Some("yes<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err());
            assert!(!source.exists());
            assert!(doc!(app.editor).path().is_none());
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn scratch_and_missing_files_are_rejected() -> anyhow::Result<()> {
    for command in [":delete-file<ret>", ":delete-file!<ret>"] {
        test_key_sequence(
            &mut AppBuilder::new().with_input_text("#[keep|]#").build()?,
            Some(command),
            Some(&|app| {
                assert!(app.editor.is_err());
                assert_eq!(doc!(app.editor).text().to_string().trim_end(), "keep");
            }),
            false,
        )
        .await?;
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("missing.txt");
        test_key_sequence(
            &mut AppBuilder::new().with_file(&source, None).build()?,
            Some(command),
            Some(&|app| {
                assert!(app.editor.is_err());
                assert!(doc!(app.editor).path().is_some());
                assert!(!source.exists());
            }),
            false,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn deletion_returns_to_other_buffer_and_removes_all_views_of_deleted_document(
) -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let previous = directory.path().join("previous.txt");
    let source = directory.path().join("delete.txt");
    std::fs::write(&previous, "keep\n")?;
    std::fs::write(&source, "delete\n")?;
    let mut app = AppBuilder::new().with_file(&previous, None).build()?;
    let previous_id = doc!(app.editor).id();
    let deleted_id = app.editor.open(&source, Action::Replace)?;
    keys(&mut app, ":vsplit<ret>:delete-file<ret>").await?;
    assert!(!app.editor.is_err());
    test_key_sequences(
        &mut app,
        vec![
            (
                Some("yes<ret>"),
                Some(&|app| {
                    assert!(!app.editor.is_err());
                    assert!(!source.exists());
                    assert_eq!(std::fs::read_to_string(&previous).unwrap(), "keep\n");
                    assert!(app.editor.document(deleted_id).is_none());
                    assert_eq!(doc!(app.editor).id(), previous_id);
                    assert!(app
                        .editor
                        .tree
                        .views()
                        .all(|(view, _)| view.doc != deleted_id));
                }),
            ),
            (Some("<space>wo"), None),
        ],
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn deletion_flushes_pending_save_without_recreating_file() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("pending.txt");
    std::fs::write(&source, "original\n")?;
    let mut app = AppBuilder::new().with_file(&source, None).build()?;
    keys(&mut app, "iedit<esc>").await?;
    let id = doc!(app.editor).id();
    app.editor.save(id, None::<std::path::PathBuf>, false)?;
    keys(&mut app, ":delete-file<ret>").await?;
    assert!(!app.editor.is_err());
    test_key_sequence(
        &mut app,
        Some("yes<ret>"),
        Some(&|app| {
            assert!(!app.editor.is_err());
            assert!(!source.exists());
            assert!(app.editor.document(id).is_none());
        }),
        false,
    )
    .await?;
    app.editor.flush_writes().await?;
    assert!(!source.exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_or_directory_target_at_confirmation_preserves_buffer() -> anyhow::Result<()> {
    for replace_with_directory in [false, true] {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("changed.txt");
        std::fs::write(&source, "original\n")?;
        let mut app = AppBuilder::new().with_file(&source, None).build()?;
        let id = doc!(app.editor).id();
        keys(&mut app, ":delete-file<ret>").await?;
        assert!(!app.editor.is_err());
        std::fs::remove_file(&source)?;
        if replace_with_directory {
            std::fs::create_dir(&source)?;
        }
        test_key_sequence(
            &mut app,
            Some("yes<ret>"),
            Some(&|app| {
                assert!(app.editor.is_err());
                assert_eq!(doc!(app.editor).id(), id);
                assert_eq!(doc!(app.editor).text().to_string(), "original\n");
                assert_eq!(source.is_dir(), replace_with_directory);
            }),
            false,
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn path_arguments_are_rejected_without_deleting_any_file() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("keep.txt");
    std::fs::write(&source, "original\n")?;
    for command in [
        ":delete-file other.txt<ret>",
        ":delete-file! other.txt<ret>",
    ] {
        test_key_sequence(
            &mut AppBuilder::new().with_file(&source, None).build()?,
            Some(command),
            Some(&|app| {
                assert!(app.editor.is_err());
                assert_eq!(std::fs::read_to_string(&source).unwrap(), "original\n");
            }),
            false,
        )
        .await?;
    }
    Ok(())
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn failed_disk_deletion_preserves_unsaved_buffer() -> anyhow::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("locked.txt");
    std::fs::write(&source, "original\n")?;
    let mut app = AppBuilder::new().with_file(&source, None).build()?;
    let id = doc!(app.editor).id();
    let _lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&source)?;
    keys(&mut app, "iedit<esc>:delete-file!<ret>").await?;
    assert!(!app.editor.is_err());
    test_key_sequence(
        &mut app,
        Some("yes<ret>"),
        Some(&|app| {
            assert!(app.editor.is_err());
            assert_eq!(doc!(app.editor).id(), id);
            assert!(doc!(app.editor).is_modified());
            assert!(doc!(app.editor).text().to_string().starts_with("edit"));
            assert_eq!(std::fs::read_to_string(&source).unwrap(), "original\n");
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn changes_received_during_confirmation_are_protected() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source = directory.path().join("changed.txt");
    std::fs::write(&source, "original\n")?;
    let mut app = AppBuilder::new().with_file(&source, None).build()?;
    let id = doc!(app.editor).id();
    keys(&mut app, ":delete-file<ret>").await?;
    assert!(!app.editor.is_err());
    {
        let (view, doc) = helix_view::current!(app.editor);
        let transaction = helix_core::Transaction::insert(
            doc.text(),
            &helix_core::Selection::point(0),
            "edit".into(),
        );
        assert!(doc.apply(&transaction, view.id));
    }
    test_key_sequence(
        &mut app,
        Some("yes<ret>"),
        Some(&|app| {
            assert!(app.editor.is_err());
            assert_eq!(doc!(app.editor).id(), id);
            assert!(doc!(app.editor).text().to_string().starts_with("edit"));
            assert_eq!(std::fs::read_to_string(&source).unwrap(), "original\n");
        }),
        false,
    )
    .await
}
