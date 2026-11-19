use super::*;
use anyhow::Context as _;
use helix_view::doc;
use std::fs;

async fn keys(app: &mut Application, input: &str) -> anyhow::Result<()> {
    send_keys_and_wait(app, input)
        .await
        .with_context(|| format!("while sending {input}"))
}

#[cfg(any(unix, windows))]
fn link_directory(target: &std::path::Path, link: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link)?;
    #[cfg(windows)]
    {
        let result = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/j"])
            .arg(link)
            .arg(target)
            .output()?;
        anyhow::ensure!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(())
}

#[cfg(any(unix, windows))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_directory_link_preserves_open_child() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::write(dir.path().join("real/work.txt"), "saved")?;
    let link = dir.path().join("link");
    link_directory(&dir.path().join("real"), &link)?;
    let child = link.join("work.txt");
    let mut app = AppBuilder::new().with_file(&child, None).build()?;
    keys(&mut app, "idraft<esc>").await?;
    let id = doc!(app.editor).id();
    app.editor.open(
        &dir.path().join("anchor.txt"),
        helix_view::editor::Action::Replace,
    )?;
    keys(&mut app, "<space>.link").await?;
    keys(&mut app, "<F4>dy").await?;
    assert!(child.is_file());
    assert!(app.editor.document(id).unwrap().is_modified());
    assert!(app.editor.get_status().unwrap().0.contains("open buffer"));
    Ok(())
}

fn screen(app: &Application) -> String {
    let buffer = app.test_screen();
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn move_file_follows_destination_and_previews_selected_file() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let project = dir.path().join("project");
    fs::create_dir(&project)?;
    let file = project.join("notes.txt");
    fs::write(&file, "move preview")?;
    let dest = project.join("archive");
    fs::create_dir(&dest)?;
    fs::write(dest.join("another.txt"), "other preview")?;
    let mut app = AppBuilder::new()
        .with_file(&dir.path().join("anchor.txt"), None)
        .build()?;
    app.test_resize(120, 24).await?;
    keys(&mut app, "<space>.project").await?;
    keys(&mut app, "<A-ret>").await?;
    keys(&mut app, "notes").await?;
    let original = screen(&app);
    keys(&mut app, "<F4>varchive").await?;
    keys(&mut app, "<ret>").await?;
    let target = dest.join("notes.txt");
    assert!(screen(&app).contains(target.to_str().unwrap()));
    keys(&mut app, "<esc>").await?;
    assert_eq!(screen(&app), original);
    keys(&mut app, "<F4>varchive").await?;
    keys(&mut app, "<ret><F4>").await?;
    assert!(!file.exists());
    assert_eq!(fs::read(&target)?, b"move preview");
    assert!(screen(&app).contains("another.txt"));
    assert!(screen(&app).contains("move preview"));
    keys(&mut app, "<ret>").await?;
    assert_eq!(doc!(app.editor).path(), Some(target.as_path()));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn move_directory_preserves_dirty_child_and_allows_retry() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("folder");
    fs::create_dir_all(src.join("child"))?;
    let file = src.join("work.txt");
    fs::write(&file, "saved")?;
    fs::create_dir_all(dir.path().join("occupied/folder"))?;
    fs::create_dir(dir.path().join("archive"))?;
    let mut app = AppBuilder::new().with_file(&file, None).build()?;
    app.test_resize(120, 24).await?;
    keys(&mut app, "idraft<esc><space>.").await?;
    keys(&mut app, "<ret>").await?;
    keys(&mut app, "folder").await?;
    keys(&mut app, "<F4>v<F4>").await?;
    assert!(screen(&app).contains("original directory"));
    for name in ["folder", "child"] {
        keys(&mut app, name).await?;
        keys(&mut app, "<ret><F4>").await?;
        assert!(screen(&app).contains("inside itself"));
    }
    keys(&mut app, "<ret>").await?;
    keys(&mut app, "<ret>").await?;
    keys(&mut app, "occupied").await?;
    keys(&mut app, "<ret><F4>").await?;
    assert!(screen(&app).contains("Already exists"));
    assert!(file.exists());
    keys(&mut app, "<ret>").await?;
    keys(&mut app, "archive").await?;
    keys(&mut app, "<ret><F4>").await?;
    let target = dir.path().join("archive/folder/work.txt");
    assert_eq!(fs::read(&target)?, b"saved");
    assert_eq!(doc!(app.editor).path(), Some(target.as_path()));
    assert!(doc!(app.editor).is_modified());
    keys(&mut app, "<esc>:w<ret>").await?;
    assert!(fs::read_to_string(target)?.contains("draft"));
    assert!(!src.exists());
    Ok(())
}

