use std::error::Error;
use cargo::{
    core::{
        package::Package, Workspace
    },
    ops::{
        publish, PublishOpts,
    }
};


pub fn release<'a>(
    packages: Vec<Package>,
    ws: Workspace<'a>,
    dry_run: bool,
    token: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let c = ws.config();
    let opts = PublishOpts {
        verify: false, token, dry_run, config: c,
        allow_dirty: true, all_features: false, no_default_features: false,
        index: None, jobs: None, target: None, registry: None, features: Vec::new(),
    };

    c.shell().status("Publishing", "Packages")?;
    for pkg in packages {
        let pkg_ws = Workspace::ephemeral(pkg.clone(), c, Some(ws.target_dir()), true)?;
        c.shell().status("Publishing", &pkg)?;
        publish(&pkg_ws, &opts)?;
    }
    Ok(())
}