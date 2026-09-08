//! Dry run by default. Apply only to a copied WAL and a new output directory.
use clap::Parser;
#[derive(Parser)]
struct Args {
    #[arg(long)]
    wal_copy: std::path::PathBuf,
    #[arg(long)]
    output: std::path::PathBuf,
    #[arg(long, default_value = "")]
    embedding_space: String,
    #[arg(long)]
    apply: bool,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let report = contextnest::services::tenants::import::migrate_copy(
        &args.wal_copy,
        &args.output,
        &args.embedding_space,
        args.apply,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
