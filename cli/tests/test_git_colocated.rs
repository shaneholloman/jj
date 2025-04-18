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

use std::fmt::Write as _;
use std::path::Path;

use testutils::git;

use crate::common::CommandOutput;
use crate::common::TestEnvironment;
use crate::common::TestWorkDir;

#[test]
fn test_git_colocated() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::init(work_dir.root());

    // Create an initial commit in Git
    let tree_id = git::add_commit(
        &git_repo,
        "refs/heads/master",
        "file",
        b"contents",
        "initial",
        &[],
    )
    .tree_id;
    git::checkout_tree_index(&git_repo, tree_id);
    assert_eq!(work_dir.read_file("file"), b"contents");
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"97358f54806c7cd005ed5ade68a779595efbae7e"
    );

    // Import the repo
    work_dir
        .run_jj(["git", "init", "--git-repo", "."])
        .success();
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  c3a12656d5027825bc69f40e11dc0bb381d7c277
    ○  97358f54806c7cd005ed5ade68a779595efbae7e master git_head() initial
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"97358f54806c7cd005ed5ade68a779595efbae7e"
    );

    // Modify the working copy. The working-copy commit should changed, but the Git
    // HEAD commit should not
    work_dir.write_file("file", "modified");
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  59642000f061cf23eb37ab6eecce428afe7824da
    ○  97358f54806c7cd005ed5ade68a779595efbae7e master git_head() initial
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"97358f54806c7cd005ed5ade68a779595efbae7e"
    );

    // Create a new change from jj and check that it's reflected in Git
    work_dir.run_jj(["new"]).success();
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  fd4e130e43068d76ec6eb2c6df01653ea12eccb4
    ○  59642000f061cf23eb37ab6eecce428afe7824da git_head()
    ○  97358f54806c7cd005ed5ade68a779595efbae7e master initial
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
    assert!(git_repo.head().unwrap().is_detached());
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"59642000f061cf23eb37ab6eecce428afe7824da"
    );
}

#[test]
fn test_git_colocated_intent_to_add() {
    let test_env = TestEnvironment::default();
    test_env
        .run_jj_in(".", ["git", "init", "--colocate", "repo"])
        .success();
    let work_dir = test_env.work_dir("repo");

    // A file added directly on top of the root commit should be marked as
    // intent-to-add
    work_dir.write_file("file1.txt", "contents");
    work_dir.run_jj(["status"]).success();
    insta::assert_snapshot!(get_index_state(work_dir.root()), @"Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 file1.txt");

    // Another new file should be marked as intent-to-add
    work_dir.run_jj(["new"]).success();
    work_dir.write_file("file2.txt", "contents");
    work_dir.run_jj(["status"]).success();
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) 0839b2e9412b ctime=0:0 mtime=0:0 size=0 flags=0 file1.txt
    Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 file2.txt
    ");

    // After creating a new commit, it should not longer be marked as intent-to-add
    work_dir.run_jj(["new"]).success();
    work_dir.write_file("file2.txt", "contents");
    work_dir.run_jj(["status"]).success();
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) 0839b2e9412b ctime=0:0 mtime=0:0 size=0 flags=0 file1.txt
    Unconflicted Mode(FILE) 0839b2e9412b ctime=0:0 mtime=0:0 size=0 flags=0 file2.txt
    ");

    // If we edit an existing commit, new files are marked as intent-to-add
    work_dir.run_jj(["edit", "@-"]).success();
    work_dir.run_jj(["status"]).success();
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) 0839b2e9412b ctime=0:0 mtime=0:0 size=0 flags=0 file1.txt
    Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 file2.txt
    ");

    // If we remove the added file, it's removed from the index
    work_dir.remove_file("file2.txt");
    work_dir.run_jj(["status"]).success();
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) 0839b2e9412b ctime=0:0 mtime=0:0 size=0 flags=0 file1.txt
    ");
}

