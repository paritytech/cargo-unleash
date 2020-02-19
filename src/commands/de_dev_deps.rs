use std::{error::Error};
use cargo::core::package::Package;
use crate::util::edit_each;

/// Deactivate the Dev Dependencies Section of the given toml
pub fn deactivate_dev_dependencies<'a, I>(iter: I) -> Result<(), Box<dyn Error>>
where I: Iterator<Item=&'a Package>
{
    edit_each(iter,
        |_, doc| {
            let _ = doc.as_table_mut().remove("dev-dependencies");
            Ok(())
        }
    )
}