
use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub(crate) struct Config {
    pub(crate) work_minutes: u32,
    pub(crate) break_minutes: u32,
    pub(crate) todos: Vec<TodoData>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            work_minutes: 30,
            break_minutes: 5,
            todos: Vec::new(),
        }
    }
}

/// Serializable snapshot of a to-do item (for persistence only).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TodoData {
    pub(crate) id: Uuid,
    pub(crate) title: String,
    pub(crate) notes: String,
    pub(crate) subtasks: Vec<TodoData>,
    pub(crate) complete: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tododata_nested_roundtrip() {
        let child = TodoData {
            id: Uuid::new_v4(),
            title: "Child".to_string(),
            notes: String::new(),
            subtasks: Vec::new(),
            complete: false,
        };
        let parent = TodoData {
            id: Uuid::new_v4(),
            title: "Parent".to_string(),
            notes: "Parent notes".to_string(),
            subtasks: vec![child],
            complete: false,
        };
        let json = serde_json::to_string(&parent).unwrap();
        let restored: TodoData = serde_json::from_str(&json).unwrap();
        assert_eq!(parent, restored);
        assert_eq!(restored.subtasks.len(), 1);
        assert_eq!(restored.subtasks[0].title, "Child");
    }
}
