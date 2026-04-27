use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ToolCallEvent {
    pub tool_name: String,
    pub file_path: Option<PathBuf>,
    pub has_offset_or_limit: bool,
}

#[derive(Debug, Clone)]
pub enum Decision {
    PassThrough,
    Substitute { content: String },
}
