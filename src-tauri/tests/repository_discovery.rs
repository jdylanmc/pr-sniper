use pr_sniper_lib::discovery::discover;
use std::fs;
use std::os::unix::fs::symlink;

mod support;
use support::Fixture;

fn repository(root: &std::path::Path, path: &str, config: &str) {
    let git = root.join(path).join(".git");
    fs::create_dir_all(&git).unwrap();
    fs::write(git.join("config"), config).unwrap();
}

#[test]
fn discovery_reads_github_metadata_without_executing_repository_code() {
    let fixture = Fixture::new();
    repository(fixture.path(), "team/project", "[remote \"origin\"]\nurl = git@github.com:Octo/Hello-World.git\n[core]\nsshCommand = touch SHOULD_NOT_EXIST\n[include]\npath = /untrusted/config");
    repository(
        fixture.path(),
        "other",
        "[remote \"upstream\"]\nurl = https://github.com/Other/Project.git\n",
    );
    let result = discover(fixture.path()).unwrap();
    assert!(!fixture.store().has_saved_settings());
    let names: Vec<_> = result
        .repositories
        .iter()
        .map(|r| r.name.as_deref())
        .collect();
    assert_eq!(names, vec![Some("other/project"), Some("octo/hello-world")]);
    assert!(!fixture.path().join("SHOULD_NOT_EXIST").exists());
    assert!(result.warnings.is_empty());
}

#[test]
fn discovery_never_follows_nested_symlinks_or_linked_git_metadata() {
    let inside = Fixture::new();
    let outside = Fixture::new();
    repository(
        outside.path(),
        "secret",
        "[remote \"origin\"]\nurl = https://github.com/private/repository\n",
    );
    symlink(outside.path(), inside.path().join("linked-folder")).unwrap();
    fs::create_dir(inside.path().join("worktree")).unwrap();
    fs::write(
        inside.path().join("worktree/.git"),
        format!("gitdir: {}", outside.path().join("secret/.git").display()),
    )
    .unwrap();
    fs::create_dir_all(inside.path().join("linked-config/.git")).unwrap();
    symlink(
        outside.path().join("secret/.git/config"),
        inside.path().join("linked-config/.git/config"),
    )
    .unwrap();
    let result = discover(inside.path()).unwrap();
    assert_eq!(result.repositories.len(), 2);
    assert!(result
        .repositories
        .iter()
        .all(|r| r.name.is_none() && r.unavailable.is_some()));
}

#[test]
fn discovery_is_explicit_and_reports_empty_unsupported_and_ambiguous_states() {
    let fixture = Fixture::new();
    assert!(discover(std::path::Path::new("relative")).is_err());
    assert!(discover(fixture.path()).unwrap().repositories.is_empty());
    repository(
        fixture.path(),
        "local",
        "[core]\nrepositoryformatversion = 0",
    );
    repository(
        fixture.path(),
        "credentials",
        "[remote \"origin\"]\nurl = https://user:password@github.com/owner/repo\n",
    );
    repository(fixture.path(), "ambiguous", "[remote \"first\"]\nurl = git@github.com:one/repo\n[remote \"second\"]\nurl = git@github.com:two/repo\n");
    let result = discover(fixture.path()).unwrap();
    assert_eq!(result.repositories.len(), 3);
    assert!(result.repositories.iter().all(|r| r.name.is_none()));
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("password"));
}
