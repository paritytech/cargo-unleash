use crate::util::edit_each;
use cargo::core::package::Package;

/// Deactivate the Dev Dependencies Section of the given toml
pub fn deactivate_dev_dependencies<'a, I>(iter: I) -> Result<(), anyhow::Error>
where
	I: Iterator<Item = &'a Package>,
{
	edit_each(iter, |_, doc| Ok(doc.as_table_mut().remove("dev-dependencies")))?;
	Ok(())
}
