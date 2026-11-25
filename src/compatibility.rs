use serde::Serialize;

pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RHAI_API_VERSION: u32 = 1;
pub const MCP_TOOLS_VERSION: u32 = 1;

#[derive(Serialize)]
pub struct Compatibility {
    pub engine_version: &'static str,
    pub scene_schema: u32,
    pub project_schema: u32,
    pub progress_schema: u32,
    pub rhai_api: u32,
    pub mcp_tools: u32,
}

pub fn info() -> Compatibility {
    Compatibility {
        engine_version: ENGINE_VERSION,
        scene_schema: crate::settings::SCENE_VERSION,
        project_schema: crate::workspace::PROJECT_SCHEMA_VERSION,
        progress_schema: crate::progress::PROGRESS_SCHEMA_VERSION,
        rhai_api: RHAI_API_VERSION,
        mcp_tools: MCP_TOOLS_VERSION,
    }
}
