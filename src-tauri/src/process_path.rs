use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub fn executable(program: &str, paths: &OsStr) -> Option<PathBuf> {
    let name = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    find(&[name.as_str()], paths, &[])
}

/// First existing file named one of `names` (in order) in each absolute PATH
/// entry, then the platform tool directories, then `fallback` directories.
/// Never searches through a shell.
/// `names` are exact file names with their extension (`az.cmd`, `winget.exe`).
pub fn find(names: &[&str], paths: &OsStr, fallback: &[PathBuf]) -> Option<PathBuf> {
    find_with(names, paths, fallback, Path::is_file)
}

fn find_with(
    names: &[&str],
    paths: &OsStr,
    fallback: &[PathBuf],
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let directories = std::env::split_paths(paths).filter(|path| path.is_absolute());
    #[cfg(target_os = "macos")]
    let directories = directories.chain([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    directories
        .chain(fallback.iter().filter(|path| path.is_absolute()).cloned())
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|path| is_file(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn searches_path_entries_in_order_then_fallbacks_with_exact_file_names() {
        let root = std::env::current_dir().unwrap().join("fake");
        let (first, second, fallback) = (root.join("a"), root.join("b"), root.join("c"));
        let paths =
            std::env::join_paths([PathBuf::from("relative"), first.clone(), second.clone()])
                .unwrap();
        let fallbacks = [PathBuf::from("relative-fallback"), fallback.clone()];
        let find = |files: &[PathBuf]| {
            let files: HashSet<_> = files.iter().cloned().collect();
            find_with(&["az.cmd"], &paths, &fallbacks, |path| files.contains(path))
        };
        let in_second = second.join("az.cmd");
        let in_fallback = fallback.join("az.cmd");
        assert_eq!(
            find(&[in_fallback.clone(), in_second.clone()]),
            Some(in_second)
        );
        assert_eq!(find(std::slice::from_ref(&in_fallback)), Some(in_fallback));
        // Only the exact name: no extension guessing, no relative directories.
        assert_eq!(
            find(&[
                first.join("az"),
                first.join("az.exe"),
                first.join("az.bat"),
                PathBuf::from("relative").join("az.cmd"),
                PathBuf::from("relative-fallback").join("az.cmd"),
            ]),
            None
        );
    }

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
