use cargo::{
    core::{
        package::Package,
    },
    ops::{
        OwnersOptions,
        modify_owners,
    },
    util::config::Config,
};

pub fn add_owner(c: &Config, package: &Package, new_owner: String, token: Option<String>)
    -> Result<(), String>
{
    if let Err(e) = modify_owners(c,
        &OwnersOptions {
            token,
            krate: Some(package.name().to_string()),
            to_add : Some(vec![new_owner.clone()]),
            to_remove: None,
            list: false,
            registry: None,
            index: None,
        }
    ) {
        let msg = e.to_string();
        if !msg.ends_with("is already an owner") {
            return Err(msg)
        }

        c.shell().status("Owner",
            format!("{:} is already an owner of {:}", new_owner, package.name())
        ).expect("Shell worked before. qed")
        
    }

    Ok(())
}