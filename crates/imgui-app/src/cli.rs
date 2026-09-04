use clap::Parser;
use std::path::PathBuf;

/// Label a dataset with a controller.
#[derive(Parser, Debug)]
#[command(name = "tagpad", version, about)]
pub struct Cli {
    /// Task file describing the items and the available verdicts.
    pub task: PathBuf,

    /// Where to write results. Defaults to `<task>.results.json`.
    #[arg(short, long)]
    pub out: Option<PathBuf>,

    /// Start over, ignoring any results already in the output file.
    #[arg(long)]
    pub fresh: bool,

    /// Name recorded alongside the judgments.
    #[arg(long, default_value = "human")]
    pub reviewer: String,
}

impl Cli {
    /// Results sit next to the task by default, so a labeller who passes one
    /// path still gets their work saved somewhere findable.
    pub fn out_path(&self) -> PathBuf {
        self.out.clone().unwrap_or_else(|| {
            let mut p = self.task.clone();
            let stem = p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            p.set_file_name(format!("{stem}.results.json"));
            p
        })
    }
}
