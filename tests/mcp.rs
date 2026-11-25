use forge3d::runtime::Scene;
use macroquad::prelude::Vec3;
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

struct Client {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
}

impl Client {
    fn send(&mut self, request: Value) {
        writeln!(self.input, "{request}").unwrap();
        self.input.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}));
        let mut line = String::new();
        assert!(self.output.read_line(&mut line).unwrap() > 0);
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], self.id);
        response["result"].clone()
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":arguments}))
    }

    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.tool(name, arguments);
        assert_ne!(result["isError"], true, "{result}");
        result["structuredContent"].clone()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_edits_simulates_saves_and_exports_a_scene() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    fs::create_dir(&root).unwrap();
    fs::write(temp.path().join("outside.mtl"), "newmtl red\nKd 1 0 0\n").unwrap();
    fs::write(
        root.join("restricted.obj"),
        "mtllib ../outside.mtl\nusemtl red\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
    )
    .unwrap();
    fs::write(
        root.join("triangle.obj"),
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n",
    )
    .unwrap();
    fs::write(
        root.join("move.rhai"),
        "fn update(object, dt, input) { print(\"trace\"); object.y += 1.0; object }",
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_forge3d-mcp"))
        .arg("--root")
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut client = Client {
        input: child.stdin.take().unwrap(),
        output: BufReader::new(child.stdout.take().unwrap()),
        child,
        id: 0,
    };
    let initialize = client.request("initialize", json!({"protocolVersion":"2025-06-18"}));
    assert_eq!(initialize["protocolVersion"], "2025-06-18");
    assert_eq!(
        initialize["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let tools = client.request("tools/list", json!({}));
    assert!(tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "export_game"));
    assert_eq!(
        client.tool("import_model", json!({"path":"restricted.obj"}))["isError"],
        true
    );

    let group = client.ok(
        "object_create",
        json!({"name":"Group","shape":"Empty","position":[10,0,0]}),
    )["object"]["id"]
        .clone();
    let model = client.ok("import_model", json!({"path":"triangle.obj"}));
    let mesh = &model["object_id"];
    let script = client.ok(
        "script_asset_set",
        json!({"name":"Move","path":"move.rhai"}),
    );
    client.ok(
        "object_update",
        json!({"id":mesh,"parent":group,"position":[1,0,0],"script":{"asset":script["asset"]}}),
    );
    assert_eq!(
        client.tool("object_update", json!({"id":group,"parent":mesh}))["isError"],
        true
    );
    assert_eq!(
        client.tool("object_delete", json!({"id":group}))["isError"],
        true
    );
    assert_eq!(
        client.tool(
            "asset_remove",
            json!({"kind":"script","index":script["asset"]})
        )["isError"],
        true
    );
    assert_eq!(
        client.ok("simulate", json!({"frames":2}))["objects"][1]["position"],
        json!([1., 2., 0.])
    );
    assert_eq!(
        client.ok("scene_snapshot", json!({}))["objects"][1]["position"],
        json!([1., 0., 0.])
    );
    let passed = client.ok(
        "test_scene",
        json!({"frames":2,"assertions":[{"id":mesh,"position":[1,2,0]}]}),
    );
    assert_eq!(passed["passed"], true);
    let failed = client.ok(
        "test_scene",
        json!({"frames":2,"assertions":[{"id":mesh,"position":[1,3,0]}]}),
    );
    assert_eq!(failed["passed"], false);
    assert_eq!(failed["failures"][0]["field"], "position");
    let capture = client.tool(
        "scene_capture",
        json!({"path":"preview.png","width":128,"height":64}),
    );
    assert_eq!(capture["content"][1]["mimeType"], "image/png");
    assert_eq!(image::open(root.join("preview.png")).unwrap().height(), 64);
    assert_eq!(
        client.tool("scene_capture", json!({"path":"preview.png"}))["isError"],
        true
    );
    assert!(client.ok("project_list", json!({}))["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["name"] == "triangle.obj"));
    client.ok("undo", json!({}));
    assert!(client.ok("scene_snapshot", json!({}))["objects"][1]["parent"].is_null());
    client.ok("redo", json!({}));

    assert_eq!(
        client.tool("scene_save", json!({"path":"../escape.forge.json"}))["isError"],
        true
    );
    client.ok("scene_save", json!({"path":"scene.forge.json"}));
    assert_eq!(
        client.tool("scene_save", json!({"path":"scene.forge.json"}))["isError"],
        true
    );
    client.ok("scene_new", json!({}));
    let reopened = client.ok("scene_open", json!({"path":"scene.forge.json"}));
    assert_eq!(reopened["objects"][1]["parent"], group);
    assert_eq!(reopened["objects"][1]["id"], *mesh);

    client.ok("export_game", json!({"path":"game"}));
    let exported = Scene::decode(&fs::read(root.join("game/assets/scene.json")).unwrap()).unwrap();
    assert_eq!(
        exported.world_transforms()[1]
            .0
            .transform_point3(Vec3::ZERO),
        Vec3::new(11., 0., 0.)
    );
    assert_eq!(exported.objects[1].script_asset, Some(0));

    client.ok("object_delete", json!({"id":group,"recursive":true}));
    assert_eq!(client.ok("scene_snapshot", json!({}))["objects"], json!([]));
    client.ok("asset_remove", json!({"kind":"script","index":0}));
    client.ok("asset_remove", json!({"kind":"model","index":0}));
    let demo = client.ok("scene_new", json!({"physics_demo":true}));
    let id = |name: &str| {
        demo["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|object| object["name"] == name)
            .unwrap()["id"]
            .clone()
    };
    let rover = id("Rover");
    let floor = id("Floor");
    let beacon = id("Trigger Beacon");
    let barrier = id("Barrier");
    let settings = client.ok("scene_settings", json!({"bindings":{"forward":"I"}}));
    assert_eq!(settings["bindings"]["forward"], "I");
    assert_eq!(settings["bindings"]["backward"], "S");
    let result = client.ok(
        "test_scene",
        json!({
            "frames":240,"dt":0.016666667,"input":[1,0],
            "expect_events":[
                {"kind":"collision_enter","a":rover,"b":floor},
                {"kind":"trigger_enter","a":rover,"b":beacon},
                {"kind":"trigger_exit","a":rover,"b":beacon},
                {"kind":"collision_enter","a":rover,"b":barrier}
            ]
        }),
    );
    assert_eq!(result["passed"], true, "{result}");
}

#[test]
fn mcp_saves_and_exports_a_project_with_the_chosen_starting_scene() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge3d-mcp"))
        .arg("--root")
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut client = Client {
        input: child.stdin.take().unwrap(),
        output: BufReader::new(child.stdout.take().unwrap()),
        child,
        id: 0,
    };
    client.request("initialize", json!({"protocolVersion":"2025-06-18"}));
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let project = client.ok("project_demo", json!({}));
    assert_eq!(project["start"], "entrance");
    assert_eq!(project["models"].as_array().unwrap().len(), 1);
    let play = client.ok("project_simulate", json!({"frames":2,"input":[1.0,0.0]}));
    assert_eq!(play["scene"], "workshop");
    assert_eq!(play["visited"], json!(["entrance", "workshop"]));
    client.ok("project_select_scene", json!({"scene":"workshop"}));
    client.ok("prefab_instantiate", json!({"prefab":"workshop-arm"}));
    client.ok("project_start", json!({"scene":"workshop"}));
    client.ok("project_save", json!({"path":"demo.forge-project.json"}));
    let project = client.ok("project_open", json!({"path":"demo.forge-project.json"}));
    assert_eq!(project["start"], "workshop");
    assert_eq!(project["models"].as_array().unwrap().len(), 1);
    client.ok("export_game", json!({"path":"game"}));
    let primary = fs::read(root.join("game/assets/scene.json")).unwrap();
    let secondary = fs::read(root.join("game/assets/scenes.json")).unwrap();
    let bundled = forge3d::game::Game::decode(&primary, Some(&secondary)).unwrap();
    assert_eq!(bundled.start, "workshop");
    assert_eq!(bundled.scenes.len(), 2);
    assert_eq!(
        bundled.scenes["workshop"]
            .objects
            .iter()
            .filter(|object| object.name == "Skinned arm")
            .count(),
        2
    );
}

#[test]
fn mcp_campaign_simulation_exposes_ui_progress_and_sound_events() {
    let temp = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_forge3d-mcp"))
        .arg("--root")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut client = Client {
        input: child.stdin.take().unwrap(),
        output: BufReader::new(child.stdout.take().unwrap()),
        child,
        id: 0,
    };
    client.request("initialize", json!({"protocolVersion":"2025-06-18"}));
    client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let info = client.ok("engine_info", json!({}));
    assert_eq!(info["engine_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(info["mcp_tools"], 1);
    assert_eq!(info["rhai_api"], 1);
    assert_eq!(info["scene_schema"], 2);
    assert_eq!(info["project_schema"], 1);
    let campaign = client.ok("campaign_demo", json!({}));
    assert_eq!(campaign["start"], "menu");
    let validated = client.ok("project_validate", json!({}));
    assert_eq!(
        validated,
        json!({"valid":true,"scenes":4,"starting_scene":"menu"})
    );
    let menu = client.ok("presentation_get", json!({}));
    assert_eq!(menu["widgets"].as_array().unwrap().len(), 8);
    client.ok("presentation_set", json!({"presentation":menu}));
    let result = client.ok(
        "project_simulate",
        json!({
            "frames":120,"input":[1,0],
            "actions":[{"frame":0,"button":"quiet"},{"frame":1,"button":"start"}]
        }),
    );
    assert_eq!(result["visited"], json!(["menu", "entrance", "workshop"]));
    assert_eq!(result["progress"]["checkpoint"], "workshop");
    assert_eq!(result["sounds"], json!(["cue", "cue", "cue"]));
    assert!(result["script_errors"].as_array().unwrap().is_empty());
    client.ok("project_select_scene", json!({"scene":"entrance"}));
    let query = json!({"origin":[0,1.2,0],"direction":[0,0,-2],"max_distance":2});
    let closed = client.ok("physics_raycast", query.clone());
    assert!(closed["hit"]["object_id"].is_u64());
    let mut activate = query;
    activate["frames"] = json!(1);
    activate["controls"] = json!({"pressed":["e"]});
    let opened = client.ok("physics_raycast", activate);
    assert_eq!(opened["script_errors"], json!([]));
    assert!(opened["hit"].is_null());
}
