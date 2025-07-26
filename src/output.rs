use std::fmt::Display;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::process;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use tempfile::NamedTempFile;

use crate::types::Card;
use crate::types::CardMetadata;
use crate::types::SRSMeta;

#[derive(Clone)]
pub enum OutputFormat {
    Clean,
    Typst,
    Sixel,
    Storage,
    Kitty,
    ITerm,
}

pub enum CardBodyParts {
    Prompt,
    All,
}

fn show_card_inner(
    card: &Card,
    format: &OutputFormat,
    card_body_parts: &CardBodyParts,
) -> Result<()> {
    let mut result = Vec::new();
    match format {
        OutputFormat::Clean => format_card_clean(card, &mut result, card_body_parts)?,
        OutputFormat::Typst => format_card_typst(card, &mut result, card_body_parts)?,
        OutputFormat::Sixel => format_card_sixel(card, &mut result, card_body_parts)?,
        OutputFormat::Storage => format_card_storage(card, &mut result, card_body_parts)?,
        OutputFormat::Kitty => show_card_kitty_or_iterm(card, card_body_parts)?,
        OutputFormat::ITerm => show_card_kitty_or_iterm(card, card_body_parts)?,
    };
    std::io::stdout().write_all(&result)?;
    Ok(())
}

pub fn show_card(card: &Card, format: &OutputFormat) -> Result<()> {
    show_card_inner(card, format, &CardBodyParts::All)
}

pub fn show_card_prompt(card: &Card, format: &OutputFormat) -> Result<()> {
    show_card_inner(card, format, &CardBodyParts::Prompt)
}

pub fn show_metadata(cm: &CardMetadata) -> Result<()> {
    println!("{:?}", cm);
    Ok(())
}

pub fn format_card_clean(
    card: &Card,
    mut writer: impl std::io::Write,
    card_body_parts: &CardBodyParts,
) -> Result<()> {
    match card_body_parts {
        CardBodyParts::Prompt => writeln!(writer, "{}", card.body.prompt)?,
        CardBodyParts::All => {
            writeln!(writer, "{}", card.body.prompt)?;
            writeln!(writer, "{}", card.body.response)?;
        }
    }
    Ok(())
}

pub fn format_card_typst(
    card: &Card,
    mut writer: impl std::io::Write,
    card_body_parts: &CardBodyParts,
) -> Result<()> {
    let markdown = match card_body_parts {
        CardBodyParts::Prompt => card.body.prompt.clone(),
        CardBodyParts::All => format!("{}\n{}", card.body.prompt, card.body.response),
    };
    let typst = markdown_to_typst(markdown)
        .with_context(|| "failed to convert markdown to typst using pandoc".to_owned())?;
    write!(writer, "{}", typst)?;
    Ok(())
}

fn card_to_png(card: &Card, card_body_parts: &CardBodyParts) -> Result<Vec<u8>> {
    let markdown = match card_body_parts {
        CardBodyParts::Prompt => card.body.prompt.clone(),
        CardBodyParts::All => format!("{}\n{}", card.body.prompt, card.body.response),
    };

    let typst = markdown_to_typst(markdown)
        .with_context(|| "failed to convert markdown to typst using pandoc".to_owned())?;

    // standard logseq layout is
    //
    // graph_root
    // ├── assets
    // │   ├── image_1666695381725_0.png
    // │   ├── ...
    // ├── journals
    // │   ├── 2022_02_13.md
    // │   ├── ...
    // ├── logseq
    // │   ├── config.edn
    // │   ├── custom.css
    // │   ├── metadata.edn
    // │   └── srs-of-matrix.edn
    // └── pages
    //     ├── Sphere.md
    //     ├── ...
    //
    // So to get graph_root we need to go up twice
    let page_path = card.metadata.card_ref.source_path;
    let graph_root = page_path.parent().and_then(Path::parent).ok_or(anyhow!(
        "page file does not have a grandparent. The page is {}",
        page_path.display()
    ))?;

    let png_buf = typst_to_png(typst, graph_root)
        .with_context(|| "failed to convert typst to png via typst cli".to_owned())?;

    Ok(png_buf)
}

pub fn format_card_sixel(
    card: &Card,
    mut writer: impl std::io::Write,
    card_body_parts: &CardBodyParts,
) -> Result<()> {
    let png_buf = card_to_png(card, card_body_parts)?;

    let sixel_buf = png_to_sixel(png_buf)
        .with_context(|| "failed to convert png to sixel via img2sixel cli".to_owned())?;
    writer.write_all(&sixel_buf)?;

    Ok(())
}

pub fn show_card_kitty_or_iterm(card: &Card, card_body_parts: &CardBodyParts) -> Result<()> {
    use image::ImageReader;
    use std::io::Cursor;

    let png_buf = card_to_png(card, card_body_parts)?;

    let img = ImageReader::with_format(Cursor::new(png_buf), image::ImageFormat::Png).decode()?;

    // TODO: figure out how to prevent image over-stretching.
    // On a 1920x1080 screen a 1500x200 image gets stretched to the full width of the screen.
    // To minimize this effect, we make the image the same width...
    let conf = viuer::Config {
        absolute_offset: false,
        use_kitty: true,
        use_iterm: true,
        // TODO: figure out how to make sixel via viuer look as good as sixel via img2sixel
        use_sixel: false,
        ..Default::default()
    };

    viuer::print(&img, &conf).unwrap();

    Ok(())
}

// TODO: add https://crates.io/crates/rsille as an option

struct Typst(String);

impl Display for Typst {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn markdown_to_typst(markdown: String) -> Result<Typst> {
    // TODO: check pandoc is sufficiently advanced

