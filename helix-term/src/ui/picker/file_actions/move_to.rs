use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{bail, ensure};
use helix_core::Position;
use helix_view::{
    graphics::{CursorKind, Rect},
    Editor,
};
use tui::buffer::Buffer;

use crate::{
    alt,
    compositor::{Component, Context, Event, EventResult},
    ctrl, key,
    ui::{self, overlay::Overlay, FileExplorer, Picker, PickerColumn},
};

pub(super) struct MoveTo {
    source: PathBuf,
    target: PathBuf,
    directories: Picker<PathBuf, PathBuf>,
    error: Option<String>,
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        helix_stdx::path::eq_ignore_case(a, b)
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

#[cfg(unix)]
fn filesystem_id(directory: &Path) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::metadata(directory)?.dev())
}

#[cfg(windows)]
fn filesystem_id(directory: &Path) -> io::Result<u64> {
    let handle = winapi_util::Handle::from_path_any(directory)?;
    Ok(winapi_util::file::information(&handle)?.volume_serial_number())
}

impl MoveTo {
    pub(super) fn new(source: &Path, root: &Path, editor: &Editor) -> io::Result<Self> {
        fs::read_dir(root)?;
        // Every intermediate directory is a possible destination.
        let entries = ui::directory_entries(root, editor, false)?
            .into_iter()
            .filter_map(|(path, is_dir)| is_dir.then_some(path));
        let column = PickerColumn::new("path", |path: &PathBuf, root: &PathBuf| {
            format!("{}/", path.strip_prefix(root).unwrap_or(path).display()).into()
        });
        let mut picker = Self {
            source: source.to_owned(),
            target: root.join(source.file_name().expect("a movable entry has a name")),
            directories: Picker::new([column], 0, entries, root.to_owned(), |_, _, _| {}),
            error: None,
        };
        picker.error = picker.validate(editor).err().map(|err| err.to_string());
        Ok(picker)
    }

    fn validate(&self, editor: &Editor) -> anyhow::Result<PathBuf> {
        let root = self.directories.editor_data.as_path();
        let real_root = fs::canonicalize(root)?;
        let parent = fs::canonicalize(self.source.parent().unwrap())?;
        ensure!(
            !paths_equal(&real_root, &parent),
            "Already in the original directory"
        );
        #[cfg(any(unix, windows))]
        ensure!(
            filesystem_id(&real_root)? == filesystem_id(&parent)?,
            "Moving between filesystems is not supported"
        );
        if self.source.is_dir() {
            let source = fs::canonicalize(&self.source)?;
            let entry = parent.join(self.source.file_name().unwrap());
            ensure!(
                !real_root.ancestors().any(|path| paths_equal(path, &source))
                    && helix_stdx::path::moved_suffix(root, &self.source, &entry).is_none(),
                "Cannot move a directory inside itself"
            );
        }
        let name = Path::new(self.target.file_name().unwrap());
        match fs::symlink_metadata(real_root.join(name)) {
            Ok(_) => bail!("Already exists: {}", self.target.display()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => (),
            Err(err) => return Err(err.into()),
        }
        // Unsaved buffers can already own the destination or one of its children,
        // including through a different spelling of the destination directory.
        let occupied = editor.documents().filter_map(|doc| doc.path()).any(|path| {
            path.ancestors().any(|ancestor| {
                ancestor
                    .file_name()
                    .is_some_and(|n| paths_equal(Path::new(n), name))
                    && ancestor
                        .parent()
                        .and_then(|p| fs::canonicalize(p).ok())
                        .is_some_and(|p| paths_equal(&p, &real_root))
            })
        });
        ensure!(!occupied, "An open buffer uses {}", self.target.display());
        Ok(real_root)
    }

    fn submit(&self, cx: &mut Context) -> anyhow::Result<EventResult> {
        cx.block_try_flush_writes()?;
        // The move itself can invalidate a link used to browse here. Bind the
        // operation, buffers and explorer to the resolved directory.
        let root = helix_stdx::path::normalize(self.validate(cx.editor)?);
        let target = root.join(self.target.file_name().unwrap());
        if let Err(err) = cx.editor.move_path_noreplace(&self.source, &target) {
            match err.kind() {
                io::ErrorKind::CrossesDevices => {
                    bail!("Moving between filesystems is not supported")
                }
                io::ErrorKind::AlreadyExists => bail!("Already exists: {}", self.target.display()),
                _ => return Err(err.into()),
            }
        }
        Ok(EventResult::Consumed(Some(Box::new(
            move |compositor, cx| {
                compositor.pop();
                if let Some(explorer) = compositor.top_mut::<Overlay<FileExplorer>>() {
                    let explorer = &mut explorer.content;
                    Arc::make_mut(&mut explorer.editor_data).0 = root;
                    explorer.prompt.clear(cx.editor);
                    explorer.handle_prompt_change(false);
                    explorer.show_preview = true;
                    if let Err(err) = explorer.refresh_entries(Some(target), cx.editor) {
                        cx.editor
                            .set_error(format!("Move succeeded, refresh failed: {err}"));
                    }
                }
            },
        ))))
    }
}

impl Component for MoveTo {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let result = match event {
            Event::Key(key!(Esc) | ctrl!('c')) => {
                return EventResult::Consumed(Some(Box::new(|c, _| {
                    c.pop();
                })))
            }
            Event::Key(key!(Enter)) => {
                let Some(path) = self.directories.selection() else {
                    return EventResult::Consumed(None);
                };
                // The synthetic ../ row returns to the directory used to enter a link.
                let root = if path.file_name().is_none() {
                    self.directories.editor_data.parent().unwrap()
                } else {
                    path
                };
                Self::new(&self.source, root, cx.editor)
                    .map(|picker| {
                        *self = picker;
                        EventResult::Consumed(None)
                    })
                    .map_err(anyhow::Error::from)
            }
            Event::Key(helix_view::input::KeyEvent {
                code: helix_view::keyboard::KeyCode::F(4),
                modifiers: helix_view::keyboard::KeyModifiers::NONE,
            }) => self.submit(cx),
            Event::Key(alt!(Enter) | ctrl!('v') | ctrl!('s')) => {
                return EventResult::Consumed(None)
            }
            _ => return self.directories.handle_event(event, cx),
        };
        result.unwrap_or_else(|err| {
            self.error = Some(err.to_string());
            EventResult::Consumed(None)
        })
    }

    fn render(&mut self, area: Rect, surface: &mut Buffer, cx: &mut Context) {
        let style = cx.editor.theme.get("ui.text");
        surface.clear_with(area, cx.editor.theme.get("ui.background"));
        if area.height < 4 || area.width < 3 {
            return;
        }
        let title = format!(
            "Move {} to...",
            self.source.file_name().unwrap().to_string_lossy()
        );
        surface.set_stringn(area.x, area.y, &title, area.width as usize, style);
        let body = area.clip_top(1).clip_bottom(2);
        self.directories.required_size((body.width, body.height));
        self.directories.render(body, surface, cx);
        surface.set_string_truncated(
            area.x,
            area.bottom() - 2,
            &self.target.to_string_lossy(),
            area.width as usize,
            |_| style,
            true,
            true,
        );
        let hint = self
            .error
            .as_deref()
            .unwrap_or("F4 Move here    Enter Open directory    Esc Cancel");
        surface.set_stringn(area.x, area.bottom() - 1, hint, area.width as usize, style);
    }

    fn cursor(&self, area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        self.directories
            .cursor(area.clip_top(1).clip_bottom(2), editor)
    }
}
