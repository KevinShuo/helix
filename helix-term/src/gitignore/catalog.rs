pub(crate) struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub aliases: &'static str,
    pub text: &'static str,
}

macro_rules! template {
    ($id:literal, $name:literal, $category:literal, $description:literal, $aliases:literal) => {
        Template {
            id: $id,
            name: $name,
            category: $category,
            description: $description,
            aliases: $aliases,
            text: include_str!(concat!("templates/", $id, ".gitignore")),
        }
    };
}

/// Catalog order is the generation order, independent of selection or detection order.
pub(crate) const TEMPLATES: &[Template] = &[
    template!("rust", "Rust", "Language", "Cargo build output; Cargo.lock is kept.", "cargo"),
    template!("node", "Node", "Language", "Node dependencies, caches, distribution and coverage output; lockfiles are kept.", "javascript typescript js ts npm yarn pnpm"),
    template!("python", "Python", "Language", "Python bytecode, virtual environments, build and test caches.", "pip poetry uv pytest"),
    template!("go", "Go", "Language", "Go test binaries and root coverage output; go.sum is kept.", "golang"),
    template!("java", "Java", "Language", "Compiled Java classes; archives and build wrappers are kept.", "jvm kotlin scala"),
    template!("cpp", "C/C++", "Language", "Compiled objects, libraries and executables.", "c c++ clang gcc msvc"),
    template!("dotnet", ".NET", "Language", ".NET build, IDE cache and test output.", "csharp c# fsharp visualstudio"),
    template!("maven", "Maven", "Build", "Root Maven output and local backup POMs; wrappers are kept.", "mvn java"),
    template!("gradle", "Gradle", "Build", "Gradle cache and root build output; wrappers are kept.", "jvm kotlin java"),
    template!("cmake", "CMake", "Build", "CMake generated files and root build directory; source Makefiles are kept.", "c cpp c++"),
    template!("windows", "Windows", "OS", "Windows thumbnail, desktop and recycle-bin files.", "win"),
    template!("macos", "macOS", "OS", "macOS Finder metadata and resource forks.", "mac osx darwin"),
    template!("linux", "Linux", "OS", "Linux desktop trash and temporary filesystem files.", "unix"),
    template!("vscode", "VS Code", "Editor", "Local VS Code files; settings, tasks, launch and extensions remain shareable.", "visual studio code"),
    template!("jetbrains", "JetBrains", "Editor", "Local .idea files; code styles, inspections, run configurations and VCS settings remain shareable.", "idea intellij pycharm clion webstorm rider"),
    template!("vim", "Vim", "Editor", "Vim swap, undo and session files.", "neovim nvim"),
    template!("emacs", "Emacs", "Editor", "Emacs auto-save and local session files.", "elisp"),
    template!("temporary", "Temporary files", "General", "Temporary and backup suffixes; review if your project uses these as test data.", "temp backup common generic"),
    template!("dotenv", "dotenv", "General", "Environment files; .env.example, .env.sample and .env.template are kept. Add exceptions for other example names.", "environment secrets env"),
];