#[test]
fn test_git_colocated_unborn_bookmark() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::init(work_dir.root());

    // add a file to an (in memory) index
    let add_file_to_index = |name: &str, data: &str| {
        let mut index_manager = git::IndexManager::new(&git_repo);
        index_manager.add_file(name, data.as_bytes());
        index_manager.sync_index();
    };

    // checkout index (i.e., drop the in-memory changes)
    let checkout_index = || {
        let mut index = git_repo.open_index().unwrap();
        let objects = git_repo.objects.clone();
        gix::worktree::state::checkout(
            &mut index,
            git_repo.workdir().unwrap(),
            objects,
            &gix::progress::Discard,
            &gix::progress::Discard,
            &gix::interrupt::IS_INTERRUPTED,
            gix::worktree::state::checkout::Options::default(),
        )
        .unwrap();
    };

    // Initially, HEAD isn't set.
    work_dir
        .run_jj(["git", "init", "--git-repo", "."])
        .success();
    assert!(git_repo.head().unwrap().is_unborn());
    assert_eq!(
        git_repo.head_name().unwrap().unwrap().as_bstr(),
        b"refs/heads/master"
    );
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  230dd059e1b059aefc0da06a2e5a7dbf22362f22
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Stage some change, and check out root. This shouldn't clobber the HEAD.
    add_file_to_index("file0", "");
    let output = work_dir.run_jj(["new", "root()"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Working copy  (@) now at: kkmpptxz fcdbbd73 (empty) (no description set)
    Parent commit (@-)      : zzzzzzzz 00000000 (empty) (no description set)
    Added 0 files, modified 0 files, removed 1 files
    [EOF]
    ");
    assert!(git_repo.head().unwrap().is_unborn());
    assert_eq!(
        git_repo.head_name().unwrap().unwrap().as_bstr(),
        b"refs/heads/master"
    );
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  fcdbbd731496cae17161cd6be9b6cf1f759655a8
    │ ○  993600f1189571af5bbeb492cf657dc7d0fde48a
    ├─╯
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
    // Staged change shouldn't persist.
    checkout_index();
    insta::assert_snapshot!(work_dir.run_jj(["status"]), @r"
    The working copy has no changes.
    Working copy  (@) : kkmpptxz fcdbbd73 (empty) (no description set)
    Parent commit (@-): zzzzzzzz 00000000 (empty) (no description set)
    [EOF]
    ");

    // Stage some change, and create new HEAD. This shouldn't move the default
    // bookmark.
    add_file_to_index("file1", "");
    let output = work_dir.run_jj(["new"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Working copy  (@) now at: royxmykx 0e146103 (empty) (no description set)
    Parent commit (@-)      : kkmpptxz e3e01407 (no description set)
    [EOF]
    ");
    assert!(git_repo.head().unwrap().is_detached());
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"e3e01407bd3539722ae4ffff077700d97c60cb11"
    );
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  0e14610343ef50775f5c44db5aeef19aee45d9ad
    ○  e3e01407bd3539722ae4ffff077700d97c60cb11 git_head()
    │ ○  993600f1189571af5bbeb492cf657dc7d0fde48a
    ├─╯
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
    // Staged change shouldn't persist.
    checkout_index();
    insta::assert_snapshot!(work_dir.run_jj(["status"]), @r"
    The working copy has no changes.
    Working copy  (@) : royxmykx 0e146103 (empty) (no description set)
    Parent commit (@-): kkmpptxz e3e01407 (no description set)
    [EOF]
    ");

    // Assign the default bookmark. The bookmark is no longer "unborn".
    work_dir
        .run_jj(["bookmark", "create", "-r@-", "master"])
        .success();

    // Stage some change, and check out root again. This should unset the HEAD.
    // https://github.com/jj-vcs/jj/issues/1495
    add_file_to_index("file2", "");
    let output = work_dir.run_jj(["new", "root()"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Working copy  (@) now at: znkkpsqq 10dd328b (empty) (no description set)
    Parent commit (@-)      : zzzzzzzz 00000000 (empty) (no description set)
    Added 0 files, modified 0 files, removed 2 files
    [EOF]
    ");
    assert!(git_repo.head().unwrap().is_unborn());
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  10dd328bb906e15890e55047740eab2812a3b2f7
    │ ○  ef75c0b0dcc9b080e00226908c21316acaa84dc6
    │ ○  e3e01407bd3539722ae4ffff077700d97c60cb11 master
    ├─╯
    │ ○  993600f1189571af5bbeb492cf657dc7d0fde48a
    ├─╯
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
    // Staged change shouldn't persist.
    checkout_index();
    insta::assert_snapshot!(work_dir.run_jj(["status"]), @r"
    The working copy has no changes.
    Working copy  (@) : znkkpsqq 10dd328b (empty) (no description set)
    Parent commit (@-): zzzzzzzz 00000000 (empty) (no description set)
    [EOF]
    ");

    // New snapshot and commit can be created after the HEAD got unset.
    work_dir.write_file("file3", "");
    let output = work_dir.run_jj(["new"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Working copy  (@) now at: wqnwkozp 101e2723 (empty) (no description set)
    Parent commit (@-)      : znkkpsqq fc8af934 (no description set)
    [EOF]
    ");
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  101e272377a9daff75358f10dbd078df922fe68c
    ○  fc8af9345b0830dcb14716e04cd2af26e2d19f63 git_head()
    │ ○  ef75c0b0dcc9b080e00226908c21316acaa84dc6
    │ ○  e3e01407bd3539722ae4ffff077700d97c60cb11 master
    ├─╯
    │ ○  993600f1189571af5bbeb492cf657dc7d0fde48a
    ├─╯
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
}

#[test]
fn test_git_colocated_export_bookmarks_on_snapshot() {
    // Checks that we export bookmarks that were changed only because the working
    // copy was snapshotted

    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::init(work_dir.root());
    work_dir
        .run_jj(["git", "init", "--git-repo", "."])
        .success();

    // Create bookmark pointing to the initial commit
    work_dir.write_file("file", "initial");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "foo"])
        .success();
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  b15ef4cdd277d2c63cce6d67c1916f53a36141f7 foo
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // The bookmark gets updated when we modify the working copy, and it should get
    // exported to Git without requiring any other changes
    work_dir.write_file("file", "modified");
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  4d2c49a8f8e2f1ba61f48ba79e5f4a5faa6512cf foo
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
    insta::assert_snapshot!(git_repo
        .find_reference("refs/heads/foo")
        .unwrap()
        .id()
        .to_string(), @"4d2c49a8f8e2f1ba61f48ba79e5f4a5faa6512cf");
}

#[test]
fn test_git_colocated_rebase_on_import() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::init(work_dir.root());
    work_dir
        .run_jj(["git", "init", "--git-repo", "."])
        .success();

    // Make some changes in jj and check that they're reflected in git
    work_dir.write_file("file", "contents");
    work_dir.run_jj(["commit", "-m", "add a file"]).success();
    work_dir.write_file("file", "modified");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "master"])
        .success();
    work_dir.run_jj(["commit", "-m", "modify a file"]).success();
    // TODO: We shouldn't need this command here to trigger an import of the
    // refs/heads/master we just exported
    work_dir.run_jj(["st"]).success();

    // Move `master` backwards, which should result in commit2 getting hidden,
    // and the working-copy commit rebased.
    let parent_commit = git_repo
        .find_reference("refs/heads/master")
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .parent_ids()
        .next()
        .unwrap()
        .detach();
    git_repo
        .reference(
            "refs/heads/master",
            parent_commit,
            gix::refs::transaction::PreviousValue::Any,
            "update ref",
        )
        .unwrap();
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  15b1d70c5e33b5d2b18383292b85324d5153ffed
    ○  47fe984daf66f7bf3ebf31b9cb3513c995afb857 master git_head() add a file
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ------- stderr -------
    Abandoned 1 commits that are no longer reachable.
    Rebased 1 descendant commits off of commits rewritten from git
    Working copy  (@) now at: zsuskuln 15b1d70c (empty) (no description set)
    Parent commit (@-)      : qpvuntsm 47fe984d master | add a file
    Added 0 files, modified 1 files, removed 0 files
    Done importing changes from the underlying Git repo.
    [EOF]
    ");
}

#[test]
fn test_git_colocated_bookmarks() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::init(work_dir.root());
    work_dir
        .run_jj(["git", "init", "--git-repo", "."])
        .success();
    work_dir.run_jj(["new", "-m", "foo"]).success();
    work_dir.run_jj(["new", "@-", "-m", "bar"]).success();
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  3560559274ab431feea00b7b7e0b9250ecce951f bar
    │ ○  1e6f0b403ed2ff9713b5d6b1dc601e4804250cda foo
    ├─╯
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22 git_head()
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Create a bookmark in jj. It should be exported to Git even though it points
    // to the working- copy commit.
    work_dir
        .run_jj(["bookmark", "create", "-r@", "master"])
        .success();
    insta::assert_snapshot!(
        git_repo.find_reference("refs/heads/master").unwrap().target().id().to_string(),
        @"3560559274ab431feea00b7b7e0b9250ecce951f"
    );
    assert!(git_repo.head().unwrap().is_detached());
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"230dd059e1b059aefc0da06a2e5a7dbf22362f22"
    );

    // Update the bookmark in Git
    let target_id = work_dir
        .run_jj(["log", "--no-graph", "-T=commit_id", "-r=description(foo)"])
        .success()
        .stdout
        .into_raw();
    git_repo
        .reference(
            "refs/heads/master",
            gix::ObjectId::from_hex(target_id.as_bytes()).unwrap(),
            gix::refs::transaction::PreviousValue::Any,
            "test",
        )
        .unwrap();
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  096dc80da67094fbaa6683e2a205dddffa31f9a8
    │ ○  1e6f0b403ed2ff9713b5d6b1dc601e4804250cda master foo
    ├─╯
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22 git_head()
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ------- stderr -------
    Abandoned 1 commits that are no longer reachable.
    Working copy  (@) now at: yqosqzyt 096dc80d (empty) (no description set)
    Parent commit (@-)      : qpvuntsm 230dd059 (empty) (no description set)
    Done importing changes from the underlying Git repo.
    [EOF]
    ");
}

#[test]
fn test_git_colocated_bookmark_forget() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    git::init(work_dir.root());
    work_dir
        .run_jj(["git", "init", "--git-repo", "."])
        .success();
    work_dir.run_jj(["new"]).success();
    work_dir
        .run_jj(["bookmark", "create", "-r@", "foo"])
        .success();
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  65b6b74e08973b88d38404430f119c8c79465250 foo
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22 git_head()
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @r"
    foo: rlvkpnrz 65b6b74e (empty) (no description set)
      @git: rlvkpnrz 65b6b74e (empty) (no description set)
    [EOF]
    ");

    let output = work_dir.run_jj(["bookmark", "forget", "--include-remotes", "foo"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Forgot 1 local bookmarks.
    Forgot 1 remote bookmarks.
    [EOF]
    ");
    // A forgotten bookmark is deleted in the git repo. For a detailed demo
    // explaining this, see `test_bookmark_forget_export` in
    // `test_bookmark_command.rs`.
    insta::assert_snapshot!(get_bookmark_output(&work_dir), @"");
}

#[test]
fn test_git_colocated_bookmark_at_root() {
    let test_env = TestEnvironment::default();
    test_env
        .run_jj_in(".", ["git", "init", "--colocate", "repo"])
        .success();
    let work_dir = test_env.work_dir("repo");

    let output = work_dir.run_jj(["bookmark", "create", "foo", "-r=root()"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Created 1 bookmarks pointing to zzzzzzzz 00000000 foo | (empty) (no description set)
    Warning: Failed to export some bookmarks:
      foo@git: Ref cannot point to the root commit in Git
    [EOF]
    ");

    let output = work_dir.run_jj(["bookmark", "move", "foo", "--to=@"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Moved 1 bookmarks to qpvuntsm 230dd059 foo | (empty) (no description set)
    [EOF]
    ");

    let output = work_dir.run_jj([
        "bookmark",
        "move",
        "foo",
        "--allow-backwards",
        "--to=root()",
    ]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Moved 1 bookmarks to zzzzzzzz 00000000 foo* | (empty) (no description set)
    Warning: Failed to export some bookmarks:
      foo@git: Ref cannot point to the root commit in Git
    [EOF]
    ");
}

#[test]
fn test_git_colocated_conflicting_git_refs() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    git::init(work_dir.root());
    work_dir
        .run_jj(["git", "init", "--git-repo", "."])
        .success();
    work_dir
        .run_jj(["bookmark", "create", "-r@", "main"])
        .success();
    let output = work_dir.run_jj(["bookmark", "create", "-r@", "main/sub"]);
    insta::with_settings!({filters => vec![("Failed to set: .*", "Failed to set: ...")]}, {
        insta::assert_snapshot!(output, @r#"
        ------- stderr -------
        Created 1 bookmarks pointing to qpvuntsm 230dd059 main main/sub | (empty) (no description set)
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
fn test_git_colocated_checkout_non_empty_working_copy() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::init(work_dir.root());
    work_dir
        .run_jj(["git", "init", "--git-repo", "."])
        .success();

    // Create an initial commit in Git
    // We use this to set HEAD to master
    let tree_id = git::add_commit(
        &git_repo,
        "refs/heads/master",
        "file",
        b"contents",
        "initial",
        &[],
    )
    .tree_id;
    git::checkout_tree_index(&git_repo, tree_id);
    assert_eq!(work_dir.read_file("file"), b"contents");
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"97358f54806c7cd005ed5ade68a779595efbae7e"
    );

    work_dir.write_file("two", "y");

    work_dir.run_jj(["describe", "-m", "two"]).success();
    work_dir.run_jj(["new", "@-"]).success();
    let output = work_dir.run_jj(["describe", "-m", "new"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Working copy  (@) now at: kkmpptxz acea3383 (empty) new
    Parent commit (@-)      : slsumksp 97358f54 master | initial
    [EOF]
    ");

    assert_eq!(
        git_repo.head_name().unwrap().unwrap().as_bstr(),
        b"refs/heads/master"
    );

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  acea3383e7a9e4e0df035ee0e83d04cac44a3a14 new
    │ ○  a99003c2d01be21a82b33c0946c30c596e900287 two
    ├─╯
    ○  97358f54806c7cd005ed5ade68a779595efbae7e master git_head() initial
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
}

#[test]
fn test_git_colocated_fetch_deleted_or_moved_bookmark() {
    let test_env = TestEnvironment::default();
    test_env.add_config("git.auto-local-bookmark = true");
    let origin_dir = test_env.work_dir("origin");
    git::init(origin_dir.root());
    origin_dir.run_jj(["git", "init", "--git-repo=."]).success();
    origin_dir.run_jj(["describe", "-m=A"]).success();
    origin_dir
        .run_jj(["bookmark", "create", "-r@", "A"])
        .success();
    origin_dir.run_jj(["new", "-m=B_to_delete"]).success();
    origin_dir
        .run_jj(["bookmark", "create", "-r@", "B_to_delete"])
        .success();
    origin_dir.run_jj(["new", "-m=original C", "@-"]).success();
    origin_dir
        .run_jj(["bookmark", "create", "-r@", "C_to_move"])
        .success();

    let clone_dir = test_env.work_dir("clone");
    git::clone(clone_dir.root(), origin_dir.root().to_str().unwrap(), None);
    clone_dir.run_jj(["git", "init", "--git-repo=."]).success();
    clone_dir.run_jj(["new", "A"]).success();
    insta::assert_snapshot!(get_log_output(&clone_dir), @r"
    @  9c2de797c3c299a40173c5af724329012b77cbdd
    │ ○  4a191a9013d3f3398ccf5e172792a61439dbcf3a C_to_move original C
    ├─╯
    │ ○  c49ec4fb50844d0e693f1609da970b11878772ee B_to_delete B_to_delete
    ├─╯
    ◆  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 A git_head() A
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    origin_dir
        .run_jj(["bookmark", "delete", "B_to_delete"])
        .success();
    // Move bookmark C sideways
    origin_dir
        .run_jj(["describe", "C_to_move", "-m", "moved C"])
        .success();
    let output = clone_dir.run_jj(["git", "fetch"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    bookmark: B_to_delete@origin [deleted] untracked
    bookmark: C_to_move@origin   [updated] tracked
    Abandoned 2 commits that are no longer reachable.
    [EOF]
    ");
    // "original C" and "B_to_delete" are abandoned, as the corresponding bookmarks
    // were deleted or moved on the remote (#864)
    insta::assert_snapshot!(get_log_output(&clone_dir), @r"
    @  9c2de797c3c299a40173c5af724329012b77cbdd
    │ ○  4f3d13296f978cbc351c46a43b4619c91b888475 C_to_move moved C
    ├─╯
    ◆  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 A git_head() A
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
}

#[test]
fn test_git_colocated_rebase_dirty_working_copy() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::init(work_dir.root());
    work_dir.run_jj(["git", "init", "--git-repo=."]).success();

    work_dir.write_file("file", "base");
    work_dir.run_jj(["new"]).success();
    work_dir.write_file("file", "old");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "feature"])
        .success();

    // Make the working-copy dirty, delete the checked out bookmark.
    work_dir.write_file("file", "new");
    git_repo
        .find_reference("refs/heads/feature")
        .unwrap()
        .delete()
        .unwrap();

    // Because the working copy is dirty, the new working-copy commit will be
    // diverged. Therefore, the feature bookmark has change-delete conflict.
    let output = work_dir.run_jj(["status"]);
    insta::assert_snapshot!(output, @r"
    Working copy changes:
    M file
    Working copy  (@) : rlvkpnrz 6bad94b1 feature?? | (no description set)
    Parent commit (@-): qpvuntsm 3230d522 (no description set)
    Warning: These bookmarks have conflicts:
      feature
    Hint: Use `jj bookmark list` to see details. Use `jj bookmark set <name> -r <rev>` to resolve.
    [EOF]
    ------- stderr -------
    Warning: Failed to export some bookmarks:
      feature@git: Modified ref had been deleted in Git
    Done importing changes from the underlying Git repo.
    [EOF]
    ");
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  6bad94b10401f5fafc8a91064661224650d10d1b feature??
    ○  3230d52258f6de7e9afbd10da8d64503cc7cdca5 git_head()
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // The working-copy content shouldn't be lost.
    insta::assert_snapshot!(work_dir.read_file("file"), @"new");
}

#[test]
fn test_git_colocated_external_checkout() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::init(work_dir.root());
    let git_check_out_ref = |name| {
        let target = git_repo
            .find_reference(name)
            .unwrap()
            .into_fully_peeled_id()
            .unwrap()
            .detach();
        git::set_head_to_id(&git_repo, target);
    };

    work_dir.run_jj(["git", "init", "--git-repo=."]).success();
    work_dir.run_jj(["ci", "-m=A"]).success();
    work_dir
        .run_jj(["bookmark", "create", "-r@-", "master"])
        .success();
    work_dir.run_jj(["new", "-m=B", "root()"]).success();
    work_dir.run_jj(["new"]).success();

    // Checked out anonymous bookmark
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  f8a23336e41840ed1757ef323402a770427dc89a
    ○  eccedddfa5152d99fc8ddd1081b375387a8a382a git_head() B
    │ ○  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 master A
    ├─╯
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Check out another bookmark by external command
    git_check_out_ref("refs/heads/master");

    // The old working-copy commit gets abandoned, but the whole bookmark should not
    // be abandoned. (#1042)
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  8bb9e8d42a37c2a4e8dcfad97fce0b8f49bc7afa
    ○  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 master git_head() A
    │ ○  eccedddfa5152d99fc8ddd1081b375387a8a382a B
    ├─╯
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ------- stderr -------
    Reset the working copy parent to the new Git HEAD.
    [EOF]
    ");

    // Edit non-head commit
    work_dir.run_jj(["new", "description(B)"]).success();
    work_dir.run_jj(["new", "-m=C", "--no-edit"]).success();
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    ○  99a813753d6db988d8fc436b0d6b30a54d6b2707 C
    @  81e086b7f9b1dd7fde252e28bdcf4ba4abd86ce5
    ○  eccedddfa5152d99fc8ddd1081b375387a8a382a git_head() B
    │ ○  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 master A
    ├─╯
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Check out another bookmark by external command
    git_check_out_ref("refs/heads/master");

    // The old working-copy commit shouldn't be abandoned. (#3747)
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  ca2a4e32f08688c6fb795c4c034a0a7e09c0d804
    ○  a7e4cec4256b7995129b9d1e1bda7e1df6e60678 master git_head() A
    │ ○  99a813753d6db988d8fc436b0d6b30a54d6b2707 C
    │ ○  81e086b7f9b1dd7fde252e28bdcf4ba4abd86ce5
    │ ○  eccedddfa5152d99fc8ddd1081b375387a8a382a B
    ├─╯
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ------- stderr -------
    Reset the working copy parent to the new Git HEAD.
    [EOF]
    ");
}

#[test]
#[cfg_attr(windows, ignore = "uses POSIX sh")]
fn test_git_colocated_concurrent_checkout() {
    let test_env = TestEnvironment::default();
    test_env
        .run_jj_in(".", ["git", "init", "--colocate", "repo"])
        .success();
    let work_dir = test_env.work_dir("repo");

    work_dir.run_jj(["new", "-mcommit1"]).success();
    work_dir.write_file("file1", "");
    work_dir.run_jj(["new", "-mcommit2"]).success();
    work_dir.write_file("file2", "");
    work_dir.run_jj(["new", "-mcommit3"]).success();

    // Run "jj commit" and "git checkout" concurrently
    let output = work_dir.run_jj([
        "commit",
        "--config=ui.editor=['sh', '-c', 'git checkout -q HEAD^']",
    ]);
    insta::assert_snapshot!(output, @r#"
    ------- stderr -------
    Warning: Failed to update Git HEAD ref
    Caused by: The reference "HEAD" should have content 58a6206c70b53dfc30dc2f8c9e3713034cfc323e, actual content was 363a08cf5e683485227336e24a006e0deac341bc
    Working copy  (@) now at: mzvwutvl 6b3bc9c8 (empty) (no description set)
    Parent commit (@-)      : zsuskuln 7d358222 (empty) commit3
    [EOF]
    "#);

    // git_head() isn't updated because the export failed
    insta::assert_snapshot!(work_dir.run_jj(["log", "--summary", "--ignore-working-copy"]), @r"
    @  mzvwutvl test.user@example.com 2001-02-03 08:05:11 6b3bc9c8
    │  (empty) (no description set)
    ○  zsuskuln test.user@example.com 2001-02-03 08:05:11 7d358222
    │  (empty) commit3
    ○  kkmpptxz test.user@example.com 2001-02-03 08:05:10 git_head() 58a6206c
    │  commit2
    │  A file2
    ○  rlvkpnrz test.user@example.com 2001-02-03 08:05:09 363a08cf
    │  commit1
    │  A file1
    ○  qpvuntsm test.user@example.com 2001-02-03 08:05:07 230dd059
    │  (empty) (no description set)
    ◆  zzzzzzzz root() 00000000
    [EOF]
    ");

    // The current Git HEAD is imported on the next jj invocation
    insta::assert_snapshot!(work_dir.run_jj(["log", "--summary"]), @r"
    @  yqosqzyt test.user@example.com 2001-02-03 08:05:13 690bd924
    │  (empty) (no description set)
    │ ○  zsuskuln test.user@example.com 2001-02-03 08:05:11 7d358222
    │ │  (empty) commit3
    │ ○  kkmpptxz test.user@example.com 2001-02-03 08:05:10 58a6206c
    ├─╯  commit2
    │    A file2
    ○  rlvkpnrz test.user@example.com 2001-02-03 08:05:09 git_head() 363a08cf
    │  commit1
    │  A file1
    ○  qpvuntsm test.user@example.com 2001-02-03 08:05:07 230dd059
    │  (empty) (no description set)
    ◆  zzzzzzzz root() 00000000
    [EOF]
    ------- stderr -------
    Reset the working copy parent to the new Git HEAD.
    [EOF]
    ");
}

#[test]
fn test_git_colocated_squash_undo() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    git::init(work_dir.root());
    work_dir.run_jj(["git", "init", "--git-repo=."]).success();
    work_dir.run_jj(["ci", "-m=A"]).success();
    // Test the setup
    insta::assert_snapshot!(get_log_output_divergence(&work_dir), @r"
    @  rlvkpnrzqnoo 9670380ac379
    ○  qpvuntsmwlqt a7e4cec4256b A git_head()
    ◆  zzzzzzzzzzzz 000000000000
    [EOF]
    ");

    work_dir.run_jj(["squash"]).success();
    insta::assert_snapshot!(get_log_output_divergence(&work_dir), @r"
    @  zsuskulnrvyr 6ee662324e5a
    ○  qpvuntsmwlqt 13ab6b96d82e A git_head()
    ◆  zzzzzzzzzzzz 000000000000
    [EOF]
    ");
    work_dir.run_jj(["undo"]).success();
    // TODO: There should be no divergence here; 2f376ea1478c should be hidden
    // (#922)
    insta::assert_snapshot!(get_log_output_divergence(&work_dir), @r"
    @  rlvkpnrzqnoo 9670380ac379
    ○  qpvuntsmwlqt a7e4cec4256b A git_head()
    ◆  zzzzzzzzzzzz 000000000000
    [EOF]
    ");
}

#[test]
fn test_git_colocated_undo_head_move() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::init(work_dir.root());
    work_dir.run_jj(["git", "init", "--git-repo=."]).success();

    // Create new HEAD
    work_dir.run_jj(["new"]).success();
    assert!(git_repo.head().unwrap().is_detached());
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"230dd059e1b059aefc0da06a2e5a7dbf22362f22");
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  65b6b74e08973b88d38404430f119c8c79465250
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22 git_head()
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // HEAD should be unset
    work_dir.run_jj(["undo"]).success();
    assert!(git_repo.head().unwrap().is_unborn());
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  230dd059e1b059aefc0da06a2e5a7dbf22362f22
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Create commit on non-root commit
    work_dir.run_jj(["new"]).success();
    work_dir.run_jj(["new"]).success();
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  69b19f73cf584f162f078fb0d91c55ca39d10bc7
    ○  eb08b363bb5ef8ee549314260488980d7bbe8f63 git_head()
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
    assert!(git_repo.head().unwrap().is_detached());
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"eb08b363bb5ef8ee549314260488980d7bbe8f63");

    // HEAD should be moved back
    let output = work_dir.run_jj(["undo"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Undid operation: b50ec983d1c1 (2001-02-03 08:05:13) new empty commit
    Working copy  (@) now at: royxmykx eb08b363 (empty) (no description set)
    Parent commit (@-)      : qpvuntsm 230dd059 (empty) (no description set)
    [EOF]
    ");
    assert!(git_repo.head().unwrap().is_detached());
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"230dd059e1b059aefc0da06a2e5a7dbf22362f22");
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  eb08b363bb5ef8ee549314260488980d7bbe8f63
    ○  230dd059e1b059aefc0da06a2e5a7dbf22362f22 git_head()
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
}

#[test]
fn test_git_colocated_update_index_preserves_timestamps() {
    let test_env = TestEnvironment::default();
    test_env
        .run_jj_in(".", ["git", "init", "--colocate", "repo"])
        .success();
    let work_dir = test_env.work_dir("repo");

    // Create a commit with some files
    work_dir.write_file("file1.txt", "will be unchanged\n");
    work_dir.write_file("file2.txt", "will be modified\n");
    work_dir.write_file("file3.txt", "will be deleted\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "commit1"])
        .success();
    work_dir.run_jj(["new"]).success();

    // Create a commit with some changes to the files
    work_dir.write_file("file2.txt", "modified\n");
    work_dir.remove_file("file3.txt");
    work_dir.write_file("file4.txt", "added\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "commit2"])
        .success();
    work_dir.run_jj(["new"]).success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  051508d190ffd04fe2d79367ad8e9c3713ac2375
    ○  563dbc583c0d82eb10c40d3f3276183ea28a0fa7 commit2 git_head()
    ○  3c270b473dd871b20d196316eb038f078f80c219 commit1
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) ed48318d9bf4 ctime=0:0 mtime=0:0 size=0 flags=0 file1.txt
    Unconflicted Mode(FILE) 2e0996000b7e ctime=0:0 mtime=0:0 size=0 flags=0 file2.txt
    Unconflicted Mode(FILE) d5f7fc3f74f7 ctime=0:0 mtime=0:0 size=0 flags=0 file4.txt
    ");

    // Update index with stats for all files. We may want to do this automatically
    // in the future after we update the index in `git::reset_head` (#3786), but for
    // now, we at least want to preserve existing stat information when possible.
    update_git_index(work_dir.root());

    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) ed48318d9bf4 ctime=[nonzero] mtime=[nonzero] size=18 flags=0 file1.txt
    Unconflicted Mode(FILE) 2e0996000b7e ctime=[nonzero] mtime=[nonzero] size=9 flags=0 file2.txt
    Unconflicted Mode(FILE) d5f7fc3f74f7 ctime=[nonzero] mtime=[nonzero] size=6 flags=0 file4.txt
    ");

    // Edit parent commit, causing the changes to be removed from the index without
    // touching the working copy
    work_dir.run_jj(["edit", "commit2"]).success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  563dbc583c0d82eb10c40d3f3276183ea28a0fa7 commit2
    ○  3c270b473dd871b20d196316eb038f078f80c219 commit1 git_head()
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Index should contain stat for unchanged file still.
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) ed48318d9bf4 ctime=[nonzero] mtime=[nonzero] size=18 flags=0 file1.txt
    Unconflicted Mode(FILE) 28d2718c947b ctime=0:0 mtime=0:0 size=0 flags=0 file2.txt
    Unconflicted Mode(FILE) 528557ab3a42 ctime=0:0 mtime=0:0 size=0 flags=0 file3.txt
    Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 file4.txt
    ");

    // Create sibling commit, causing working copy to match index
    work_dir.run_jj(["new", "commit1"]).success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  ccb1b1807383dba5ff4d335fd9fb92aa540f4632
    │ ○  563dbc583c0d82eb10c40d3f3276183ea28a0fa7 commit2
    ├─╯
    ○  3c270b473dd871b20d196316eb038f078f80c219 commit1 git_head()
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Index should contain stat for unchanged file still.
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) ed48318d9bf4 ctime=[nonzero] mtime=[nonzero] size=18 flags=0 file1.txt
    Unconflicted Mode(FILE) 28d2718c947b ctime=0:0 mtime=0:0 size=0 flags=0 file2.txt
    Unconflicted Mode(FILE) 528557ab3a42 ctime=0:0 mtime=0:0 size=0 flags=0 file3.txt
    ");
}

#[test]
fn test_git_colocated_update_index_merge_conflict() {
    let test_env = TestEnvironment::default();
    test_env
        .run_jj_in(".", ["git", "init", "--colocate", "repo"])
        .success();
    let work_dir = test_env.work_dir("repo");

    // Set up conflict files
    work_dir.write_file("conflict.txt", "base\n");
    work_dir.write_file("base.txt", "base\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "base"])
        .success();

    work_dir.run_jj(["new", "base"]).success();
    work_dir.write_file("conflict.txt", "left\n");
    work_dir.write_file("left.txt", "left\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "left"])
        .success();

    work_dir.run_jj(["new", "base"]).success();
    work_dir.write_file("conflict.txt", "right\n");
    work_dir.write_file("right.txt", "right\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "right"])
        .success();

    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=0 base.txt
    Unconflicted Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=0 conflict.txt
    Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 right.txt
    ");

    // Update index with stat for base.txt
    update_git_index(work_dir.root());

    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) df967b96a579 ctime=[nonzero] mtime=[nonzero] size=5 flags=0 base.txt
    Unconflicted Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=0 conflict.txt
    Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 right.txt
    ");

    // Create merge conflict
    work_dir.run_jj(["new", "left", "right"]).success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @    aea7acd77752c3f74914de1fe327075a579bf7c6
    ├─╮
    │ ○  df62ad35fc873e89ade730fa9a407cd5cfa5e6ba right
    ○ │  68cc2177623364e4f0719d6ec8da1d6ea8d6087e left git_head()
    ├─╯
    ○  14b3ff6c73a234ab2a26fc559512e0f056a46bd9 base
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Conflict should be added in index with correct blob IDs. The stat for
    // base.txt should not change.
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) df967b96a579 ctime=[nonzero] mtime=[nonzero] size=5 flags=0 base.txt
    Base         Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=1000 conflict.txt
    Ours         Mode(FILE) 45cf141ba67d ctime=0:0 mtime=0:0 size=0 flags=2000 conflict.txt
    Theirs       Mode(FILE) c376d892e8b1 ctime=0:0 mtime=0:0 size=0 flags=3000 conflict.txt
    Unconflicted Mode(FILE) 45cf141ba67d ctime=0:0 mtime=0:0 size=0 flags=0 left.txt
    Unconflicted Mode(FILE) c376d892e8b1 ctime=0:0 mtime=0:0 size=0 flags=0 right.txt
    ");

    work_dir.run_jj(["new"]).success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  cae33b49a8a514996983caaf171c5edbf0d70e78
    ×    aea7acd77752c3f74914de1fe327075a579bf7c6 git_head()
    ├─╮
    │ ○  df62ad35fc873e89ade730fa9a407cd5cfa5e6ba right
    ○ │  68cc2177623364e4f0719d6ec8da1d6ea8d6087e left
    ├─╯
    ○  14b3ff6c73a234ab2a26fc559512e0f056a46bd9 base
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Index should be the same after `jj new`.
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) df967b96a579 ctime=[nonzero] mtime=[nonzero] size=5 flags=0 base.txt
    Base         Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=1000 conflict.txt
    Ours         Mode(FILE) 45cf141ba67d ctime=0:0 mtime=0:0 size=0 flags=2000 conflict.txt
    Theirs       Mode(FILE) c376d892e8b1 ctime=0:0 mtime=0:0 size=0 flags=3000 conflict.txt
    Unconflicted Mode(FILE) 45cf141ba67d ctime=0:0 mtime=0:0 size=0 flags=0 left.txt
    Unconflicted Mode(FILE) c376d892e8b1 ctime=0:0 mtime=0:0 size=0 flags=0 right.txt
    ");
}

#[test]
fn test_git_colocated_update_index_rebase_conflict() {
    let test_env = TestEnvironment::default();
    test_env
        .run_jj_in(".", ["git", "init", "--colocate", "repo"])
        .success();
    let work_dir = test_env.work_dir("repo");

    // Set up conflict files
    work_dir.write_file("conflict.txt", "base\n");
    work_dir.write_file("base.txt", "base\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "base"])
        .success();

    work_dir.run_jj(["new", "base"]).success();
    work_dir.write_file("conflict.txt", "left\n");
    work_dir.write_file("left.txt", "left\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "left"])
        .success();

    work_dir.run_jj(["new", "base"]).success();
    work_dir.write_file("conflict.txt", "right\n");
    work_dir.write_file("right.txt", "right\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "right"])
        .success();

    work_dir.run_jj(["edit", "left"]).success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  68cc2177623364e4f0719d6ec8da1d6ea8d6087e left
    │ ○  df62ad35fc873e89ade730fa9a407cd5cfa5e6ba right
    ├─╯
    ○  14b3ff6c73a234ab2a26fc559512e0f056a46bd9 base git_head()
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=0 base.txt
    Unconflicted Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=0 conflict.txt
    Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 left.txt
    ");

    // Update index with stat for base.txt
    update_git_index(work_dir.root());

    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) df967b96a579 ctime=[nonzero] mtime=[nonzero] size=5 flags=0 base.txt
    Unconflicted Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=0 conflict.txt
    Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 left.txt
    ");

    // Create rebase conflict
    work_dir
        .run_jj(["rebase", "-r", "left", "-d", "right"])
        .success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  233cb41e128e74aa2fcbf01c85d69b33a118faa8 left
    ○  df62ad35fc873e89ade730fa9a407cd5cfa5e6ba right git_head()
    ○  14b3ff6c73a234ab2a26fc559512e0f056a46bd9 base
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Index should contain files from parent commit, so there should be no conflict
    // in conflict.txt yet. The stat for base.txt should not change.
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) df967b96a579 ctime=[nonzero] mtime=[nonzero] size=5 flags=0 base.txt
    Unconflicted Mode(FILE) c376d892e8b1 ctime=0:0 mtime=0:0 size=0 flags=0 conflict.txt
    Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 left.txt
    Unconflicted Mode(FILE) c376d892e8b1 ctime=0:0 mtime=0:0 size=0 flags=0 right.txt
    ");

    work_dir.run_jj(["new"]).success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  6d84b9021f9e07b69770687071c4e8e71113e688
    ×  233cb41e128e74aa2fcbf01c85d69b33a118faa8 left git_head()
    ○  df62ad35fc873e89ade730fa9a407cd5cfa5e6ba right
    ○  14b3ff6c73a234ab2a26fc559512e0f056a46bd9 base
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Now the working copy commit's parent is conflicted, so the index should have
    // a conflict with correct blob IDs.
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) df967b96a579 ctime=[nonzero] mtime=[nonzero] size=5 flags=0 base.txt
    Base         Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=1000 conflict.txt
    Ours         Mode(FILE) c376d892e8b1 ctime=0:0 mtime=0:0 size=0 flags=2000 conflict.txt
    Theirs       Mode(FILE) 45cf141ba67d ctime=0:0 mtime=0:0 size=0 flags=3000 conflict.txt
    Unconflicted Mode(FILE) 45cf141ba67d ctime=0:0 mtime=0:0 size=0 flags=0 left.txt
    Unconflicted Mode(FILE) c376d892e8b1 ctime=0:0 mtime=0:0 size=0 flags=0 right.txt
    ");
}

#[test]
fn test_git_colocated_update_index_3_sided_conflict() {
    let test_env = TestEnvironment::default();
    test_env
        .run_jj_in(".", ["git", "init", "--colocate", "repo"])
        .success();
    let work_dir = test_env.work_dir("repo");

    // Set up conflict files
    work_dir.write_file("conflict.txt", "base\n");
    work_dir.write_file("base.txt", "base\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "base"])
        .success();

    work_dir.run_jj(["new", "base"]).success();
    work_dir.write_file("conflict.txt", "side-1\n");
    work_dir.write_file("side-1.txt", "side-1\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "side-1"])
        .success();

    work_dir.run_jj(["new", "base"]).success();
    work_dir.write_file("conflict.txt", "side-2\n");
    work_dir.write_file("side-2.txt", "side-2\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "side-2"])
        .success();

    work_dir.run_jj(["new", "base"]).success();
    work_dir.write_file("conflict.txt", "side-3\n");
    work_dir.write_file("side-3.txt", "side-3\n");
    work_dir
        .run_jj(["bookmark", "create", "-r@", "side-3"])
        .success();

    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=0 base.txt
    Unconflicted Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=0 conflict.txt
    Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 side-3.txt
    ");

    // Update index with stat for base.txt
    update_git_index(work_dir.root());

    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) df967b96a579 ctime=[nonzero] mtime=[nonzero] size=5 flags=0 base.txt
    Unconflicted Mode(FILE) df967b96a579 ctime=0:0 mtime=0:0 size=0 flags=0 conflict.txt
    Unconflicted Mode(FILE) e69de29bb2d1 ctime=0:0 mtime=0:0 size=0 flags=20004000 side-3.txt
    ");

    // Create 3-sided merge conflict
    work_dir
        .run_jj(["new", "side-1", "side-2", "side-3"])
        .success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @      faee07ad76218d193f2784f4988daa2ac46db30c
    ├─┬─╮
    │ │ ○  86e722ea6a9da2551f1e05bc9aa914acd1cb2304 side-3
    │ ○ │  b8b9ca2d8178c4ba727a61e2258603f30ac7c6d3 side-2
    │ ├─╯
    ○ │  a4b3ce25ef4857172e7777567afd497a917a0486 side-1 git_head()
    ├─╯
    ○  14b3ff6c73a234ab2a26fc559512e0f056a46bd9 base
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // We can't add conflicts with more than 2 sides to the index, so we add a dummy
    // conflict instead. The stat for base.txt should not change.
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Ours         Mode(FILE) eb8299123d2a ctime=0:0 mtime=0:0 size=0 flags=2000 .jj-do-not-resolve-this-conflict
    Unconflicted Mode(FILE) df967b96a579 ctime=[nonzero] mtime=[nonzero] size=5 flags=0 base.txt
    Unconflicted Mode(FILE) dd8f930010b3 ctime=0:0 mtime=0:0 size=0 flags=0 conflict.txt
    Unconflicted Mode(FILE) dd8f930010b3 ctime=0:0 mtime=0:0 size=0 flags=0 side-1.txt
    Unconflicted Mode(FILE) 7b44e11df720 ctime=0:0 mtime=0:0 size=0 flags=0 side-2.txt
    Unconflicted Mode(FILE) 42f37a71bf20 ctime=0:0 mtime=0:0 size=0 flags=0 side-3.txt
    ");

    work_dir.run_jj(["new"]).success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  b0e5644063c2a12fb265e5f65cd88c6a2e1cf865
    ×      faee07ad76218d193f2784f4988daa2ac46db30c git_head()
    ├─┬─╮
    │ │ ○  86e722ea6a9da2551f1e05bc9aa914acd1cb2304 side-3
    │ ○ │  b8b9ca2d8178c4ba727a61e2258603f30ac7c6d3 side-2
    │ ├─╯
    ○ │  a4b3ce25ef4857172e7777567afd497a917a0486 side-1
    ├─╯
    ○  14b3ff6c73a234ab2a26fc559512e0f056a46bd9 base
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");

    // Index should be the same after `jj new`.
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Ours         Mode(FILE) eb8299123d2a ctime=0:0 mtime=0:0 size=0 flags=2000 .jj-do-not-resolve-this-conflict
    Unconflicted Mode(FILE) df967b96a579 ctime=[nonzero] mtime=[nonzero] size=5 flags=0 base.txt
    Unconflicted Mode(FILE) dd8f930010b3 ctime=0:0 mtime=0:0 size=0 flags=0 conflict.txt
    Unconflicted Mode(FILE) dd8f930010b3 ctime=0:0 mtime=0:0 size=0 flags=0 side-1.txt
    Unconflicted Mode(FILE) 7b44e11df720 ctime=0:0 mtime=0:0 size=0 flags=0 side-2.txt
    Unconflicted Mode(FILE) 42f37a71bf20 ctime=0:0 mtime=0:0 size=0 flags=0 side-3.txt
    ");

    // If we add a file named ".jj-do-not-resolve-this-conflict", it should take
    // precedence over the dummy conflict.
    work_dir.write_file(".jj-do-not-resolve-this-conflict", "file\n");
    work_dir.run_jj(["new"]).success();
    insta::assert_snapshot!(get_index_state(work_dir.root()), @r"
    Unconflicted Mode(FILE) f73f3093ff86 ctime=0:0 mtime=0:0 size=0 flags=0 .jj-do-not-resolve-this-conflict
    Unconflicted Mode(FILE) df967b96a579 ctime=[nonzero] mtime=[nonzero] size=5 flags=0 base.txt
    Unconflicted Mode(FILE) dd8f930010b3 ctime=0:0 mtime=0:0 size=0 flags=0 conflict.txt
    Unconflicted Mode(FILE) dd8f930010b3 ctime=0:0 mtime=0:0 size=0 flags=0 side-1.txt
    Unconflicted Mode(FILE) 7b44e11df720 ctime=0:0 mtime=0:0 size=0 flags=0 side-2.txt
    Unconflicted Mode(FILE) 42f37a71bf20 ctime=0:0 mtime=0:0 size=0 flags=0 side-3.txt
    ");
}

#[must_use]
fn get_log_output_divergence(work_dir: &TestWorkDir) -> CommandOutput {
    let template = r#"
    separate(" ",
      change_id.short(),
      commit_id.short(),
      description.first_line(),
      bookmarks,
      if(git_head, "git_head()"),
      if(divergent, "!divergence!"),
    )
    "#;
    work_dir.run_jj(["log", "-T", template])
}

#[must_use]
fn get_log_output(work_dir: &TestWorkDir) -> CommandOutput {
    let template = r#"
    separate(" ",
      commit_id,
      bookmarks,
      if(git_head, "git_head()"),
      description,
    )
    "#;
    work_dir.run_jj(["log", "-T", template, "-r=all()"])
}

fn update_git_index(repo_path: &Path) {
    let mut iter = git::open(repo_path)
        .status(gix::progress::Discard)
        .unwrap()
        .into_index_worktree_iter(None)
        .unwrap();

    // need to explicitly iterate over the changes to recreate the index

    for item in iter.by_ref() {
        item.unwrap();
    }

    iter.outcome_mut()
        .unwrap()
        .write_changes()
        .unwrap()
        .unwrap();
}

fn get_index_state(repo_path: &Path) -> String {
    let git_repo = gix::open(repo_path).expect("git repo should exist");
    let mut buffer = String::new();
    // We can't use the real time from disk, since it would change each time the
    // tests are run. Instead, we just show whether it's zero or nonzero.
    let format_time = |time: gix::index::entry::stat::Time| {
        if time.secs == 0 && time.nsecs == 0 {
            "0:0"
        } else {
            "[nonzero]"
        }
    };
    let index = git_repo.index_or_empty().unwrap();
    for entry in index.entries() {
        writeln!(
            &mut buffer,
            "{:12} {:?} {} ctime={} mtime={} size={} flags={:x} {}",
            format!("{:?}", entry.stage()),
            entry.mode,
            entry.id.to_hex_with_len(12),
            format_time(entry.stat.ctime),
            format_time(entry.stat.mtime),
            entry.stat.size,
            entry.flags.bits(),
            entry.path_in(index.path_backing()),
        )
        .unwrap();
    }
    buffer
}

#[test]
fn test_git_colocated_unreachable_commits() {
    let test_env = TestEnvironment::default();
    let work_dir = test_env.work_dir("repo");
    let git_repo = git::init(work_dir.root());

    // Create an initial commit in Git
    let commit1 = git::add_commit(
        &git_repo,
        "refs/heads/master",
        "some-file",
        b"some content",
        "initial",
        &[],
    )
    .commit_id;
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"cd740e230992f334de13a0bd0b35709b3f7a89af"
    );

    // Add a second commit in Git
    let commit2 = git::add_commit(
        &git_repo,
        "refs/heads/dummy",
        "next-file",
        b"more content",
        "next",
        &[commit1],
    )
    .commit_id;
    git_repo
        .find_reference("refs/heads/dummy")
        .unwrap()
        .delete()
        .unwrap();
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"cd740e230992f334de13a0bd0b35709b3f7a89af"
    );

    // Import the repo while there is no path to the second commit
    work_dir
        .run_jj(["git", "init", "--git-repo", "."])
        .success();
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  9ff88424a06a94d04738847733e68e510b906345
    ○  cd740e230992f334de13a0bd0b35709b3f7a89af master git_head() initial
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ");
    insta::assert_snapshot!(
        git_repo.head_id().unwrap().to_string(),
        @"cd740e230992f334de13a0bd0b35709b3f7a89af"
    );

    // Check that trying to look up the second commit fails gracefully
    let output = work_dir.run_jj(["show", &commit2.to_string()]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Error: Revision `b23bb53bdce25f0e03ff9e484eadb77626256041` doesn't exist
    [EOF]
    [exit status: 1]
    ");
}

#[test]
fn test_git_colocated_operation_cleanup() {
    let test_env = TestEnvironment::default();
    let output = test_env.run_jj_in(".", ["git", "init", "--colocate", "repo"]);
    insta::assert_snapshot!(output, @r#"
    ------- stderr -------
    Initialized repo in "repo"
    [EOF]
    "#);

    let work_dir = test_env.work_dir("repo");

    work_dir.write_file("file", "1");
    work_dir.run_jj(["describe", "-m1"]).success();
    work_dir.run_jj(["new"]).success();

    work_dir.write_file("file", "2");
    work_dir.run_jj(["describe", "-m2"]).success();
    work_dir
        .run_jj(["bookmark", "create", "-r@", "main"])
        .success();
    work_dir.run_jj(["new", "root()+"]).success();

    work_dir.write_file("file", "3");
    work_dir.run_jj(["describe", "-m3"]).success();
    work_dir
        .run_jj(["bookmark", "create", "-r@", "feature"])
        .success();
    work_dir.run_jj(["new"]).success();

    insta::assert_snapshot!(get_log_output(&work_dir), @r#"
    @  e3feb4fda7b5e1d458a460ce76cb840b8f3cae34
    ○  e810c2ff6f3287a27e5d08aa3f429e284d99fea0 feature git_head() 3
    │ ○  52fef888179abf5819a0a0d4f7907fcc025cb2a1 main 2
    ├─╯
    ○  61c11921948922575504d7b9f2df236543d0cec9 1
    ◆  0000000000000000000000000000000000000000
    [EOF]
    "#);

    // Start a rebase in Git and expect a merge conflict.
    let output = std::process::Command::new("git")
        .current_dir(work_dir.root())
        .args(["rebase", "main"])
        .output()
        .unwrap();
    assert!(!output.status.success());

    // Check that we’re in the middle of a conflicted rebase.
    assert!(std::fs::exists(work_dir.root().join(".git").join("rebase-merge")).unwrap());
    let output = std::process::Command::new("git")
        .current_dir(work_dir.root())
        .args(["status", "--porcelain=v1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @r#"
    UU file
    "#);
    insta::assert_snapshot!(get_log_output(&work_dir), @r#"
    @  fbb4e341d1e7e1d3b87377c075bd8a407305ba3a
    ○  52fef888179abf5819a0a0d4f7907fcc025cb2a1 main git_head() 2
    │ ○  e810c2ff6f3287a27e5d08aa3f429e284d99fea0 feature 3
    ├─╯
    ○  61c11921948922575504d7b9f2df236543d0cec9 1
    ◆  0000000000000000000000000000000000000000
    [EOF]
    ------- stderr -------
    Reset the working copy parent to the new Git HEAD.
    [EOF]
    "#);

    // Reset the Git HEAD with Jujutsu.
    let output = work_dir.run_jj(["new", "main"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    Working copy  (@) now at: kmkuslsw 92667528 (empty) (no description set)
    Parent commit (@-)      : kkmpptxz 52fef888 main | 2
    Added 0 files, modified 1 files, removed 0 files
    [EOF]
    ");
    insta::assert_snapshot!(get_log_output(&work_dir), @r#"
    @  926675286938f585d83b3646a95df96206968e8c
    │ ○  fbb4e341d1e7e1d3b87377c075bd8a407305ba3a
    ├─╯
    ○  52fef888179abf5819a0a0d4f7907fcc025cb2a1 main git_head() 2
    │ ○  e810c2ff6f3287a27e5d08aa3f429e284d99fea0 feature 3
    ├─╯
    ○  61c11921948922575504d7b9f2df236543d0cec9 1
    ◆  0000000000000000000000000000000000000000
    [EOF]
    "#);

    // Check that the operation was correctly aborted.
    assert!(!std::fs::exists(work_dir.root().join(".git").join("rebase-merge")).unwrap());
    let output = std::process::Command::new("git")
        .current_dir(work_dir.root())
        .args(["status", "--porcelain=v1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap(), @"");
}

#[must_use]
fn get_bookmark_output(work_dir: &TestWorkDir) -> CommandOutput {
    // --quiet to suppress deleted bookmarks hint
    work_dir.run_jj(["bookmark", "list", "--all-remotes", "--quiet"])
}
