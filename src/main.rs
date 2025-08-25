use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use chrono::DateTime;
use chrono::FixedOffset;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use figment::providers::Serialized;
use serde::Deserialize;
use serde::Serialize;

use crate::output::OutputFormat;
use crate::output::OutputSettings;
use crate::output::show_card;
use crate::storage::extract_card_by_ref;
use crate::storage::extract_card_metadatas;
use crate::storage::find_page_files;
use crate::types::CardMetadata;
use crate::types::Fingerprint;

pub mod output;
pub mod review;
pub mod storage;
pub mod terminal;
pub mod types;

/// Work with Spaced Repetition Cards (SRS) embedded in Logseq pages
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[command(flatten)]
    config: Config,
}

fn parse_hex(src: &str) -> Result<Fingerprint> {
    let s = src.trim_start_matches("0x");
    Ok(u64::from_str_radix(s, 16)?.into())
}

fn parse_datetime(src: &str) -> Result<DateTime<FixedOffset>> {
    Ok(DateTime::parse_from_rfc3339(src)?)
}

#[derive(Args)]
struct CardRefArgs {
    /// The path to the page file or graph root directory
    path: PathBuf,

    /// Fingerprint of the card's prompt.
    /// Use metadata command to find one.
    #[arg(value_parser = parse_hex)]
    prompt_fingerprint: Option<Fingerprint>,
}

#[derive(Args, Serialize, Deserialize, Debug)]
struct OutputArgs {
    #[arg(
        long,
        default_value_t = OutputFormatArg::Clean,
        value_enum
    )]
    format: OutputFormatArg,

    /// Pixel density in PPI (Pixels Per Inch).
    ///
    /// Used with image based formats.
    #[arg(long, default_value_t = 96.0)]
    ppi: f32,

    /// "base" font size in points.
    ///
    /// Everything is scaled relative to this font size.
    ///
    /// It is best to set this to the same font size as your terminal font size.
    ///
    /// Used with image based formats.
    #[arg(long, default_value_t = 12)]
    base_font_size: i32,

    /// Height of the line relative to the font size.
    ///
    /// Used for figuring out vertical size of the card.
    ///
    /// Some terminals allow scaling line height (and some scale by default),
    /// this value needs to be known to calculate the right image size.
    ///
    /// It is best to set this to the same line height scaling
    /// as your terminal line height scaling.
    ///
    /// Used with image based formats.
    #[arg(long, default_value_t = 1.2)]
    line_height_scaling: f32,
}

impl Default for OutputArgs {
    fn default() -> Self {
        Self {
            format: OutputFormatArg::Clean,
            ppi: 96.0,
            base_font_size: 12,
            line_height_scaling: 1.2,
        }
    }
}

