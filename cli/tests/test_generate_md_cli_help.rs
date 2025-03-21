// Copyright 2024 The Jujutsu Authors
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

use insta::assert_snapshot;

use crate::common::TestEnvironment;

const PREAMBLE: &str = r#"
<!-- BEGIN MARKDOWN-->

"#;

#[test]
fn test_generate_markdown_docs_in_docs_dir() {
    let test_env = TestEnvironment::default();
    let mut markdown_help = PREAMBLE.to_string();
    markdown_help.push_str(
        test_env
            .run_jj_in(".", ["util", "markdown-help"])
            .success()
            .stdout
            .raw(),
    );

    insta::with_settings!({
        snapshot_path => ".",
        snapshot_suffix => ".md",
        prepend_module_to_snapshot => false,
        omit_expression => true,
        description => "AUTO-GENERATED FILE, DO NOT EDIT. This cli reference is generated \
                        by a test as an `insta` snapshot. MkDocs includes this snapshot \
                        from docs/cli-reference.md.",
    },
    { assert_snapshot!("cli-reference", markdown_help) });
}
