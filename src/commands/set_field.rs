use crate::util::edit_each;
use cargo::core::package::Package;

use toml_edit::{Item, Table, Value};

/// Deactivate the Dev Dependencies Section of the given toml
pub fn set_field<'a, I>(
	iter: I,
	root_key: String,
	key: String,
	value: Value,
) -> Result<(), anyhow::Error>
where
	I: Iterator<Item = &'a Package>,
{
	let _ = edit_each(iter, |p, doc| {
		let table = {
			let t =
				doc.as_table_mut().entry(&root_key).or_insert_with(|| Item::Table(Table::new()));
			if let Item::Table(inner) = t {
				inner
			} else {
				anyhow::bail!(
					"Error in manifest of {:}: root key {:} is not a table.",
					p.name(),
					root_key
				);
			}
		};
		let _ = table.insert(&key, Item::Value(value.clone().decorated(" ", "")));
		Ok(())
	});
	Ok(())
}
