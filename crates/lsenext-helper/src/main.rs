use anyhow::{bail, Context, Result};
use lsenext_core::{create_link, load_state, LinkKind};
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_default();
    match command.as_str() {
        "drop-symlink" => drop_links(LinkKind::Symbolic, args.next())?,
        "drop-junction" => drop_links(LinkKind::Junction, args.next())?,
        "clear" => {
            lsenext_core::clear_state()?;
        }
        _ => {
            bail!(
                "usage: lsenext-helper <drop-symlink|drop-junction|clear> <target-directory>"
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
