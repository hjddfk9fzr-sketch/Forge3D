//! Workshop Run uses the public native gameplay API.
use crate::gameplay::{Command, Context, Message, Systems};

/// Register additional Rust systems here to use them in the standard player.
pub fn game_systems() -> Systems {
    let mut systems = Systems::default();
    systems.register("workshop.spawn", spawn_pickup).unwrap();
    systems.register("workshop.pickup", collect_pickup).unwrap();
    systems
        .register("workshop.inventory", receive_pickup)
        .unwrap();
    systems
}

fn spawn_pickup(ctx: Context<'_>, commands: &mut Vec<Command>) -> Result<(), String> {
    let prefab = ctx.data.as_str().ok_or("Spawner expects a prefab name.")?;
    commands.push(Command::Spawn {
        prefab: prefab.into(),
        position: ctx.object.position,
    });
    commands.push(Command::Despawn(ctx.object.id));
    Ok(())
}

fn collect_pickup(ctx: Context<'_>, commands: &mut Vec<Command>) -> Result<(), String> {
    let radius = ctx.data.as_f64().ok_or("Pickup expects a radius.")? as f32;
    let position = macroquad::prelude::Vec3::from_array(ctx.object.position);
    for object in &ctx.scene.objects {
        if object.components.contains_key("workshop.inventory")
            && position.distance(object.position.into()) <= radius
        {
            commands.push(Command::Send(Message {
                from: ctx.object.id,
                to: object.id,
                name: "picked-up".into(),
                value: 1.into(),
            }));
            commands.push(Command::Despawn(ctx.object.id));
            break;
        }
    }
    Ok(())
}

fn receive_pickup(ctx: Context<'_>, commands: &mut Vec<Command>) -> Result<(), String> {
    let count = ctx
        .messages
        .iter()
        .filter(|m| m.to == ctx.object.id && m.name == "picked-up")
        .count() as u64;
    if count > 0 {
        let stored = ctx
            .data
            .as_u64()
            .ok_or("Inventory expects an unsigned count.")?;
        commands.push(Command::Set {
            id: ctx.object.id,
            component: "workshop.inventory".into(),
            value: stored.saturating_add(count).into(),
        });
    }
    Ok(())
}
