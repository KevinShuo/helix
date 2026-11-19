use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{bail, ensure, Context as _};
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
    sources: Arc<[PathBuf]>,
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

fn check_target_names(sources: &[PathBuf], root: &Path) -> anyhow::Result<()> {
    if sources.len() < 2 {
        return Ok(());
    }
    let mut names: Vec<_> = sources
        .iter()
        .map(|p| Path::new(p.file_name().unwrap()))
        .collect();
    #[cfg(windows)]
    names.sort_unstable_by(|a, b| helix_stdx::path::cmp_ignore_case(a, b));
    #[cfg(not(windows))]
    names.sort_unstable();
    if let Some(pair) = names.windows(2).find(|p| paths_equal(p[0], p[1])) {
        bail!(
            "Multiple selected entries would use {}",
            root.join(pair[1]).display()
        );
    }
    #[cfg(target_vendor = "apple")]
    {
        use std::{collections::HashSet, ffi::CString, os::unix::ffi::OsStrExt};
        use unicode_normalization::UnicodeNormalization;
        let directory = CString::new(root.as_os_str().as_bytes())?;
        // SAFETY: the path is NUL terminated and remains alive for this call.
        let case_sensitive =
            unsafe { libc::pathconf(directory.as_ptr(), libc::_PC_CASE_SENSITIVE) } > 0;
        let mut normalized = HashSet::new();
        let mut folded = HashSet::new();
        for name in names {
            if let Some(text) = name.to_str() {
                let text: String = text.nfd().collect();
                let unique = if case_sensitive {
                    normalized.insert(text)
                } else {
                    folded.insert(unicase::UniCase::unicode(text))
                };
                ensure!(
                    unique,
                    "Multiple selected entries would use {}",
                    root.join(name).display()
                );
            }
        }
    }
    Ok(())
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
    pub(super) fn new(sources: Arc<[PathBuf]>, root: &Path, editor: &Editor) -> io::Result<Self> {
        debug_assert!(!sources.is_empty());
        fs::read_dir(root)?;
        // Every intermediate directory is a possible destination.
        let entries = ui::directory_entries(root, editor, false)?
            .into_iter()
            .filter_map(|(path, is_dir)| is_dir.then_some(path));
        let column = PickerColumn::new("path", |path: &PathBuf, root: &PathBuf| {
            format!("{}/", path.strip_prefix(root).unwrap_or(path).display()).into()
        });
        let mut picker = Self {
            sources,
            directories: Picker::new([column], 0, entries, root.to_owned(), |_, _, _| {}),
            error: None,
        };
        picker.error = picker.validate(editor).err().map(|err| err.to_string());
        Ok(picker)
    }

    fn validate(&self, editor: &Editor) -> anyhow::Result<Vec<(PathBuf, PathBuf)>> {
        let root = self.directories.editor_data.as_path();
        let real_root = fs::canonicalize(root)?;
        let destination = helix_stdx::path::normalize(&real_root);
        let mut moves = Vec::with_capacity(self.sources.len());
        #[cfg(any(unix, windows))]
        let device = filesystem_id(&real_root)?;
        check_target_names(&self.sources, root)?;
        for source in self.sources.iter() {
            let name = Path::new(source.file_name().unwrap());
            let target = root.join(name);
            fs::symlink_metadata(source)
                .with_context(|| format!("Cannot access {}", source.display()))?;
            let parent = fs::canonicalize(source.parent().unwrap())?;
            ensure!(
                !paths_equal(&real_root, &parent),
                "Already in the original directory"
            );
            #[cfg(any(unix, windows))]
            ensure!(
                device == filesystem_id(&parent)?,
                "Moving between filesystems is not supported"
            );
            if source.is_dir() {
                let real_source = fs::canonicalize(source)?;
                let entry = parent.join(name);
                ensure!(
                    !real_root
                        .ancestors()
                        .any(|path| paths_equal(path, &real_source))
                        && helix_stdx::path::moved_suffix(root, source, &entry).is_none(),
                    "Cannot move a directory inside itself"
                );
                // Flattened rows can refer to entries reached through another marked directory.
                for other in self
                    .sources
                    .iter()
                    .filter(|p| p.parent() != source.parent())
                {
                    ensure!(
                        helix_stdx::path::moved_suffix(other.parent().unwrap(), source, &entry)
                            .is_none(),
                        "Selection overlaps: {} and {}",
                        source.display(),
                        other.display()
                    );
                }
            }
            match fs::symlink_metadata(real_root.join(name)) {
                Ok(_) => bail!("Already exists: {}", target.display()),
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
            ensure!(!occupied, "An open buffer uses {}", target.display());
            // Another move may break an alias used to reach this source. Resolve
            // the parent, keeping a symlink in the leaf position as an entry.
            let bound = if self.sources.len() > 1 {
                helix_stdx::path::normalize(parent).join(name)
            } else {
                source.clone()
            };
            moves.push((bound, destination.join(name)));
        }
        Ok(moves)
    }

    fn submit(&self, cx: &mut Context) -> anyhow::Result<EventResult> {
        cx.block_try_flush_writes()?;
        // The move itself can invalidate a link used to browse here. Bind the
        // operation, buffers and explorer to the resolved directory.
        let moves = self.validate(cx.editor)?;
        let result = match moves.as_slice() {
            [(source, target)] => cx
                .editor
                .move_path_noreplace(source, target)
                .map_err(|err| (0, err)),
            moves => cx.editor.move_paths_noreplace(moves),
        };
        if let Err((index, err)) = result {
            let (source, target) = &moves[index];
            let reason = match err.kind() {
                io::ErrorKind::CrossesDevices => {
                    "Moving between filesystems is not supported".into()
                }
                io::ErrorKind::AlreadyExists => format!("Already exists: {}", target.display()),
                _ => format!("{}: {err}", source.display()),
            };
            if index == 0 {
                bail!("{reason}");
            }
            let message = format!(
                "Moved {index} of {}. {reason}. Remaining entries are marked.",
                self.sources.len()
            );
            let remaining = self.sources[index..]
                .iter()
                .zip(&moves[index..])
                .map(|(original, (bound, _))| {
                    if fs::canonicalize(original.parent().unwrap()).is_ok_and(|parent| {
                        paths_equal(
                            &helix_stdx::path::normalize(parent),
                            bound.parent().unwrap(),
                        )
                    }) {
                        original.clone()
                    } else {
                        bound.clone()
                    }
                })
                .collect();
            return Ok(EventResult::Consumed(Some(Box::new(
                move |compositor, cx| {
                    compositor.pop();
                    if let Some(explorer) = compositor.top_mut::<Overlay<FileExplorer>>() {
                        let explorer = &mut explorer.content;
                        explorer.action_menu.as_mut().unwrap().marked = remaining;
                        if let Err(err) = explorer.refresh_entries(None, cx.editor) {
                            cx.editor
                                .set_error(format!("{message} Refresh failed: {err}"));
                            return;
                        }
                    }
                    cx.editor.set_error(message);
                },
            ))));
        }
        let target = moves.into_iter().next().unwrap().1;
        let root = target.parent().unwrap().to_owned();
        Ok(EventResult::Consumed(Some(Box::new(
            move |compositor, cx| {
                compositor.pop();
                if let Some(explorer) = compositor.top_mut::<Overlay<FileExplorer>>() {
                    let explorer = &mut explorer.content;
                    explorer.action_menu.as_mut().unwrap().marked.clear();
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
                Self::new(self.sources.clone(), root, cx.editor)
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
        let root = self.directories.editor_data.as_path();
        let (title, target) = if let [source] = self.sources.as_ref() {
            let name = source.file_name().unwrap();
            (
                format!("Move {} to...", name.to_string_lossy()),
                root.join(name),
            )
        } else {
            let names = self
                .sources
                .iter()
                .take(3)
                .map(|p| p.file_name().unwrap().to_string_lossy())
                .collect::<Vec<_>>()
                .join(", ");
            let more = if self.sources.len() > 3 { ", …" } else { "" };
            (
                format!("Move {} items: {names}{more}", self.sources.len()),
                root.to_owned(),
            )
        };
        surface.set_stringn(area.x, area.y, &title, area.width as usize, style);
        let body = area.clip_top(1).clip_bottom(2);
        self.directories.required_size((body.width, body.height));
        self.directories.render(body, surface, cx);
        surface.set_string_truncated(
            area.x,
            area.bottom() - 2,
            &target.to_string_lossy(),
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
