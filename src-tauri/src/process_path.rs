use std::ffi::OsStr;
use std::path::PathBuf;

pub fn executable(program: &str, paths: &OsStr) -> Option<PathBuf> {
    let name = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    find(&[name.as_str()], paths, &[])
}

/// First existing file named one of `names` (in order) in each absolute PATH
/// entry, then the platform tool directories, then `fallback` directories.
/// Never searches through a shell.
pub fn find(names: &[&str], paths: &OsStr, fallback: &[PathBuf]) -> Option<PathBuf> {
    let directories = std::env::split_paths(paths).filter(|path| path.is_absolute());
    #[cfg(target_os = "macos")]
    let directories = directories.chain([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    directories
        .chain(fallback.iter().filter(|path| path.is_absolute()).cloned())
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_native_executable_in_absolute_path_with_spaces_without_shell_search() {
        let root = std::env::current_dir()
            .unwrap()
            .join(format!(".pr-sniper-path-{}", uuid::Uuid::new_v4()));
        let directory = root.join("tools with spaces");
        std::fs::create_dir_all(&directory).unwrap();
        let binary = directory.join(format!("fixture{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&binary, b"not executed").unwrap();
        let paths = std::env::join_paths([PathBuf::from("."), directory]).unwrap();
        assert_eq!(executable("fixture", &paths), Some(binary));
        assert_eq!(executable("missing", &paths), None);
        assert_eq!(executable("fixture", OsStr::new(".")), None);
        let fallback = root.join("fallback");
        std::fs::create_dir_all(&fallback).unwrap();
        let script = fallback.join("fixture.cmd");
        std::fs::write(&script, b"not executed").unwrap();
        assert_eq!(
            find(&["fixture.cmd"], OsStr::new(""), &[fallback]),
            Some(script)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
