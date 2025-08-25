use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use anyhow::anyhow;
use serde::Serialize;

use insta_cmd::get_cargo_bin;
use rexpect::process::wait::WaitStatus;
use rexpect::session::PtySession;
use rexpect::session::spawn_command;

#[derive(Serialize)]
pub struct Info {
    program: String,
    args: Vec<String>,
    envs: HashMap<String, Option<String>>,
    page_lines: Vec<Vec<String>>,
}

impl Info {
    fn new(cmd: &Command, pages: Vec<&str>) -> Self {
        Info {
            program: insta_cmd_describe_program(cmd.get_program()),
            args: cmd.get_args().map(|x| x.to_string_lossy().into_owned()).collect(),
            envs: cmd
                .get_envs()
                .map(|(k, v)| {
                    (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))
                })
                .collect(),
            page_lines: pages
                .iter()
                .map(|p| p.split("\n").map(|s| s.to_owned()).collect())
                .collect(),
        }
    }
}

// Extracted from macros
// https://github.com/mitsuhiko/insta-cmd/blob/0.6.0/src/macros.rs#L11-L17
fn insta_cmd_format_output(output: std::process::Output) -> String {
    format!(
        "success: {:?}\nexit_code: {}\n----- stdout -----\n{}\n----- stderr -----\n{}",
        output.status.success(),
        output.status.code().unwrap_or(!0),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

// Extracted from private function
// https://github.com/mitsuhiko/insta-cmd/blob/0.6.0/src/spawn.rs#L22-L30
fn insta_cmd_describe_program(cmd: &std::ffi::OsStr) -> String {
    let filename = Path::new(cmd).file_name().unwrap();
    let name = filename.to_string_lossy();
    let name = &name as &str;
    name.into()
}

fn construct_graph_root(pages: &[&str]) -> Result<tempfile::TempDir> {
    let graph_root = tempfile::TempDir::new()?;
    let pages_dir = graph_root.path().join("pages");
    std::fs::create_dir(pages_dir.as_path())?;

    pages.iter().enumerate().for_each(|(idx, page)| {
        fs::write(pages_dir.join(format!("{}.md", idx)), page)
            .expect("expect temp page writes to succeed")
    });

    Ok(graph_root)
}

fn construct_command<I, S>(args: I, envs: Vec<(&str, &str)>) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(get_cargo_bin("losrs"));
    cmd.args(args).envs(envs);
    cmd
}

fn fill_variables(args: Vec<&str>, graph_root: &Path) -> Vec<String> {
    args.iter().map(|arg| arg.replace("$GRAPH_ROOT", graph_root.to_str().unwrap())).collect()
}

macro_rules! test_card_output {
    ($name:ident, $args:expr, $envs:expr, $pages:expr ) => {
        #[test]
        fn $name() -> Result<()> {
            let args: Vec<&str> = $args;
            let envs: Vec<(&str,&str)> = $envs;
            let pages: Vec<&str> = $pages;
            let graph_root = construct_graph_root(&pages)?;
            let args: Vec<String> = fill_variables(args, graph_root.path());

            let mut cmd = construct_command(args, envs);

            let output = cmd.output().unwrap();

            insta::with_settings!({
                omit_expression => true,
                info => &Info::new(&cmd, pages),
                filters => vec![
                    (r"/tmp/.tmp\w+/", "[TMP_DIR]/"),
                ],
            },
            {
                insta::assert_snapshot!(insta_cmd_format_output(output));
            });
            Ok(())
        }
    };
}

test_card_output!(root_help, vec!["--help"], vec![], vec![""]);

test_card_output!(show_help, vec!["show", "--help"], vec![], vec![""]);

test_card_output!(
    show,
    vec!["show", "$GRAPH_ROOT",],
    vec![],
    vec![
        r#"- Not card
- What is a sphere? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-21T00:00:00.000Z
  card-last-reviewed:: 2025-03-22T09:54:57.202Z
  card-last-score:: 5
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#
    ]
);

test_card_output!(
    show_format_clean,
    vec!["show", "$GRAPH_ROOT"],
    vec![("LOSRS__OUTPUT__FORMAT", "clean")],
    vec![
        r#"- Not card
- What is a sphere? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-21T00:00:00.000Z
  card-last-reviewed:: 2025-03-22T09:54:57.202Z
  card-last-score:: 5
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#
    ]
);

test_card_output!(
    show_format_typst,
    vec!["show", "$GRAPH_ROOT"],
    vec![("LOSRS__OUTPUT__FORMAT", "typst")],
    vec![
        r#"- Not card
- What is the antiderivative of $f(x) = x^r$ (symbolic)? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-12-28T00:00:00.000Z
  card-last-reviewed:: 2025-04-28T09:12:30.985Z
  card-last-score:: 5
  - $$\int{x^r dx} = \frac{x^{(r+1)}}{r+1} + C$$
- Not card
"#
    ]
);

test_card_output!(
    show_format_storage,
    vec!["show", "$GRAPH_ROOT"],
    vec![("LOSRS__OUTPUT__FORMAT", "storage")],
    vec![
        r#"- Not card
- What is the antiderivative of $f(x) = x^r$ (symbolic)? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-12-28T00:00:00.000Z
  card-last-reviewed:: 2025-04-28T09:12:30.985Z
  card-last-score:: 5
  - $$\int{x^r dx} = \frac{x^{(r+1)}}{r+1} + C$$
- Not card
"#
    ]
);

test_card_output!(
    show_format_storage_card_without_metadata,
    vec!["show", "$GRAPH_ROOT"],
    vec![("LOSRS__OUTPUT__FORMAT", "storage")],
    vec![
        r#"- Not card
- What is the antiderivative of $f(x) = x^r$ (symbolic)? #card
  - $$\int{x^r dx} = \frac{x^{(r+1)}}{r+1} + C$$
- Not card
"#
    ]
);

test_card_output!(
    show_format_storage_card_with_fsrs_metadata,
    vec!["show", "$GRAPH_ROOT"],
    vec![("LOSRS__OUTPUT__FORMAT", "storage")],
    vec![
        r#"- Not card
- What is a sphere? #card
  card-last-interval:: 9.0
  card-repeats:: 7
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-12-01T15:04:05.123456789Z
  card-last-reviewed:: 2025-11-22T15:04:05.123456789Z
  card-last-score:: 5
  card-fsrs-metadata:: {"due":"2025-12-01T15:04:05.123456789Z","stability":8.774341658142419,"difficulty":7.040172161986166,"elapsed_days":245,"scheduled_days":9,"reps":7,"lapses":1,"state":"Review","last_review":"2025-11-22T15:04:05.123456789Z"}
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#
    ]
);

test_card_output!(
    show_format_storage_card_with_reordered_metadata,
    vec!["show", "$GRAPH_ROOT"],
    vec![("LOSRS__OUTPUT__FORMAT", "storage")],
    vec![
        r#"- Not card
- What is the antiderivative of $f(x) = x^r$ (symbolic)? #card
  card-last-reviewed:: 2025-04-28T09:12:30.985Z
  card-last-interval:: 244.14
  card-ease-factor:: 3.1
  card-last-score:: 5
  card-repeats:: 6
  card-next-schedule:: 2025-12-28T00:00:00.000Z
  - $$\int{x^r dx} = \frac{x^{(r+1)}}{r+1} + C$$
- Not card
"#
    ]
);

test_card_output!(
    show_card_with_data_after_metadata,
    vec!["show", "$GRAPH_ROOT",],
    vec![],
    vec![
        r#"- Not card
- What is the relationship between angles $\\alpha$ and $\\gamma_{1}$ in the picture relative to the transversal?
  card-last-interval:: 30.0
  card-repeats:: 6
  card-ease-factor:: 2.5
  card-next-schedule:: 2026-01-01T00:00:00.000Z
  card-last-reviewed:: 2025-12-02T00:00:00.000Z
  card-last-score:: 5
  https://upload.wikimedia.org/wikipedia/commons/thumb/3/3d/Transverzala_8.svg/262px-Transverzala_8.svg.png
  #card
  - They are alternate angles.
- Not card
"#
    ]
);

test_card_output!(
    show_card_with_unicode_prompt,
    vec!["show", "$GRAPH_ROOT",],
    vec![],
    vec![
        r#"- Not card
- Какова связь между углами $\\alpha$ и $\\gamma_{1}$ на изображении относительно секущей?
  card-last-interval:: 30.0
  card-repeats:: 6
  card-ease-factor:: 2.5
  card-next-schedule:: 2026-01-01T00:00:00.000Z
  card-last-reviewed:: 2025-12-02T00:00:00.000Z
  card-last-score:: 5
  https://upload.wikimedia.org/wikipedia/commons/thumb/3/3d/Transverzala_8.svg/262px-Transverzala_8.svg.png
  #card
  - Они накрест лежащие.
- Not card
"#
    ]
);

test_card_output!(metadata_help, vec!["metadata", "--help"], vec![], vec![""]);

test_card_output!(
    metadata,
    vec!["metadata", "$GRAPH_ROOT"],
    vec![],
    vec![
        r#"- Not card
- What is a sphere? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-21T00:00:00.000Z
  card-last-reviewed:: 2025-03-22T09:54:57.202Z
  card-last-score:: 5
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#
    ]
);

test_card_output!(
    show_with_fingerprint,
    vec!["show", "$GRAPH_ROOT", "0xb9de554a02212aca"],
    vec![],
    vec![
        r#"- Not card
- What is a sphere? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-21T00:00:00.000Z
  card-last-reviewed:: 2025-03-22T09:54:57.202Z
  card-last-score:: 5
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- What is the volume of a sphere (symbolic)? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-27T00:00:00.000Z
  card-last-reviewed:: 2025-03-28T07:46:41.223Z
  card-last-score:: 5
  - $$V = \frac{4}{3} \pi r^3$$
- Not card
"#
    ]
);

test_card_output!(
    show_multiple_page_files,
    vec!["show", "$GRAPH_ROOT",],
    vec![],
    vec![
        r#"- Not card
- What is the volume of a sphere (symbolic)? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-27T00:00:00.000Z
  card-last-reviewed:: 2025-03-28T07:46:41.223Z
  card-last-score:: 5
  - $$V = \frac{4}{3} \pi r^3$$
- Not card
"#,
        r#"- Not card
- What is a sphere? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-21T00:00:00.000Z
  card-last-reviewed:: 2025-03-22T09:54:57.202Z
  card-last-score:: 5
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#
    ]
);

test_card_output!(
    show_card_is_deeply_nested,
    vec!["show", "$GRAPH_ROOT",],
    vec![],
    vec![
        r#"- Not card
- induction
  - Automatically generated induction principles for Inductive Types (in general)
    - What kind of function is a generated induction principle function (similarity, not implimentation)? #card
      card-last-score:: 5
      card-repeats:: 6
      card-next-schedule:: 2026-01-25T00:00:00.000Z
      card-last-interval:: 244.14
      card-ease-factor:: 3.5
      card-last-reviewed:: 2025-05-26T09:11:31.735Z
      - Fixpoint
- Not card
"#,
    ]
);

test_card_output!(
    show_format_storage_card_is_deeply_nested,
    vec!["show", "$GRAPH_ROOT"],
    vec![("LOSRS__OUTPUT__FORMAT", "storage")],
    vec![
        r#"- Not card
- induction
  - Automatically generated induction principles for Inductive Types (in general)
    - What kind of function is a generated induction principle function (similarity, not implimentation)? #card
      card-last-score:: 5
      card-repeats:: 6
      card-next-schedule:: 2026-01-25T00:00:00.000Z
      card-last-interval:: 244.14
      card-ease-factor:: 3.5
      card-last-reviewed:: 2025-05-26T09:11:31.735Z
      - Fixpoint
- Not card
"#,
    ]
);

#[derive(Serialize)]
pub struct ReviewInfo {
    program: String,
    args: Vec<String>,
    page_lines: Vec<String>,
    interaction_meta: HashMap<String, String>,
}

impl ReviewInfo {
    fn new(cmd: &Command, page: &str, interaction_meta: HashMap<String, String>) -> Self {
        ReviewInfo {
            program: insta_cmd_describe_program(cmd.get_program()),
            args: cmd.get_args().map(|x| x.to_string_lossy().into_owned()).collect(),
            page_lines: page.split("\n").map(|s| s.to_owned()).collect(),
            interaction_meta,
        }
    }
}

fn expect_review_interaction(p: &mut PtySession, remembered: bool) -> Result<()> {
    p.exp_string("Press any key to show the answer")?;
    p.send(" ")?;
    p.flush()?;

    p.exp_string("How much effort did recall require?")?;
    p.exp_string("1 - Little Effort; 2 - Some effort; 3 - Much Effort; 4 - Did not recall")?;
    p.send(if remembered { "2" } else { "4" })?;
    p.flush()?;

    p.read_line()?; // for the process to exit

    Ok(())
}

fn expect_no_review_interaction(p: &mut PtySession) -> Result<()> {
    p.exp_string("Reviewed all cards, huzzah!")?;

    Ok(())
}

fn expect_nope_out_interaction(p: &mut PtySession) -> Result<()> {
    p.exp_string("Ctrl+C or Esc to nope out")?;
    p.send_control('c')?;
    p.read_line()?; // for the process to exit

    Ok(())
}

fn read_solitary_page(graph_root: &Path) -> Result<String> {
    let page_paths = fs::read_dir(graph_root.join("pages"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect::<Vec<_>>();
    if page_paths.len() != 1 {
        return Err(anyhow!(
            "expected {graph_root:?} to have exactly 1 page, got: {}",
            page_paths.len()
        ));
    }
    let page_path = &page_paths[0];
    Ok(fs::read_to_string(page_path)?)
}

macro_rules! test_card_review {
    ($name:ident, $args:expr, $page:expr, $f:expr, $interaction_meta:expr ) => {
        #[test]
        fn $name() -> Result<()> {
            let mut args: Vec<&str> = vec!["review", "$GRAPH_ROOT"];
            args.extend_from_slice(&($args));
            let page: &str = $page;
            let f: fn(&mut PtySession) -> Result<()> = $f;
            let interaction_meta: HashMap<String, String> = $interaction_meta;

            let graph_root = construct_graph_root(&[page])?;
            let args = fill_variables(args, graph_root.path());
            let cmd = construct_command(&args, vec![]);

            let cmd_info = &ReviewInfo::new(&cmd, page, interaction_meta);
            let mut p = spawn_command(cmd, Some(1000))?;

            f(&mut p)?;


            let status = p.process.status().ok_or(anyhow!("could not get process status"))?;
            match status {
                WaitStatus::Exited(_, _) => {}
                _ => return Err(anyhow!("expected process to exit, got {:?}", status)),
            }

            let file_raw = read_solitary_page(graph_root.path())?;
            insta::with_settings!({
                omit_expression => true,
                info => cmd_info,
                filters => vec![
                    (r"/tmp/.tmp\w+/", "[TMP_DIR]/"),
                ],
            },
            {
                insta::assert_snapshot!(file_raw);
            });

            Ok(())
        }
    };
}

test_card_review!(
    review_remembered_yes,
    vec!["--at=2025-11-22T15:04:05.123456789Z"],
    r#"- Not card
- What is a sphere? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-21T00:00:00.000Z
  card-last-reviewed:: 2025-03-22T09:54:57.202Z
  card-last-score:: 5
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#,
    |p: &mut PtySession| -> Result<()> { expect_review_interaction(p, true) },
    HashMap::from([
        ("expected type of interaction".to_string(), "review".to_string()),
        ("remembered".to_string(), "true".to_string())
    ])
);

test_card_review!(
    review_remembered_no,
    vec!["--at=2025-11-22T15:04:05.123456789Z"],
    r#"- Not card
- What is a sphere? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-21T00:00:00.000Z
  card-last-reviewed:: 2025-03-22T09:54:57.202Z
  card-last-score:: 5
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#,
    |p: &mut PtySession| -> Result<()> { expect_review_interaction(p, false) },
    HashMap::from([
        ("expected type of interaction".to_string(), "review".to_string()),
        ("remembered".to_string(), "false".to_string())
    ])
);

test_card_review!(
    review_card_without_meta_remembered_yes,
    vec!["--at=2025-11-22T15:04:05.123456789Z"],
    r#"- Not card
- What is a sphere? #card
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#,
    |p: &mut PtySession| -> Result<()> { expect_review_interaction(p, true) },
    HashMap::from([
        ("expected type of interaction".to_string(), "review".to_string()),
        ("remembered".to_string(), "true".to_string())
    ])
);

test_card_review!(
    review_card_without_meta_remembered_no,
    vec!["--at=2025-11-22T15:04:05.123456789Z"],
    r#"- Not card
- What is a sphere? #card
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#,
    |p: &mut PtySession| -> Result<()> { expect_review_interaction(p, false) },
    HashMap::from([
        ("expected type of interaction".to_string(), "review".to_string()),
        ("remembered".to_string(), "false".to_string())
    ])
);

test_card_review!(
    review_card_second_remembered_no,
    vec!["--at=2025-11-23T15:04:05.123456789Z"],
    r#"- Not card
- What is a sphere? #card
  card-fsrs-metadata:: {"due":"2025-11-23T15:04:05.123456789Z","stability":0.4072,"difficulty":7.2102,"elapsed_days":0,"scheduled_days":1,"reps":1,"lapses":0,"state":"Review","last_review":"2025-11-22T15:04:05.123456789Z"}
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#,
    |p: &mut PtySession| -> Result<()> { expect_review_interaction(p, false) },
    HashMap::from([
        ("expected type of interaction".to_string(), "review".to_string()),
        ("remembered".to_string(), "false".to_string())
    ])
);

test_card_output!(review_help, vec!["review", "--help"], vec![], vec![""]);

test_card_review!(
    review_card_not_ready,
    vec!["--at=2024-01-01T15:04:05.123456789Z"],
    r#"- Not card
- What is a sphere? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-21T00:00:00.000Z
  card-last-reviewed:: 2025-03-22T09:54:57.202Z
  card-last-score:: 5
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card
"#,
    |p: &mut PtySession| -> Result<()> { expect_no_review_interaction(p) },
    HashMap::from([("expected type of interaction".to_string(), "no review".to_string()),])
);

test_card_review!(
    review_card_seed_0,
    vec!["--at=2025-09-01T15:04:05.123456789Z", "--seed=0"],
    r#"- Not card
- Alphabet forward cards
  - What is Gregg Simplified for "N" (description)? #card
    card-last-interval:: 15.0
    card-repeats:: 4
    card-ease-factor:: 1.0
    card-next-schedule:: 2025-08-12T09:03:05.489Z
    card-last-reviewed:: 2025-07-04T09:03:05.489Z
    card-last-score:: 1
    - forward short stroke
  - What is Gregg Simplified for "M" (description)? #card
    card-last-interval:: 15.0
    card-repeats:: 4
    card-ease-factor:: 1.0
    card-next-schedule:: 2025-08-12T09:03:05.489Z
    card-last-reviewed:: 2025-07-04T09:03:05.489Z
    card-last-score:: 1
    - forward long stroke
- Not card
"#,
    |p: &mut PtySession| -> Result<()> {
        expect_review_interaction(p, true)?;
        expect_nope_out_interaction(p)?;
        Ok(())
    },
    HashMap::from([
        (
            "expected type of interaction".to_string(),
            "review first card, then nope out".to_string()
        ),
        (
            "first card with given seed".to_string(),
            r#"What is Gregg Simplified for "M" (description)?"#.to_string()
        ),
    ])
);

test_card_review!(
    review_card_seed_100,
    vec!["--at=2025-09-01T15:04:05.123456789Z", "--seed=100"],
    r#"- Not card
- Alphabet forward cards
  - What is Gregg Simplified for "N" (description)? #card
    card-last-interval:: 15.0
    card-repeats:: 4
    card-ease-factor:: 1.0
    card-next-schedule:: 2025-08-12T09:03:05.489Z
    card-last-reviewed:: 2025-07-04T09:03:05.489Z
    card-last-score:: 1
    - forward short stroke
  - What is Gregg Simplified for "M" (description)? #card
    card-last-interval:: 15.0
    card-repeats:: 4
    card-ease-factor:: 1.0
    card-next-schedule:: 2025-08-12T09:03:05.489Z
    card-last-reviewed:: 2025-07-04T09:03:05.489Z
    card-last-score:: 1
    - forward long stroke
- Not card
"#,
    |p: &mut PtySession| -> Result<()> {
        expect_review_interaction(p, true)?;
        expect_nope_out_interaction(p)?;
        Ok(())
    },
    HashMap::from([
        (
            "expected type of interaction".to_string(),
            "review first card, then nope out".to_string()
        ),
        (
            "first card with given seed".to_string(),
            r#"What is Gregg Simplified for "N" (description)?"#.to_string()
        ),
    ])
);

#[test]
fn newline_writeback_on_review() -> Result<()> {
    let args = vec!["review", "$GRAPH_ROOT", "--at=2025-11-22T15:04:05.123456789Z"];
    let page = r#"- What is a sphere? #card
  card-last-interval:: 244.14
  card-repeats:: 6
  card-ease-factor:: 3.1
  card-next-schedule:: 2025-11-21T00:00:00.000Z
  card-last-reviewed:: 2025-03-22T09:54:57.202Z
  card-last-score:: 5
  - Set of points in a 3 dimensional space that are equidistant from a center point.
- Not card"#;
    let graph_root = construct_graph_root(&[page])?;
    let args = fill_variables(args, graph_root.path());
    let cmd = construct_command(&args, vec![]);

    let mut p = spawn_command(cmd, Some(1000))?;

    expect_review_interaction(&mut p, true)?;

    let status = p.process.status().ok_or(anyhow!("could not get process status"))?;
    match status {
        WaitStatus::Exited(_, _) => {}
        _ => return Err(anyhow!("expected process to exit, got {:?}", status)),
    }

    let page_raw = read_solitary_page(graph_root.path())?;
    let leading_newline_count = page_raw.chars().take_while(|&c| c == '\n').count();
    let trailing_newline_count = page_raw.chars().rev().take_while(|&c| c == '\n').count();

    assert_eq!(leading_newline_count, 0, "expect 0 leading newlines when file is just one card");
    assert_eq!(trailing_newline_count, 1, "expect 1 trailing newline when file is just one card");

    Ok(())
}