#[cfg(any(unix, windows))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn move_checks_resolved_destination_and_unsaved_buffer_names() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("folder");
    fs::create_dir_all(src.join("nested"))?;
    fs::write(src.join("work.txt"), "saved")?;
    link_directory(&src.join("nested"), &dir.path().join("alias"))?;
    let dest = dir.path().join("archive");
    fs::create_dir(&dest)?;
    link_directory(&dest, &src.join("outward"))?;
    let reserved = dest.join("folder");
    let mut app = AppBuilder::new()
        .with_file(&dir.path().join("anchor.txt"), None)
        .build()?;
    app.editor
        .open(&reserved, helix_view::editor::Action::Load)?;
    keys(&mut app, "<space>.folder").await?;
    keys(&mut app, "<F4>valias").await?;
    keys(&mut app, "<ret><F4>").await?;
    assert!(screen(&app).contains("inside itself"));
    keys(&mut app, "<ret>").await?;
    keys(&mut app, "folder").await?;
    keys(&mut app, "<ret>").await?;
    keys(&mut app, "outward").await?;
    keys(&mut app, "<ret><F4>").await?;
    assert!(screen(&app).contains("inside itself"));
    keys(&mut app, "<ret>").await?;
    keys(&mut app, "<ret>").await?;
    keys(&mut app, "archive").await?;
    keys(&mut app, "<ret><F4>").await?;
    assert!(screen(&app).contains("An open buffer uses"));
    assert!(src.join("work.txt").is_file());
    assert!(!reserved.exists());
    Ok(())
}

#[cfg(any(unix, windows))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn move_directory_retargets_buffer_opened_through_an_alias() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("folder");
    fs::create_dir(&src)?;
    let alias = dir.path().join("alias");
    link_directory(&src, &alias)?;
    let archive = dir.path().join("archive");
    fs::create_dir(&archive)?;
    link_directory(&archive, &src.join("out"))?;
    link_directory(&src.join("out"), &dir.path().join("shortcut"))?;
    fs::write(src.join("work.txt"), "saved")?;
    let mut app = AppBuilder::new()
        .with_file(&alias.join("work.txt"), None)
        .build()?;
    keys(&mut app, "idraft<esc>").await?;
    let id = doc!(app.editor).id();
    app.editor.open(
        &dir.path().join("anchor.txt"),
        helix_view::editor::Action::Replace,
    )?;
    let duplicate = app
        .editor
        .open(&src.join("work.txt"), helix_view::editor::Action::Load)?;
    keys(&mut app, "<space>.folder").await?;
    keys(&mut app, "<F4>vshortcut").await?;
    keys(&mut app, "<ret><F4>").await?;
    assert!(screen(&app).contains("Multiple open buffers"));
    assert!(src.join("work.txt").is_file());
    assert!(app.editor.close_document(duplicate, false).is_ok());
    keys(&mut app, "<F4>").await?;
    let moved = dir.path().join("archive/folder/work.txt");
    let doc = app.editor.document(id).unwrap();
    assert_eq!(doc.path(), Some(moved.as_path()));
    assert!(doc.is_modified());
    Ok(())
}