    let mut child = process::Command::new("pandoc")
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .args(vec!["--from", "markdown", "--to", "typst"])
        .spawn()?;
    {
        let mut stdin =
            child.stdin.take().expect("Stdin via pipe requested but ChildStdin is not present");
        stdin.write_all(markdown.as_bytes()).with_context(|| "could not pass markdown on stdin")?;
    }

    let output = child.wait_with_output()?;

    let status = output.status;
    let stdout = String::from_utf8(output.stdout).with_context(|| "processing stdout failed")?;
    let stderr = String::from_utf8(output.stderr).with_context(|| "processing stderr failed")?;

    if status.success() && !stderr.is_empty() {
        return Err(anyhow!("the output is invalid, got warnings:\n{}", stderr));
    } else if !status.success() {
        return Err(anyhow!("{}", stderr));
    }

    Ok(Typst(stdout))
}

// At 300 pixel per inch (the only pixel density option for typst),
// 6.4in = 1920
// 3.0in = 900
// These dimensions are just about right to work in the environments that I am testing with
//   1. Microsoft terminal with sixel on a 1920x1080 screen
//   2. Wezterm with iterm on a 1920x1080 screen
//   3. TODO: ghostty on macbook
// TODO: make this configurable
const TYPST_FRONTMATTER: &str = r##"#set page(width: 6.3in, height: 2.9in, margin: 0.1in)
#show quote: it => {
  rect(
    inset: (left: 12pt, rest: 8pt),
    stroke: (left: 3pt + gray, rest: none),
    it.body
  )
}
"##;

fn typst_to_png(typst: Typst, graph_root: &Path) -> Result<Vec<u8>> {
    // typst_file needs to be in graph_root to support root relative references to assets,
    // like `![](assets/image_1666695381725_0.png)`
    //
    // typst does not support page relative references to assets,
    // like `![](../assets/image_1666695381725_0.png)`
    //
    // TODO: find or file an issue
    let mut typst_file = NamedTempFile::new_in(graph_root)?;
    typst_file.write_all(TYPST_FRONTMATTER.as_bytes())?;
    typst_file.write_all(typst.to_string().as_bytes())?;

    let mut png_file = NamedTempFile::new()?;
    let output = process::Command::new("typst")
        .arg("compile")
        .arg("--ppi=300")
        .arg("--format=png")
        .arg(typst_file.path())
        .arg(png_file.path())
        .stderr(process::Stdio::piped())
        .output()?;

    let status = output.status;
    let stderr = String::from_utf8(output.stderr).with_context(|| "processing stderr failed")?;

    if !status.success() {
        return Err(anyhow!("{}", stderr));
    }

    let mut png_buf: Vec<u8> = Vec::new();
    png_file.read_to_end(&mut png_buf)?;

    Ok(png_buf)
}

fn png_to_sixel(png_buf: Vec<u8>) -> Result<Vec<u8>> {
    // TODO: use libsixel instead of shelling out
    let mut child = process::Command::new("img2sixel")
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn()?;
    {
        let mut stdin =
            child.stdin.take().expect("Stdin via pipe requested but ChildStdin is not present");
        stdin.write_all(&png_buf).with_context(|| "could not pass png on stdin")?;
    }

    let output = child.wait_with_output()?;
    let status = output.status;
    let stderr = String::from_utf8(output.stderr).with_context(|| "processing stderr failed")?;

    if !status.success() {
        return Err(anyhow!("{}", stderr));
    }

    Ok(output.stdout)
}

fn format_card_storage_text(
    mut writer: impl std::io::Write,
    text: &str,
    indent: &str,
) -> Result<()> {
    for line in text.lines() {
        writeln!(writer, "{indent}{line}")?
    }
    Ok(())
}

fn format_card_storage_srs_meta(
    mut writer: impl std::io::Write,
    srs_meta: &SRSMeta,
    indent: &str,
) -> Result<()> {
    let logseq_srs_meta = &srs_meta.logseq_srs_meta;
    writeln!(writer, "{indent}card-last-interval:: {}", logseq_srs_meta.last_interval)?;
    writeln!(writer, "{indent}card-repeats:: {}", logseq_srs_meta.repeats)?;
    writeln!(writer, "{indent}card-ease-factor:: {}", logseq_srs_meta.ease_factor)?;
    writeln!(
        writer,
        "{indent}card-next-schedule:: {}",
        logseq_srs_meta.next_schedule.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    )?;
    writeln!(
        writer,
        "{indent}card-last-reviewed:: {}",
        logseq_srs_meta.last_reviewed.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    )?;
    writeln!(writer, "{indent}card-last-score:: {}", logseq_srs_meta.last_score)?;
    writeln!(
        writer,
        "{indent}card-fsrs-metadata:: {}",
        serde_json::to_string(&srs_meta.fsrs_meta)?
    )?;

    Ok(())
}

pub fn format_card_storage(
    card: &Card,
    mut writer: impl std::io::Write,
    card_body_parts: &CardBodyParts,
) -> Result<()> {
    // format_card_storage does not accept a CardBodyPart, as the card is always stored with all parts
    if let CardBodyParts::Prompt = card_body_parts {
        return Err(anyhow!("can not output just the prompt in storage format"));
    }
    let prompt_indent = " ".repeat(card.body.prompt_indent);
    let meta_indent = " ".repeat(card.body.prompt_indent + 2);
    format_card_storage_text(&mut writer, &card.body.prompt, &prompt_indent)?;
    format_card_storage_srs_meta(&mut writer, &card.metadata.srs_meta, &meta_indent)?;
    format_card_storage_text(&mut writer, &card.body.response, &prompt_indent)?;

    Ok(())
}
