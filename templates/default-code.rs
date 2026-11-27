use forge_api::{component::Schema, gameplay::Systems};

pub fn schemas() -> Vec<Schema> {
    Vec::new()
}

pub fn register(_: &mut Systems) -> Result<(), String> {
    Ok(())
}
