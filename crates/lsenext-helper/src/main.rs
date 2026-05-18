use anyhow::{bail, Context, Result};
use lsenext_core::{create_link, load_state, save_sources, LinkKind};
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_default();
    match command.as_str() {
        "pick-source" => {
            let paths = args.map(PathBuf::from).collect::<Vec<_>>();
            if paths.is_empty() {
                bail!("at least one source path is required");
            }
            save_sources(&paths)?;
        }
        "drop-symlink" => drop_links(LinkKind::Symbolic, args.next())?,
        "drop-junction" => drop_links(LinkKind::Junction, args.next())?,
        "clear" => {
            lsenext_core::clear_state()?;
        }
        _ => {
            bail!(
                "usage: lsenext-helper <pick-source|drop-symlink|drop-junction|clear> [paths]"
            );
        }
    }
    Ok(())
}

fn drop_links(kind: LinkKind, target: Option<String>) -> Result<()> {
    let target = target
        .map(PathBuf::from)
        .context("target directory argument is required")?;
    let state = load_state()?.context("no picked LSENext source is stored")?;
    for source in &state.sources {
        create_link(kind, source, &target)?;
    }
    Ok(())
}
