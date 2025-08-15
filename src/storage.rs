use std::ffi::OsStr;
use std::fs::File;
use std::fs::{self};
use std::io::Write;
use std::ops::RangeInclusive;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use chrono::DateTime;

use markdown::ParseOptions;
use markdown::mdast::Node;
use markdown::mdast::{self};
use markdown::to_mdast;

use crate::output::CardBodyParts;
use crate::output::format_card_storage;
use crate::types::Card;
use crate::types::CardBody;
use crate::types::CardMetadata;
use crate::types::CardRef;
use crate::types::FSRSMeta;
use crate::types::LogseqSRSMeta;
use crate::types::SRSMeta;

fn list_item_is_card(li: &mdast::ListItem) -> bool {
    // A ListItem "is a card" if its first child is a Paragraph whos child is a Text with
    // value that has substring "#card"
    // Example card:
    // ListItem {
    //   children: [
    //       Paragraph {
    //           children: [
    //               Text {
    //                   value: "What is the taxon common name for Angiosperm? #card\ncard-last-interval:: 97.66\ncard-repeats:: 5\ncard-ease-factor:: 3\ncard-next-schedule:: 2025-07-14T00:00:00.000Z\ncard-last-reviewed:: 2025-04-07T09:12:54.010Z\ncard-last-score:: 5",
    //                   position: Some(
    //                       4:3-10:22 (53-293),
    //                   ),
    //               },
    //           ],
    //           position: Some(
    //               4:3-10:22 (53-293),
    //           ),
    //       },
    //       ...
    //   ],
    //   position: Some(
    //       4:1-11:21 (51-314),
    //   ),
    //   spread: false,
    //   checked: None,
    // }

    if let Some(Node::Paragraph(p)) = li.children.first() {
        return p.children.iter().any(|child| {
            if let Node::Text(text) = child { text.value.contains("#card") } else { false }
        });
    }

    false
}

fn find_card_list_items(file_raw: &str) -> Result<Vec<mdast::ListItem>> {
    let tree = to_mdast(file_raw, &ParseOptions::default())
        .map_err(|x| anyhow!("could not parse markdown: {:?}", x))?;
    let Node::Root(r) = tree else {
        return Err(anyhow!("expected Root node, got: {:?}", tree));
    };
    let top_list = match r.children.as_slice() {
        [Node::Paragraph(_)] | [] => {
            // If it's just a paragraph, there are no cards.
            // If it's empty, there are no cards.
            return Ok(vec![]);
        }
        [Node::Paragraph(_), Node::List(l)] => l,
        [Node::List(l)] => l,
        top_nodes => {
            return Err(anyhow!("expected (Paragraph,)? List, got: {:?}", top_nodes));
        }
    };
    Ok(find_card_list_items_inner(top_list))
}

fn find_card_list_items_inner(list: &mdast::List) -> Vec<mdast::ListItem> {
    let mut cards = Vec::new();
    for node in &list.children {
        if let Node::ListItem(li) = node {
            if list_item_is_card(li) {
                cards.push(li.clone());
                // We don't want cards within cards, perhaps it is worth warning about this
                continue;
            }
            for child in &li.children {
                if let Node::List(l) = child {
                    let mut nested = find_card_list_items_inner(l);
                    cards.append(&mut nested);
                }
            }
        }
    }
    cards
}

fn range_from_position(position: &markdown::unist::Position) -> RangeInclusive<usize> {
    // start.line and end.line are 1-indexed
    // start "Represents the place of the first character of the parsed source region."
    // end "Represents the place of the first character after the parsed source region, whether it exists or not."
    RangeInclusive::new(position.start.line - 1, position.end.line - 1)
}

fn find_card_ranges(
    card: &mdast::ListItem,
) -> Result<(RangeInclusive<usize>, RangeInclusive<usize>)> {
    // TODO: allow multiple paragraphs followed by a list
    // take until list?
    let (prompt_paragraph, response_list) = match card.children.as_slice() {
        [Node::Paragraph(p), Node::List(l)] => (p, l),
        _ => {
            return Err(anyhow!(
                "Expected card children to be [Paragraph, List], got {:?}",
                card.children
            ));
        }
    };

    let p_position = prompt_paragraph
        .position
        .as_ref()
        .ok_or_else(|| anyhow!("The p somehow didn't have a position"))?;
    let p_range = range_from_position(p_position);

    let l_position = response_list
        .position
        .as_ref()
        .ok_or_else(|| anyhow!("The p somehow didn't have a position"))?;
    let l_range = range_from_position(l_position);

    Ok((p_range, l_range))
}

