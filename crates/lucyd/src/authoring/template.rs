use std::path::Path;

const FILES: &[(&str, &str)] = &[
    (
        "manifest.toml",
        include_str!("../../plugin-template/manifest.toml.template"),
    ),
    (
        "Cargo.toml",
        include_str!("../../plugin-template/Cargo.toml.template"),
    ),
    ("build.sh", include_str!("../../plugin-template/build.sh")),
    ("test.sh", include_str!("../../plugin-template/test.sh")),
    (
        "src/lib.rs",
        include_str!("../../plugin-template/src/lib.rs"),
    ),
    (
        "src/main.rs",
        include_str!("../../plugin-template/src/main.rs"),
    ),
    (
        "tests/plugin.rs",
        include_str!("../../plugin-template/tests/plugin.rs"),
    ),
    (
        "tests/support/mod.rs",
        include_str!("../../plugin-template/tests/support/mod.rs"),
    ),
    (
        "tests/fixtures/web/example.com/index.html",
        include_str!("../../plugin-template/tests/fixtures/web/example.com/index.html"),
    ),
];

pub fn write(dir: &Path, name: &str) -> std::io::Result<()> {
    for (path, text) in FILES {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text.replace("{{name}}", name))?;
    }
    Ok(())
}
