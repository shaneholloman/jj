// Copyright 2025 The Jujutsu Authors
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

use crate::common::create_commit_with_files;
use crate::common::CommandOutput;
use crate::common::TestEnvironment;
use crate::common::TestWorkDir;

#[test]
fn test_revert() {
    let test_env = TestEnvironment::default();
    test_env.run_jj_in(".", ["git", "init", "repo"]).success();
    let work_dir = test_env.work_dir("repo");

    create_commit_with_files(&work_dir, "a", &[], &[("a", "a\n")]);
    create_commit_with_files(&work_dir, "b", &["a"], &[]);
    create_commit_with_files(&work_dir, "c", &["b"], &[]);
    create_commit_with_files(&work_dir, "d", &["c"], &[]);
    // Test the setup
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  cf3ddd838fd1 d
    ○  14e954305d4b c
    ○  aa48676d4a49 b
    ○  2443ea76b0b1 a
    ◆  000000000000
    [EOF]
    ");
    let output = work_dir.run_jj(["diff", "-ra", "-s"]);
    insta::assert_snapshot!(output, @r"
    A a
    [EOF]
    ");
    let setup_opid = work_dir.current_operation_id();

    // Reverting without a location is an error
    let output = work_dir.run_jj(["revert", "-ra"]);
    insta::assert_snapshot!(output, @r"
    ------- stderr -------
    error: the following required arguments were not provided:
      <--destination <REVSETS>|--insert-after <REVSETS>|--insert-before <REVSETS>>

    Usage: jj revert --revisions <REVSETS> <--destination <REVSETS>|--insert-after <REVSETS>|--insert-before <REVSETS>>

    For more information, try '--help'.
    [EOF]
    [exit status: 2]
    ");

    // Revert the commit with `--destination`
    let output = work_dir.run_jj(["revert", "-ra", "-d@"]);
    insta::assert_snapshot!(output, @r#"
    ------- stderr -------
    Reverted 1 commits as follows:
      lylxulpl f844336e Revert "a"
    [EOF]
    "#);
    insta::assert_snapshot!(get_log_output(&work_dir), @r#"
    ○  f844336ef2a5 Revert "a"
    │
    │  This reverts commit 2443ea76b0b1c531326908326aab7020abab8e6c.
    @  cf3ddd838fd1 d
    ○  14e954305d4b c
    ○  aa48676d4a49 b
    ○  2443ea76b0b1 a
    ◆  000000000000
    [EOF]
    "#);
    let output = work_dir.run_jj(["diff", "-s", "-r@+"]);
    insta::assert_snapshot!(output, @r"
    D a
    [EOF]
    ");

    // Revert the new reverted commit
    let output = work_dir.run_jj(["revert", "-r@+", "-d@+"]);
    insta::assert_snapshot!(output, @r#"
    ------- stderr -------
    Reverted 1 commits as follows:
      uyznsvlq 57fb197b Revert "Revert "a""
    [EOF]
    "#);
    insta::assert_snapshot!(get_log_output(&work_dir), @r#"
    ○  57fb197b953f Revert "Revert "a""
    │
    │  This reverts commit f844336ef2a54f4499a5efefa1a9549451276316.
    ○  f844336ef2a5 Revert "a"
    │
    │  This reverts commit 2443ea76b0b1c531326908326aab7020abab8e6c.
    @  cf3ddd838fd1 d
    ○  14e954305d4b c
    ○  aa48676d4a49 b
    ○  2443ea76b0b1 a
    ◆  000000000000
    [EOF]
    "#);
    let output = work_dir.run_jj(["diff", "-s", "-r@++"]);
    insta::assert_snapshot!(output, @r"
    A a
    [EOF]
    ");
    work_dir.run_jj(["op", "restore", &setup_opid]).success();

    // Revert the commit with `--insert-after`
    let output = work_dir.run_jj(["revert", "-ra", "-Ab"]);
    insta::assert_snapshot!(output, @r#"
    ------- stderr -------
    Reverted 1 commits as follows:
      tlkvzzqu ff67400f Revert "a"
    Rebased 2 descendant commits
    Working copy  (@) now at: vruxwmqv 37ad0315 d | (empty) d
    Parent commit (@-)      : royxmykx ca80e93f c | (empty) c
    Added 0 files, modified 0 files, removed 1 files
    [EOF]
    "#);
    insta::assert_snapshot!(get_log_output(&work_dir), @r#"
    @  37ad03151aa7 d
    ○  ca80e93fdef9 c
    ○  ff67400f3e1f Revert "a"
    │
    │  This reverts commit 2443ea76b0b1c531326908326aab7020abab8e6c.
    ○  aa48676d4a49 b
    ○  2443ea76b0b1 a
    ◆  000000000000
    [EOF]
    "#);
    let output = work_dir.run_jj(["diff", "-s", "-rb+"]);
    insta::assert_snapshot!(output, @r"
    D a
    [EOF]
    ");
    work_dir.run_jj(["op", "restore", &setup_opid]).success();

    // Revert the commit with `--insert-before`
    let output = work_dir.run_jj(["revert", "-ra", "-Bd"]);
    insta::assert_snapshot!(output, @r#"
    ------- stderr -------
    Reverted 1 commits as follows:
      xlzxqlsl 0b91fe42 Revert "a"
    Rebased 1 descendant commits
    Working copy  (@) now at: vruxwmqv 3a8a8069 d | (empty) d
    Parent commit (@-)      : xlzxqlsl 0b91fe42 Revert "a"
    Added 0 files, modified 0 files, removed 1 files
    [EOF]
    "#);
    insta::assert_snapshot!(get_log_output(&work_dir), @r#"
    @  3a8a80692ac3 d
    ○  0b91fe42616d Revert "a"
    │
    │  This reverts commit 2443ea76b0b1c531326908326aab7020abab8e6c.
    ○  14e954305d4b c
    ○  aa48676d4a49 b
    ○  2443ea76b0b1 a
    ◆  000000000000
    [EOF]
    "#);
    let output = work_dir.run_jj(["diff", "-s", "-rd-"]);
    insta::assert_snapshot!(output, @r"
    D a
    [EOF]
    ");
    work_dir.run_jj(["op", "restore", &setup_opid]).success();

    // Revert the commit with `--insert-after` and `--insert-before`
    let output = work_dir.run_jj(["revert", "-ra", "-Aa", "-Bd"]);
    insta::assert_snapshot!(output, @r#"
    ------- stderr -------
    Reverted 1 commits as follows:
      pkstwlsy 29508aef Revert "a"
    Rebased 1 descendant commits
    Working copy  (@) now at: vruxwmqv 3489c981 d | (empty) d
    Parent commit (@-)      : royxmykx 14e95430 c | (empty) c
    Parent commit (@-)      : pkstwlsy 29508aef Revert "a"
    Added 0 files, modified 0 files, removed 1 files
    [EOF]
    "#);
    insta::assert_snapshot!(get_log_output(&work_dir), @r#"
    @    3489c98177aa d
    ├─╮
    │ ○  29508aefc220 Revert "a"
    │ │
    │ │  This reverts commit 2443ea76b0b1c531326908326aab7020abab8e6c.
    ○ │  14e954305d4b c
    ○ │  aa48676d4a49 b
    ├─╯
    ○  2443ea76b0b1 a
    ◆  000000000000
    [EOF]
    "#);
    let output = work_dir.run_jj(["diff", "-s", "-r", "a+ & d-"]);
    insta::assert_snapshot!(output, @r"
    D a
    [EOF]
    ");
    work_dir.run_jj(["op", "restore", &setup_opid]).success();
}

#[test]
fn test_revert_multiple() {
    let test_env = TestEnvironment::default();
    test_env.run_jj_in(".", ["git", "init", "repo"]).success();
    let work_dir = test_env.work_dir("repo");

    create_commit_with_files(&work_dir, "a", &[], &[("a", "a\n")]);
    create_commit_with_files(&work_dir, "b", &["a"], &[("a", "a\nb\n")]);
    create_commit_with_files(&work_dir, "c", &["b"], &[("a", "a\nb\n"), ("b", "b\n")]);
    create_commit_with_files(&work_dir, "d", &["c"], &[]);
    create_commit_with_files(&work_dir, "e", &["d"], &[("a", "a\nb\nc\n")]);

    // Test the setup
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  208f8612074a e
    ○  ceeec03be46b d
    ○  413337bbd11f c
    ○  46cc97af6802 b
    ○  2443ea76b0b1 a
    ◆  000000000000
    [EOF]
    ");

    // Revert multiple commits
    let output = work_dir.run_jj(["revert", "-rb", "-rc", "-re", "-d@"]);
    insta::assert_snapshot!(output, @r#"
    ------- stderr -------
    Reverted 3 commits as follows:
      wqnwkozp 05f5fa79 Revert "e"
      mouksmqu f5d9e8b2 Revert "c"
      tqvpomtp fb78f44d Revert "b"
    [EOF]
    "#);
    insta::assert_snapshot!(get_log_output(&work_dir), @r#"
    ○  fb78f44decd2 Revert "b"
    │
    │  This reverts commit 46cc97af6802301d8db381386e8485ff3ff24ae6.
    ○  f5d9e8b20bd1 Revert "c"
    │
    │  This reverts commit 413337bbd11f7a6636c010d9e196acf801d8df2f.
    ○  05f5fa79161a Revert "e"
    │
    │  This reverts commit 208f8612074af4c219d06568a8e1f04f2e80dc25.
    @  208f8612074a e
    ○  ceeec03be46b d
    ○  413337bbd11f c
    ○  46cc97af6802 b
    ○  2443ea76b0b1 a
    ◆  000000000000
    [EOF]
    "#);
    // View the output of each reverted commit
    let output = work_dir.run_jj(["show", "@+"]);
    insta::assert_snapshot!(output, @r#"
    Commit ID: 05f5fa79161a41b9ed3dc11e156d18de8abc7907
    Change ID: wqnwkozpkustnxypnnntnykwrqrkrpvv
    Author   : Test User <test.user@example.com> (2001-02-03 08:05:19)
    Committer: Test User <test.user@example.com> (2001-02-03 08:05:19)

        Revert "e"

        This reverts commit 208f8612074af4c219d06568a8e1f04f2e80dc25.

    Modified regular file a:
       1    1: a
       2    2: b
       3     : c
    [EOF]
    "#);
    let output = work_dir.run_jj(["show", "@++"]);
    insta::assert_snapshot!(output, @r#"
    Commit ID: f5d9e8b20bd1c5c7485e8baab4b287759c717a52
    Change ID: mouksmquosnpvwqrpsvvxtxpywpnxlss
    Author   : Test User <test.user@example.com> (2001-02-03 08:05:19)
    Committer: Test User <test.user@example.com> (2001-02-03 08:05:19)

        Revert "c"

        This reverts commit 413337bbd11f7a6636c010d9e196acf801d8df2f.

    Removed regular file b:
       1     : b
    [EOF]
    "#);
    let output = work_dir.run_jj(["show", "@+++"]);
    insta::assert_snapshot!(output, @r#"
    Commit ID: fb78f44decd2082bc2a6940624744c90b20635a8
    Change ID: tqvpomtpwrqsylrpsxknultrymmqxmxv
    Author   : Test User <test.user@example.com> (2001-02-03 08:05:19)
    Committer: Test User <test.user@example.com> (2001-02-03 08:05:19)

        Revert "b"

        This reverts commit 46cc97af6802301d8db381386e8485ff3ff24ae6.

    Modified regular file a:
       1    1: a
       2     : b
    [EOF]
    "#);
}

#[test]
fn test_revert_description_template() {
    let test_env = TestEnvironment::default();
    test_env.run_jj_in(".", ["git", "init", "repo"]).success();
    test_env.add_config(
        r#"
        [templates]
        revert_description = '''
        separate(" ",
          "Revert commit",
          commit_id.short(),
          '"' ++ description.first_line() ++ '"',
        )
        '''
        "#,
    );
    let work_dir = test_env.work_dir("repo");
    create_commit_with_files(&work_dir, "a", &[], &[("a", "a\n")]);

    // Test the setup
    insta::assert_snapshot!(get_log_output(&work_dir), @r"
    @  2443ea76b0b1 a
    ◆  000000000000
    [EOF]
    ");
    let output = work_dir.run_jj(["diff", "-s"]);
    insta::assert_snapshot!(output, @r"
    A a
    [EOF]
    ");

    // Verify that message of reverted commit follows the template
    let output = work_dir.run_jj(["revert", "-r@", "-d@"]);
    insta::assert_snapshot!(output, @r#"
    ------- stderr -------
    Reverted 1 commits as follows:
      royxmykx 1db880a5 Revert commit 2443ea76b0b1 "a"
    [EOF]
    "#);
    insta::assert_snapshot!(get_log_output(&work_dir), @r#"
    ○  1db880a5204e Revert commit 2443ea76b0b1 "a"
    @  2443ea76b0b1 a
    ◆  000000000000
    [EOF]
    "#);
}

#[must_use]
fn get_log_output(work_dir: &TestWorkDir) -> CommandOutput {
    let template = r#"commit_id.short() ++ " " ++ description"#;
    work_dir.run_jj(["log", "-T", template])
}