fn destructure_card<'a>(
    card: &mdast::ListItem,
    file_raw_lines: &'a [&'a str],
) -> Result<(&'a [&'a str], &'a [&'a str])> {
    let (p_range, l_range) = find_card_ranges(card)?;
    let Some(p_lines) = file_raw_lines.get(p_range) else {
        return Err(anyhow!("Failed to get prompt lines"));
    };

    let Some(l_lines) = file_raw_lines.get(l_range) else {
        return Err(anyhow!("Failed to get response list lines"));
    };

    Ok((p_lines, l_lines))
}

fn is_metadata_line(l: &str) -> bool {
    l.trim_start().starts_with("card-")
}

impl SRSMeta {
    fn from_prompt_lines(prompt_lines: &[&str]) -> Result<Self> {
        let mut logseq_srs_meta = LogseqSRSMeta::default();
        let mut fsrs_meta: Option<FSRSMeta> = None;

        for line in prompt_lines {
            let Some((k, v)) = line.trim().split_once(":: ") else {
                continue;
            };
            (|| -> Result<()> {
                match k {
                    "card-last-interval" => {
                        logseq_srs_meta.last_interval = v.parse()?;
                    }
                    "card-repeats" => {
                        logseq_srs_meta.repeats = v.parse()?;
                    }
                    "card-ease-factor" => {
                        logseq_srs_meta.ease_factor = v.parse()?;
                    }
                    "card-next-schedule" => {
                        logseq_srs_meta.next_schedule = DateTime::parse_from_rfc3339(v)?;
                    }
                    "card-last-reviewed" => {
                        logseq_srs_meta.last_reviewed = DateTime::parse_from_rfc3339(v)?;
                    }
                    "card-last-score" => {
                        logseq_srs_meta.last_score = v.parse()?;
                    }
                    "card-fsrs-metadata" => {
                        fsrs_meta = Some(serde_json::from_str(v)?);
                    }
                    _ => {}
                };
                Ok(())
            })()
            .with_context(|| anyhow!("when processing key '{}'", k))?;
        }
        match fsrs_meta {
            Some(fsrs_meta) => {
                let logseq_srs_meta: LogseqSRSMeta = (&fsrs_meta).into();
                Ok(SRSMeta { logseq_srs_meta, fsrs_meta })
            }
            None => {
                // This case includes "neither metadata is present",
                let fsrs_meta: FSRSMeta = (&logseq_srs_meta).into();
                Ok(SRSMeta { logseq_srs_meta, fsrs_meta })
            }
        }
    }
}

fn strip_prompt_metadata<'a>(
    prompt_lines: impl Iterator<Item = &'a str>,
) -> impl Iterator<Item = &'a str> {
    prompt_lines.filter(|l| !is_metadata_line(l))
}

fn strip_indent<'a>(
    lines: impl Iterator<Item = &'a str>,
    indent: &str,
) -> impl Iterator<Item = &'a str> {
    lines.map(move |line| line.strip_prefix(indent).unwrap_or(line))
}

fn extract_card<'a>(
    card_list_item: &mdast::ListItem,
    path: &'a Path,
    file_raw_lines: &[&str],
) -> Result<Card<'a>> {
    let (prompt_lines, response_lines) = destructure_card(card_list_item, file_raw_lines)?;

    let prompt_line_first = prompt_lines.first().unwrap_or(&"").to_owned().trim_end();
    let prompt_indent_size = prompt_line_first.chars().take_while(|c| *c == ' ').count();
    let prompt_indent = " ".repeat(prompt_indent_size);
    // prompt_indent+2 to strip `- `
    let prompt_prefix = prompt_line_first.chars().skip(prompt_indent_size + 2).take(64).collect();

    let prompt = strip_indent(strip_prompt_metadata(prompt_lines.iter().copied()), &prompt_indent)
        .collect::<Vec<_>>()
        .join("\n");

    let response =
        strip_indent(response_lines.iter().copied(), &prompt_indent).collect::<Vec<_>>().join("\n");

    Ok(Card {
        metadata: CardMetadata {
            card_ref: CardRef { source_path: path, prompt_fingerprint: prompt.as_str().into() },
            prompt_prefix,
            srs_meta: SRSMeta::from_prompt_lines(prompt_lines)
                .with_context(|| "when extracting SRS meta")?,
        },
        body: CardBody { prompt, prompt_indent: prompt_indent_size, response },
    })
}

