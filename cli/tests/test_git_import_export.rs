// Copyright 2022 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use itertools::Itertools as _;
use jj_lib::backend::CommitId;
use testutils::git;

use crate::common::CommandOutput;
use crate::common::TestEnvironment;
use crate::common::TestWorkDir;

#[test]
fn test_resolution_of_git_tracking_bookmarks() {
    let test_env = TestEnvironment::default();
    test_env.run_jj_in(".", ["git", "init", "repo"]).success();
    let work_dir = test_env.work_dir("repo");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "main"])
        .success();
    work_dir
        .run_jj(["describe", "-r", "main", "-m", "old_message"])
        .success();

    // Create local-git tracking bookmark
    let output = work_dir.run_jj(["git", "export"]);
    insta::assert_snapshot!(output, @"");
    // Move the local bookmark somewhere else
    work_dir
        .run_jj(["describe", "-r", "main", "-m", "new_message"])
        .success();
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @r"
    main: qpvuntsm b61d21b6 (empty) new_message
      @git (ahead by 1 commits, behind by 1 commits): qpvuntsm hidden 03757d22 (empty) old_message
    [EOF]
    ");

    // Test that we can address both revisions
    let query = |expr| {
        let template = r#"commit_id ++ " " ++ description"#;
        work_dir.run_jj(["log", "-r", expr, "-T", template, "--no-graph"])
    };
    insta::assert_snapshot!(query("main"), @r"
    b61d21b660c17a7191f3f73873bfe7d3f7938628 new_message
    [EOF]
    ");
    insta::assert_snapshot!(query("main@git"), @r"
    03757d2212d89990ec158e97795b612a38446652 old_message
    [EOF]
    ");
    // Can't be selected by remote_bookmarks()
    insta::assert_snapshot!(query(r#"remote_bookmarks(exact:"main", exact:"git")"#), @"");
}

#[test]
fn test_git_export_conflicting_git_refs() {
    let test_env = TestEnvironment::default();
    test_env.run_jj_in(".", ["git", "init", "repo"]).success();
    let work_dir = test_env.work_dir("repo");

    work_dir
        .run_jj(["bookmark", "create", "-r@", "main"])
        .success();
    work_dir
        .run_jj(["bookmark", "create", "-r@", "main/sub"])
        .success();
    let output = work_dir.run_jj(["git", "export"]);
    insta::with_settings!({filters => vec![("Failed to set: .*", "Failed to set: ...")]}, {
        insta::assert_snapshot!(output, @r#"
        ------- stderr -------
        Warning: Failed to export some bookmarks:
          main/sub@git: Failed to set: ...
        Hint: Git doesn't allow a branch name that looks like a parent directory of
        another (e.g. `foo` and `foo/bar`). Try to rename the bookmarks that failed to
        export or their "parent" bookmarks.
        [EOF]
        "#);
    });
}

#[test]
fn test_git_export_undo() {
    let test_env = TestEnvironment::default();
    test_env.run_jj_in(".", ["git", "init", "repo"]).success();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::open(work_dir.root().join(".jj/repo/store/git"));

    work_dir
        .run_jj(["bookmark", "create", "-r@", "a"])
        .success();
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @r"
    a: qpvuntsm 230dd059 (empty) (no description set)
    [EOF]
    ");
    let output = work_dir.run_jj(["git", "export"]);
    insta::assert_snapshot!(output, @"");
    insta::assert_snapshot!(work_dir.run_jj(["log", "-ra@git"]), @r"
    @  qpvuntsm test.user@example.com 2001-02-03 08:05:07 a 230dd059
    │  (empty) (no description set)
    ~
    [EOF]
    ");

    // Exported refs won't be removed by undoing the export, but the git-tracking
    // bookmark is. This is the same as remote-tracking bookmarks.
    let output = work_dir.run_jj(["op", "undo"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Undid operation: edb40232c741 (2001-02-03 08:05:10) export git refs
    [EOF]
    ");
    insta::assert_debug_snapshot!(get_git_repo_refs(&git_repo), @r#"
    [
        (
            "refs/heads/a",
            CommitId(
                "230dd059e1b059aefc0da06a2e5a7dbf22362f22",
            ),
        ),
    ]
    "#);
    insta::assert_snapshot!(work_dir.run_jj(["log", "-ra@git"]), @r"
    ------- stderr -------
    Error: Revision `a@git` doesn't exist
    Hint: Did you mean `a`?
    [EOF]
    [exit status: 1]
    ");

    // This would re-export bookmark "a" and create git-tracking bookmark.
    let output = work_dir.run_jj(["git", "export"]);
    insta::assert_snapshot!(output, @"");
    insta::assert_snapshot!(work_dir.run_jj(["log", "-ra@git"]), @r"
    @  qpvuntsm test.user@example.com 2001-02-03 08:05:07 a 230dd059
    │  (empty) (no description set)
    ~
    [EOF]
    ");
}

#[test]
fn test_git_import_undo() {
    let test_env = TestEnvironment::default();
    test_env.run_jj_in(".", ["git", "init", "repo"]).success();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::open(work_dir.root().join(".jj/repo/store/git"));

    // Create bookmark "a" in git repo
    let commit_id = work_dir
        .run_jj(&["log", "-Tcommit_id", "--no-graph", "-r@"])
        .success()
        .stdout
        .into_raw();
    let commit_id = gix::ObjectId::from_hex(commit_id.as_bytes()).unwrap();
    git_repo
        .reference(
            "refs/heads/a",
            commit_id,
            gix::refs::transaction::PreviousValue::Any,
            "",
        )
        .unwrap();

    // Initial state we will return to after `undo`. There are no bookmarks.
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @"");
    let base_operation_id = work_dir.current_operation_id();

    let output = work_dir.run_jj(["git", "import"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    bookmark: a@git [new] tracked
    [EOF]
    ");
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @r"
    a: qpvuntsm 230dd059 (empty) (no description set)
      @git: qpvuntsm 230dd059 (empty) (no description set)
    [EOF]
    ");

    // "git import" can be undone by default.
    let output = work_dir.run_jj(["op", "restore", &base_operation_id]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Restored to operation: eac759b9ab75 (2001-02-03 08:05:07) add workspace 'default'
    [EOF]
    ");
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @"");
    // Try "git import" again, which should re-import the bookmark "a".
    let output = work_dir.run_jj(["git", "import"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    bookmark: a@git [new] tracked
    [EOF]
    ");
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @r"
    a: qpvuntsm 230dd059 (empty) (no description set)
      @git: qpvuntsm 230dd059 (empty) (no description set)
    [EOF]
    ");
}

#[test]
fn test_git_import_move_export_with_default_undo() {
    let test_env = TestEnvironment::default();
    test_env.run_jj_in(".", ["git", "init", "repo"]).success();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::open(work_dir.root().join(".jj/repo/store/git"));

    // Create bookmark "a" in git repo
    let commit_id = work_dir
        .run_jj(&["log", "-Tcommit_id", "--no-graph", "-r@"])
        .success()
        .stdout
        .into_raw();
    let commit_id = gix::ObjectId::from_hex(commit_id.as_bytes()).unwrap();
    git_repo
        .reference(
            "refs/heads/a",
            commit_id,
            gix::refs::transaction::PreviousValue::Any,
            "",
        )
        .unwrap();

    // Initial state we will try to return to after `op restore`. There are no
    // bookmarks.
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @"");
    let base_operation_id = work_dir.current_operation_id();

    let output = work_dir.run_jj(["git", "import"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    bookmark: a@git [new] tracked
    [EOF]
    ");
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @r"
    a: qpvuntsm 230dd059 (empty) (no description set)
      @git: qpvuntsm 230dd059 (empty) (no description set)
    [EOF]
    ");

    // Move bookmark "a" and export to git repo
    work_dir.run_jj(["new"]).success();
    work_dir
        .run_jj(["bookmark", "set", "a", "--to=@"])
        .success();
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @r"
    a: yqosqzyt 096dc80d (empty) (no description set)
      @git (behind by 1 commits): qpvuntsm 230dd059 (empty) (no description set)
    [EOF]
    ");
    let output = work_dir.run_jj(["git", "export"]);
    insta::assert_snapshot!(output, @"");
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @r"
    a: yqosqzyt 096dc80d (empty) (no description set)
      @git: yqosqzyt 096dc80d (empty) (no description set)
    [EOF]
    ");

    // "git import" can be undone with the default `restore` behavior, as shown in
    // the previous test. However, "git export" can't: the bookmarks in the git
    // repo stay where they were.
    let output = work_dir.run_jj(["op", "restore", &base_operation_id]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Restored to operation: eac759b9ab75 (2001-02-03 08:05:07) add workspace 'default'
    Working copy  (@) now at: qpvuntsm 230dd059 (empty) (no description set)
    Parent commit (@-)      : zzzzzzzz 00000000 (empty) (no description set)
    [EOF]
    ");
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @"");
    insta::assert_debug_snapshot!(get_git_repo_refs(&git_repo), @r#"
    [
        (
            "refs/heads/a",
            CommitId(
                "096dc80da67094fbaa6683e2a205dddffa31f9a8",
            ),
        ),
    ]
    "#);

    // The last bookmark "a" state is imported from git. No idea what's the most
    // intuitive result here.
    let output = work_dir.run_jj(["git", "import"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    bookmark: a@git [new] tracked
    [EOF]
    ");
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @r"
    a: yqosqzyt 096dc80d (empty) (no description set)
      @git: yqosqzyt 096dc80d (empty) (no description set)
    [EOF]
    ");
}

#[must_use]
fn get_bookmark_output(work_dir: &TestWorkDir) -> CommandOutput {
    work_dir.run_jj(["bookmark", "list", "--all-remotes"])
}

fn get_git_repo_refs(git_repo: &gix::Repository) -> Vec<(bstr::BString, CommitId)> {
    let mut refs: Vec<_> = git_repo
        .references()
        .unwrap()
        .all()
        .unwrap()
        .filter_ok(|git_ref| {
            matches!(
                git_ref.name().category(),
                Some(gix::reference::Category::Tag)
                    | Some(gix::reference::Category::LocalBranch)
                    | Some(gix::reference::Category::RemoteBranch),
            )
        })
        .filter_map_ok(|mut git_ref| {
            let full_name = git_ref.name().as_bstr().to_owned();
            let git_commit = git_ref.peel_to_commit().ok()?;
            let commit_id = CommitId::from_bytes(git_commit.id().as_bytes());
            Some((full_name, commit_id))
        })
        .try_collect()
        .unwrap();
    refs.sort();
    refs
}