#[cfg(any(unix, windows))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires HELIX_TEST_OTHER_VOLUME on a different filesystem from the temp directory"]
async fn move_across_filesystems_preserves_source() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let dest = tempfile::tempdir_in(std::env::var("HELIX_TEST_OTHER_VOLUME")?)?;
    let file = dir.path().join("note.txt");
    fs::write(&file, "saved")?;
    link_directory(dest.path(), &dir.path().join("external"))?;
    let mut app = AppBuilder::new().with_file(&file, None).build()?;
    keys(&mut app, "<space>.note").await?;
    keys(&mut app, "<F4>vexternal").await?;
    keys(&mut app, "<ret>").await?;
    assert!(screen(&app).contains("between filesystems"));
    keys(&mut app, "<F4>").await?;
    assert!(screen(&app).contains("between filesystems"));
    assert_eq!(fs::read(&file)?, b"saved");
    assert!(!dest.path().join("note.txt").exists());
    assert_eq!(doc!(app.editor).path(), Some(file.as_path()));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn menu_and_filter_survive_cancel_and_matching_rename() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("note.txt");
    fs::write(&file, "preview text")?;
    fs::write(dir.path().join("other.txt"), "other")?;
    let mut app = AppBuilder::new().with_file(&file, None).build()?;
    app.test_resize(100, 20).await?;
    keys(&mut app, "<space>.note").await?;
    let initial = screen(&app);
    assert!(
        initial.contains("F4 File actions") && initial.contains("preview text"),
        "{initial}"
    );
    keys(&mut app, "<F4>").await?;
    assert!(screen(&app).contains("m  New directory"));
    keys(&mut app, "rdiscard<esc>").await?;
    assert_eq!(screen(&app), initial);
    keys(&mut app, "<F4>rnotes<ret>").await?;
    let renamed = screen(&app);
    assert!(
        renamed.contains("1/3")
            && renamed.contains("notes.txt")
            && renamed.contains("preview text"),
        "{renamed}"
    );
    keys(&mut app, "<F4>nfresh.txt<ret><ret>").await?;
    assert_eq!(
        doc!(app.editor).path(),
        Some(dir.path().join("fresh.txt").as_path())
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manage_entries_in_the_browsed_directory() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("original.txt");
    fs::write(&file, "keep")?;
    let mut app = AppBuilder::new().with_file(&file, None).build()?;
    keys(&mut app, "<space>.").await?;
    keys(&mut app, "<F4>noriginal.txt<ret>").await?;
    assert_eq!(fs::read_to_string(&file)?, "keep");
    keys(&mut app, "<C-u>new.txt<ret>").await?;
    assert_eq!(fs::read(dir.path().join("new.txt"))?, b"");
    keys(&mut app, "<F4>mfolder<ret>").await?;
    assert!(dir.path().join("folder").is_dir());
    keys(&mut app, "<F4>d<ret>").await?;
    assert!(dir.path().join("folder").is_dir());
    keys(&mut app, "<F4>dy").await?;
    assert!(!dir.path().join("folder").exists());
    keys(&mut app, "original").await?;
    keys(&mut app, "<F4>rdiscard<esc>").await?;
    keys(&mut app, "<F4>rrenamed<ret>").await?;
    let renamed = dir.path().join("renamed.txt");
    assert_eq!(fs::read_to_string(&renamed)?, "keep");
    assert!(!file.exists());
    assert_eq!(doc!(app.editor).path(), Some(renamed.as_path()));
    keys(&mut app, "<ret>ichanged<esc><space>.renamed").await?;
    keys(&mut app, "<F4>dy").await?;
    assert!(renamed.exists());
    assert!(doc!(app.editor).is_modified());
    assert!(app
        .editor
        .get_status()
        .unwrap()
        .0
        .contains("unsaved changes"));
    keys(&mut app, "<esc><esc>:w<ret>").await?;
    keys(&mut app, "<space>.renamed").await?;
    keys(&mut app, "<F4>dy").await?;
    assert!(!renamed.exists());
    assert!(app.editor.document_by_path(&renamed).is_none());
    keys(&mut app, "<ret>").await?;
    assert_eq!(
        doc!(app.editor).path(),
        Some(dir.path().join("new.txt").as_path())
    );
    Ok(())
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn case_only_rename_updates_spelling_on_disk_and_in_buffer() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("report.txt");
    fs::write(&file, "saved")?;
    let mut app = AppBuilder::new().with_file(&file, None).build()?;
    keys(&mut app, "<space>.report").await?;
    keys(&mut app, "<F4>rREPORT<ret>").await?;
    let name = fs::read_dir(dir.path())?.next().unwrap()?.file_name();
    assert_eq!(name, "REPORT.txt");
    assert_eq!(
        doc!(app.editor).path().unwrap().file_name(),
        Some(name.as_os_str())
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refuses_unsafe_targets_and_keeps_dirty_children_on_directory_rename() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let folder = dir.path().join("folder");
    fs::create_dir(&folder)?;
    let file = folder.join("work.txt");
    fs::write(&file, "saved")?;
    let mut app = AppBuilder::new().with_file(&file, None).build()?;
    keys(&mut app, "idraft<esc><space>.").await?;
    keys(&mut app, "<F4>d<esc>").await?;
    keys(&mut app, "<F4>n../outside.txt<ret><esc>").await?;
    assert!(!dir.path().join("outside.txt").exists());
    keys(&mut app, "<ret>").await?;
    keys(&mut app, "folder").await?;
    keys(&mut app, "<F4>dy<esc>").await?;
    assert_eq!(fs::read_to_string(&file)?, "saved");
    keys(&mut app, "<F4>r新目录<ret>").await?;
    let moved = dir.path().join("新目录/work.txt");
    assert_eq!(fs::read_to_string(&moved)?, "saved");
    assert_eq!(doc!(app.editor).path(), Some(moved.as_path()));
    assert!(doc!(app.editor).is_modified());
    Ok(())
}
