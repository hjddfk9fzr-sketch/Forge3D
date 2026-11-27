use forge_api::{component::{Schema, Field, Kind}, gameplay::{Command, Context, Systems}};

pub fn schemas() -> Vec<Schema> {
    vec![Schema {
        name: "project.spin".into(),
        fields: vec![Field::new("degrees", "Degrees per second", Kind::Number {
            default: 60., min: -360., max: 360.,
        })],
        trigger: false,
    }]
}

pub fn register(systems: &mut Systems) -> Result<(), String> {
    systems.register("project.spin", spin)
}

fn spin(context: Context<'_>, commands: &mut Vec<Command>) -> Result<(), String> {
    let speed = context.data["degrees"].as_f64().ok_or("Expected a spin speed.")? as f32;
    if context.object.spin != speed {
        commands.push(Command::SetSpin { id: context.object.id, degrees_per_second: speed });
    }
    Ok(())
}
