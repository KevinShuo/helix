use super::*;
use helix_core::diagnostic::Severity;
use helix_view::problems::ProblemSource;

pub(super) const SIGNATURE: Signature = Signature {
    positionals: (0, Some(0)),
    flags: &[
        Flag {
            name: "source",
            doc: "Problem source",
            completions: Some(&["all", "lsp", "build"]),
            ..Flag::DEFAULT
        },
        Flag {
            name: "severity",
            doc: "Minimum severity",
            completions: Some(&["warning", "error", "all"]),
            ..Flag::DEFAULT
        },
        Flag {
            name: "scope",
            doc: "Navigation scope",
            completions: Some(&["workspace", "document"]),
            ..Flag::DEFAULT
        },
    ],
    ..Signature::DEFAULT
};

fn filters(editor: &mut Editor, args: &Args) -> anyhow::Result<()> {
    let mut filter = editor.problems.filter;
    if let Some(value) = args.get_flag("source") {
        filter.source = match value {
            "all" => None,
            "lsp" => Some(ProblemSource::Lsp),
            "build" => Some(ProblemSource::Build),
            _ => bail!("Invalid source: {value}"),
        };
    }
    if let Some(value) = args.get_flag("severity") {
        filter.minimum_severity = match value {
            "all" => None,
            "warning" => Some(Severity::Warning),
            "error" => Some(Severity::Error),
            _ => bail!("Invalid severity: {value}"),
        };
    }
    if let Some(value) = args.get_flag("scope") {
        filter.document = match value {
            "document" => true,
            "workspace" => false,
            _ => bail!("Invalid scope: {value}"),
        };
    }
    editor.problems.filter = filter;
    Ok(())
}

pub(super) fn next(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event == PromptEvent::Validate {
        filters(cx.editor, &args)?;
        super::super::problems::navigate(cx.editor, true, 1);
    }
    Ok(())
}
pub(super) fn previous(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event == PromptEvent::Validate {
        filters(cx.editor, &args)?;
        super::super::problems::navigate(cx.editor, false, 1);
    }
    Ok(())
}
pub(super) fn list(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event == PromptEvent::Validate {
        filters(cx.editor, &args)?;
        crate::job::dispatch_blocking(super::super::problems::picker);
    }
    Ok(())
}

pub(super) fn compile(
    cx: &mut compositor::Context,
    args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event == PromptEvent::Validate {
        crate::handlers::compilation::start(cx.editor, args.first().unwrap_or("").to_owned())?;
    }
    Ok(())
}
pub(super) fn recompile(
    cx: &mut compositor::Context,
    _: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event == PromptEvent::Validate {
        crate::handlers::compilation::recompile(cx.editor)?;
    }
    Ok(())
}
pub(super) fn cancel(
    cx: &mut compositor::Context,
    _: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event == PromptEvent::Validate {
        crate::handlers::compilation::cancel(cx.editor);
    }
    Ok(())
}
pub(super) fn log(cx: &mut compositor::Context, _: Args, event: PromptEvent) -> anyhow::Result<()> {
    if event == PromptEvent::Validate {
        crate::handlers::compilation::show_log(cx.editor)?;
    }
    Ok(())
}
