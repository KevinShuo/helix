#[cfg(feature = "git")]
pub(crate) use helix_vcs::ignore_completion::{Effect, Rules, Sources};

#[cfg(not(feature = "git"))]
pub(crate) use fallback::{Effect, Rules, Sources};

#[cfg(not(feature = "git"))]
mod fallback {
    use std::path::Path;

    #[allow(dead_code)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Effect {
        Changed,
        Unchanged,
        Overridden,
        ParentExcluded,
        Unknown,
    }

    pub(crate) struct Sources;
    impl Sources {
        pub(crate) fn load(_: &Path, _: &Path, _: bool) -> Self {
            Self
        }
        pub(crate) fn rules(&self, _: &str, _: &str) -> Rules<'_> {
            Rules(std::marker::PhantomData)
        }
    }
    pub(crate) struct Rules<'a>(std::marker::PhantomData<&'a Sources>);
    impl Rules<'_> {
        pub(crate) fn ignore_case(&self) -> bool {
            false
        }
        pub(crate) fn tracked(&self, _: &Path) -> Option<bool> {
            None
        }
        pub(crate) fn effect(&self, _: &str, _: &Path, _: bool) -> Effect {
            Effect::Unknown
        }
    }
}
