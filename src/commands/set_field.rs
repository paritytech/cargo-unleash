use std::{error::Error};
use cargo::core::package::Package;
use toml_edit::{ Item, Table, Value, decorated};
use crate::util::edit_each;

/// Deactivate the Dev Dependencies Section of the given toml
pub fn set_field<'a, I>(iter: I, root_key: String, key: String, value: Value) -> Result<(), Box<dyn Error>>
where I: Iterator<Item=&'a Package>
{
    let _ = edit_each(iter, |p, doc| {
        let table = {
            let t = doc.as_table_mut().entry(&root_key);
            if t.is_none() {
                *t = Item::Table(Table::new());
            }
            if let Item::Table(inner) = t {
                inner
            } else {
                return Err(format!("Error in manifest of {:}: root key {:} is not a table.", p.name(), root_key).into())
            }
        };
        let entry = table.entry(&key);
        *entry = Item::Value(decorated(value.clone(), " ", ""));
        Ok(())
    });
    Ok(())
}