pub fn extract_card_metadatas<'a>(path: &'a Path) -> Result<Vec<CardMetadata<'a>>> {
    let file_raw = fs::read_to_string(path)?;
    let file_raw_lines: Vec<&str> = file_raw.lines().collect();

    let card_list_items = find_card_list_items(&file_raw)
        .with_context(|| anyhow!("when searching for card list items"))?;

    let cards = card_list_items
        .iter()
        .map(|li| {
            extract_card(li, path, &file_raw_lines).with_context(|| {
                anyhow!(
                    "when extracting a card from list item on line {}",
                    li.position
                        .as_ref()
                        .map(|pos| pos.start.line)
                        .expect("so far list items always have a start...")
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let card_metadatas = cards.into_iter().map(|c| c.metadata).collect();

    Ok(card_metadatas)
}

pub fn extract_card_by_ref<'a>(card_ref: &CardRef<'a>) -> Result<Card<'a>> {
    let path = card_ref.source_path;
    let file_raw = fs::read_to_string(path)?;
    let file_raw_lines: Vec<&str> = file_raw.lines().collect();

    let card_list_items = find_card_list_items(&file_raw)?;

    for li in card_list_items.as_slice() {
        let c = extract_card(li, path, &file_raw_lines)?;
        if c.metadata.card_ref.prompt_fingerprint == card_ref.prompt_fingerprint {
            return Ok(c);
        }
    }
    Err(anyhow!(
        "Card with fingerprint {} was not found in {}.",
        card_ref.prompt_fingerprint,
        card_ref.source_path.display(),
    ))
}

pub fn rewrite_card_srs_meta(card_ref: &CardRef, srs_meta: &SRSMeta) -> Result<()> {
    let path = card_ref.source_path;
    let file_raw = fs::read_to_string(path)?;
    let file_raw_lines: Vec<&str> = file_raw.lines().collect();

    let card_list_items = find_card_list_items(&file_raw)?;

    for li in card_list_items.as_slice() {
        let mut card = extract_card(li, path, &file_raw_lines)?;
        card.metadata.srs_meta = srs_meta.clone();
        if card.metadata.card_ref.prompt_fingerprint == card_ref.prompt_fingerprint {
            let (p_lines, l_lines) = find_card_ranges(li)?;
            let mut f = File::create(path)?;

            let pre_lines = &file_raw_lines[..p_lines.into_inner().0];
            if !pre_lines.is_empty() {
                f.write_all(pre_lines.join("\n").as_bytes())?;
                f.write_all("\n".as_bytes())?;
            }

            format_card_storage(&card, &mut f, &CardBodyParts::All)?;

            let post_lines = &file_raw_lines[l_lines.into_inner().1 + 1..];
            if !post_lines.is_empty() {
                f.write_all(post_lines.join("\n").as_bytes())?;
                f.write_all("\n".as_bytes())?;
            }

            return Ok(());
        }
    }

    Err(anyhow!(
        "Card with fingerprint {} was not found in {}.",
        card_ref.prompt_fingerprint,
        card_ref.source_path.display(),
    ))
}

pub fn find_page_files(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        return Err(anyhow!("{} does not exist", path.display()));
    }
    if !path.is_dir() {
        return Ok(vec![path.to_owned()]);
    };
    let pages_dir = path.join("pages");
    if !pages_dir.exists() {
        return Err(anyhow!(
            "{} is a directory without a pages subdirectory, expected logseq graph root",
            path.display()
        ));
    }
    let page_files = std::fs::read_dir(pages_dir)?
        .filter_map(Result::ok)
        .map(|d| d.path())
        .filter(|p| p.is_file() && p.extension() == Some(OsStr::new("md")))
        .collect();
    Ok(page_files)
}
