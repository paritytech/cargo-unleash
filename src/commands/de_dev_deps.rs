use crate::util::with_deactivated_dev_dependencies;
use cargo::core::package::Package;
use std::error::Error;

/// Deactivate the Dev Dependencies Section of the given toml
pub fn deactivate_dev_dependencies<'a, I>(iter: I) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = &'a Package>,
{
    with_deactivated_dev_dependencies(iter, || Ok(()))
}
