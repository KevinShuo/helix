#[cfg(feature = "integration")]
mod test {
    mod helpers;

    use helix_core::{syntax::config::AutoPairConfig, Selection};
    use helix_term::config::Config;

    use indoc::indoc;

    use self::helpers::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn hello_world() -> anyhow::Result<()> {
        test(("#[\n|]#", "ihello world<esc>", "hello world#[|\n]#")).await?;
        Ok(())
    }

    mod ai_completion;
    mod auto_pairs;
    mod command_line;
    mod commands;
    mod folding;
    #[cfg(feature = "git")]
    mod function_hints;
    #[cfg(feature = "git")]
    mod git_tools;
    mod movement;
    mod splits;
}
