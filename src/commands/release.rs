use crate::commands::add_owner;
use cargo::{
    core::{package::Package, Workspace},
    ops::{publish, PublishOpts},
};
use std::error::Error;
use std::{thread, time::Duration};

pub fn release<'a>(
    packages: Vec<Package>,
    ws: Workspace<'a>,
    dry_run: bool,
    token: Option<String>,
    owner: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let c = ws.config();
    let opts = PublishOpts {
        verify: false,
        token: token.clone(),
        dry_run,
        config: c,
        allow_dirty: true,
        all_features: false,
        no_default_features: false,
        index: None,
        jobs: None,
        targets: Default::default(),
        registry: None,
        features: Default::default(),
    };

    let delay = {
        if packages.len() > 29 {
            // more than 30, delay so we do not publish more than 30 in 10min.
            21
        } else {
            // below the limit we just burst them out.
            0
        }
    };

    c.shell().status("Publishing", "Packages")?;
    for (idx, pkg) in packages.iter().enumerate() {
        if idx > 0 && delay > 0 {
            c.shell().status(
                "Waiting",
                "published 30 crates – API limites require us to wait in between.",
            )?;
            thread::sleep(Duration::from_secs(delay));
        }

        let pkg_ws = Workspace::ephemeral(pkg.clone(), c, Some(ws.target_dir()), true)?;
        c.shell().status("Publishing", &pkg)?;
        publish(&pkg_ws, &opts)?;
        if let Some(ref o) = owner {
            add_owner(c, &pkg, o.clone(), token.clone())?;
        }
    }
    Ok(())
}