#[derive(Args, Serialize, Deserialize, Debug, Default)]
struct Config {
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Clone, ValueEnum, Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
enum OutputFormatArg {
    Clean,
    Typst,
    Storage,
    Sixel,
    Kitty,
    ITerm,
}

impl From<&OutputFormatArg> for OutputFormat {
    fn from(value: &OutputFormatArg) -> Self {
        match value {
            OutputFormatArg::Clean => OutputFormat::Clean,
            OutputFormatArg::Typst => OutputFormat::Typst,
            OutputFormatArg::Storage => OutputFormat::Storage,
            OutputFormatArg::Sixel => OutputFormat::Sixel,
            OutputFormatArg::Kitty => OutputFormat::Kitty,
            OutputFormatArg::ITerm => OutputFormat::ITerm,
        }
    }
}

impl From<&OutputArgs> for OutputSettings {
    fn from(o: &OutputArgs) -> Self {
        OutputSettings {
            format: (&o.format).into(),
            ppi: o.ppi,
            base_font_size_pt: o.base_font_size,
            line_height_scaling: o.line_height_scaling,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Print cards
    Show {
        #[command(flatten)]
        card_ref: CardRefArgs,
    },
    /// Review cards
    Review {
        #[command(flatten)]
        card_ref: CardRefArgs,

        /// RFC3999 timestamp to use as the time of the review.
        /// Affects both selection and updating.
        #[arg(long, value_parser = parse_datetime, value_name = "TIMESTAMP")]
        at: Option<DateTime<FixedOffset>>,

        /// Seed used for shuffling cards ready to be reviewed
        #[arg(long)]
        seed: Option<u64>,
    },
    /// Print metadata for cards
    Metadata {
        #[command(flatten)]
        card_ref: CardRefArgs,
    },
    /// Fix metadata for cards
    FixMetadata {
        #[command(flatten)]
        card_ref: CardRefArgs,
    },
    /// Manage configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// show the merged configuration
    Show,
}

fn act_on_card_ref<F>(path: &Path, prompt_fingerprint: Option<Fingerprint>, f: F) -> Result<()>
where
    F: Fn(&mut Vec<CardMetadata>) -> Result<()>,
{
    // The complexity in act_on_card_ref is introduced so that PathBufs from page_files
    // outlive CardMetadatas from all_card_metadatas, which refer to these PathBufs.
    // There is probably a better way to do this.
    let page_files: Vec<PathBuf> = find_page_files(path)?;
    let mut all_card_metadatas: Vec<CardMetadata> = Vec::new();
    for page_file in page_files.iter() {
        let mut card_metadatas = extract_card_metadatas(page_file).with_context(|| {
            format!("when extracting card metadatas from {}", page_file.display())
        })?;

        if let Some(prompt_fingerprint) = prompt_fingerprint.clone() {
            card_metadatas.retain(|cm| cm.card_ref.prompt_fingerprint == prompt_fingerprint);
        }
        all_card_metadatas.extend(card_metadatas);
    }
    f(&mut all_card_metadatas)?;
    Ok(())
}

fn shuffle_slice<T>(s: &mut [T], seed: u64) {
    use rand::SeedableRng;
    use rand::rngs::SmallRng;
    use rand::seq::SliceRandom;
    let mut rng = SmallRng::seed_from_u64(seed);
    s.shuffle(&mut rng);
}

fn main() -> Result<()> {
    use figment::Figment;
    use figment::providers::Env;

    let cli = Cli::parse();
    let config: Config = Figment::new()
        .merge(Serialized::defaults(cli.config))
        .merge(Env::prefixed("LOSRS__").split("__"))
        .extract()?;

    match cli.command {
        Commands::Show { card_ref: CardRefArgs { path, prompt_fingerprint } } => {
            let output_settings = (&config.output).into();
            act_on_card_ref(&path, prompt_fingerprint, |card_metas| {
                for cm in card_metas {
                    let card = extract_card_by_ref(&cm.card_ref).with_context(|| {
                                format!(
                                    "When extracting card with fingerprint {} from {}, card with prompt prefix: {}",
                                    cm.card_ref.prompt_fingerprint,
                                    cm.card_ref.source_path.display(),
                                    cm.prompt_prefix
                                )
                            })?;
                    show_card(&card, &output_settings)?
                }
                Ok(())
            })?;
        }
        Commands::Review { card_ref: CardRefArgs { path, prompt_fingerprint }, at, seed } => {
            let output_settings = (&config.output).into();
            let at = match at {
                Some(at) => at,
                None => chrono::offset::Utc::now().fixed_offset(),
            };

            act_on_card_ref(&path, prompt_fingerprint, |card_metas| {
                card_metas.retain(|cm| cm.srs_meta.logseq_srs_meta.next_schedule <= at);
                shuffle_slice(card_metas, seed.unwrap_or_default());
                for cm in card_metas {
                    review::review_card(cm, at, &output_settings)?
                }
                Ok(())
            })?;
            println!("Reviewed all cards, huzzah!");
        }
        Commands::Metadata { card_ref: CardRefArgs { path, prompt_fingerprint } } => {
            act_on_card_ref(&path, prompt_fingerprint, |card_metas| {
                for cm in card_metas {
                    output::show_metadata(cm)?;
                }
                Ok(())
            })?;
        }
        Commands::FixMetadata { card_ref: CardRefArgs { path, prompt_fingerprint } } => {
            act_on_card_ref(&path, prompt_fingerprint, |card_metas| {
                for cm in card_metas {
                    // TODO: detect cards that are in the same file with the same fingerprint and nope out
                    storage::rewrite_card_srs_meta(&cm.card_ref, &cm.srs_meta)?;
                }
                Ok(())
            })?;
        }
        Commands::Config { command } => match command {
            ConfigCommands::Show => {
                println!("{}", serde_json::to_string(&config)?)
            }
        },
    }

    Ok(())
}
