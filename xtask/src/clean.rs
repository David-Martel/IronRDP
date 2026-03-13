use crate::prelude::*;

pub fn workspace(sh: &Shell) -> anyhow::Result<()> {
    let _s = Section::new("CLEAN");

    println!("Remove local cargo root folder...");
    sh.remove_path("./.cargo/local_root")?;
    println!("Done.");

    cmd!(sh, "{CARGO} clean").run()?;

    Ok(())
}
