use super::{send_keys_and_wait as keys, *};
use helix_view::doc;
use std::fs;

#[cfg(any(unix, windows))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_directory_link_preserves_open_child() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    fs::create_dir(dir.path().join("real"))?;
    fs::write(dir.path().join("real/work.txt"), "saved")?;
    let link = dir.path().join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("real"), &link)?;
    #[cfg(windows)]
    {
        let result = std::process::Command::new("cmd")
            .current_dir(dir.path())
            .args(["/d", "/c", "mklink", "/j", "link", "real"])
            .output()?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
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
