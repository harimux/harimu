use godot::prelude::*;

use harimu::{Position, WorldSnapshot, load_world_snapshot, snapshot_from_persistent};

struct HarimuGodotViewer;

#[gdextension]
unsafe impl ExtensionLibrary for HarimuGodotViewer {}

#[derive(GodotClass)]
#[class(base=Node, init)]
struct WorldSnapshotProvider {
    #[base]
    base: Base<Node>,
}

#[godot_api]
impl INode for WorldSnapshotProvider {}

#[godot_api]
impl WorldSnapshotProvider {
    #[func]
    fn load_snapshot(&self) -> Dictionary {
        match load_world_snapshot() {
            Ok(Some(snapshot)) => snapshot_to_dict(&snapshot),
            Ok(None) => match snapshot_from_persistent() {
                Ok(snapshot) => snapshot_to_dict(&snapshot),
                Err(err) => {
                    godot_error!("No snapshot available: {}", err);
                    Dictionary::new()
                }
            },
            Err(err) => {
                godot_error!("Failed to load snapshot: {}", err);
                Dictionary::new()
            }
        }
    }
}

fn snapshot_to_dict(snapshot: &WorldSnapshot) -> Dictionary {
    let mut dict = Dictionary::new();
    let _ = dict.insert("tick", snapshot.tick as i64);

    let mut agents = Array::<Dictionary>::new();
    for agent in &snapshot.agents {
        let mut entry = Dictionary::new();
        let _ = entry.insert("id", agent.id as i64);
        let _ = entry.insert("name", agent.name.clone());
        let _ = entry.insert("qi", agent.qi as i64);
        let _ = entry.insert("transistors", agent.transistors as i64);
        let _ = entry.insert("alive", agent.alive);
        let _ = entry.insert("age", agent.age as i64);
        let _ = entry.insert("position", position_to_vec3(agent.position));
        let _ = entry.insert("max_age", agent.max_age as i64);
        agents.push(&entry);
    }
    let _ = dict.insert("agents", agents);

    let mut ore_nodes = Array::<Dictionary>::new();
    for node in &snapshot.ore_nodes {
        let mut entry = Dictionary::new();
        let _ = entry.insert("id", node.id as i64);
        let _ = entry.insert("ore", node.ore.to_string());
        let _ = entry.insert("position", position_to_vec3(node.position));
        let _ = entry.insert("available", node.available as i64);
        let _ = entry.insert("capacity", node.capacity as i64);
        let _ = entry.insert("recharge_per_tick", node.recharge_per_tick as i64);
        ore_nodes.push(&entry);
    }
    let _ = dict.insert("ore_nodes", ore_nodes);

    let mut structures = Array::<Dictionary>::new();
    for structure in &snapshot.structures {
        let mut entry = Dictionary::new();
        let _ = entry.insert("id", structure.id as i64);
        let _ = entry.insert("kind", structure.kind.to_string());
        let _ = entry.insert("owner", structure.owner as i64);
        let _ = entry.insert("position", position_to_vec3(structure.position));
        structures.push(&entry);
    }
    let _ = dict.insert("structures", structures);

    dict
}

fn position_to_vec3(pos: Position) -> Vector3 {
    Vector3::new(pos.x as f32, pos.y as f32, pos.z as f32)
}
