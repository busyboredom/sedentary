use cosmic::widget;
use uuid::Uuid;

use crate::config::TodoData;

/// Which part of a task row the cursor is hovering over during a drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropZone {
    Above,
    /// Middle region — nest inside this task as a subtask.
    Inside,
    Below,
}

#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Todo {
    pub(crate) id: Uuid,
    pub(crate) title: String,
    pub(crate) show_notes: bool,
    pub(crate) notes: widget::text_editor::Content,
    pub(crate) subtasks: Vec<Todo>,
    pub(crate) complete: bool,
}

impl Clone for Todo {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            title: self.title.clone(),
            show_notes: self.show_notes,
            notes: widget::text_editor::Content::with_text(&self.notes.text()),
            subtasks: self.subtasks.clone(),
            complete: self.complete,
        }
    }
}

impl Default for Todo {
    fn default() -> Self {
        Todo {
            id: Uuid::new_v4(),
            title: "New task title".to_string(),
            show_notes: false,
            notes: widget::text_editor::Content::new(),
            subtasks: Vec::new(),
            complete: false,
        }
    }
}

impl From<TodoData> for Todo {
    fn from(data: TodoData) -> Self {
        Self {
            id: data.id,
            title: data.title,
            show_notes: false,
            notes: widget::text_editor::Content::with_text(&data.notes),
            subtasks: data.subtasks.into_iter().map(Self::from).collect(),
            complete: data.complete,
        }
    }
}

impl From<&Todo> for TodoData {
    fn from(todo: &Todo) -> Self {
        Self {
            id: todo.id,
            title: todo.title.clone(),
            notes: todo.notes.text(),
            subtasks: todo.subtasks.iter().map(Self::from).collect(),
            complete: todo.complete,
        }
    }
}

/// A collection of to-do items with tree manipulation operations.
pub(crate) struct TodoList {
    items: Vec<Todo>,
}

impl TodoList {
    pub(crate) fn new(items: Vec<Todo>) -> Self {
        Self { items }
    }

    /// Returns a slice of the top-level items.
    #[cfg(test)]
    pub(crate) fn items(&self) -> &[Todo] {
        &self.items
    }

    /// Returns the number of top-level items.
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    /// Appends a todo to the end of the top-level list.
    pub(crate) fn push(&mut self, todo: Todo) {
        self.items.push(todo);
    }

    /// Returns an iterator over the top-level items.
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, Todo> {
        self.items.iter()
    }

    /// Find a mutable reference to a Todo by ID (recursive).
    pub(crate) fn find_mut(&mut self, id: Uuid) -> Option<&mut Todo> {
        fn find_in(todos: &mut [Todo], id: Uuid) -> Option<&mut Todo> {
            for todo in todos {
                if todo.id == id {
                    return Some(todo);
                }
                if let Some(found) = find_in(&mut todo.subtasks, id) {
                    return Some(found);
                }
            }
            None
        }
        find_in(&mut self.items, id)
    }

    /// Remove a Todo by ID (recursive), and return it if found.
    pub(crate) fn remove(&mut self, id: Uuid) -> Option<Todo> {
        fn remove_from(todos: &mut Vec<Todo>, id: Uuid) -> Option<Todo> {
            if let Some(pos) = todos.iter().position(|t| t.id == id) {
                return Some(todos.remove(pos));
            }
            for todo in todos {
                if let Some(removed) = remove_from(&mut todo.subtasks, id) {
                    return Some(removed);
                }
            }
            None
        }
        remove_from(&mut self.items, id)
    }

    /// Insert a Todo at an offset relative to a target Todo.
    /// `offset = 0` inserts before, `offset = 1` inserts after.
    fn insert_relative(&mut self, target_id: Uuid, new_todo: Todo, offset: usize) -> bool {
        fn insert_in(
            todos: &mut Vec<Todo>,
            target_id: Uuid,
            new_todo: Todo,
            offset: usize,
        ) -> Result<(), Todo> {
            if let Some(pos) = todos.iter().position(|t| t.id == target_id) {
                todos.insert(pos + offset, new_todo);
                return Ok(());
            }
            let mut passed = new_todo;
            for todo in todos {
                match insert_in(&mut todo.subtasks, target_id, passed, offset) {
                    Ok(()) => return Ok(()),
                    Err(returned) => passed = returned,
                }
            }
            Err(passed)
        }
        insert_in(&mut self.items, target_id, new_todo, offset).is_ok()
    }

    pub(crate) fn insert_before(&mut self, target_id: Uuid, new_todo: Todo) -> bool {
        self.insert_relative(target_id, new_todo, 0)
    }

    pub(crate) fn insert_after(&mut self, target_id: Uuid, new_todo: Todo) -> bool {
        self.insert_relative(target_id, new_todo, 1)
    }

    /// Nest a Todo as a subtask of a target Todo.
    pub(crate) fn nest_inside(&mut self, target_id: Uuid, new_todo: Todo) -> bool {
        if let Some(target) = self.find_mut(target_id) {
            target.subtasks.push(new_todo);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a Todo with a specific ID and title.
    fn make_todo(id: Uuid, title: &str) -> Todo {
        Todo {
            id,
            title: title.to_string(),
            ..Default::default()
        }
    }

    /// Helper: create a `TodoList` with three top-level items.
    fn sample_list() -> (TodoList, Uuid, Uuid, Uuid) {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let list = TodoList::new(vec![
            make_todo(a, "A"),
            make_todo(b, "B"),
            make_todo(c, "C"),
        ]);
        (list, a, b, c)
    }

    #[test]
    fn remove_top_level() {
        let (mut list, a, _, _) = sample_list();
        let removed = list.remove(a);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().title, "A");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn remove_nested() {
        let a = Uuid::new_v4();
        let child = Uuid::new_v4();
        let mut parent = make_todo(a, "parent");
        parent.subtasks.push(make_todo(child, "child"));
        let mut list = TodoList::new(vec![parent]);

        let removed = list.remove(child);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().title, "child");
        assert!(list.items()[0].subtasks.is_empty());
    }

    #[test]
    fn insert_before_first() {
        let (mut list, a, _, _) = sample_list();
        let new_id = Uuid::new_v4();
        assert!(list.insert_before(a, make_todo(new_id, "new")));
        assert_eq!(list.items()[0].id, new_id);
        assert_eq!(list.items()[1].id, a);
        assert_eq!(list.len(), 4);
    }

    #[test]
    fn insert_before_middle() {
        let (mut list, _, b, _) = sample_list();
        let new_id = Uuid::new_v4();
        assert!(list.insert_before(b, make_todo(new_id, "new")));
        assert_eq!(list.items()[1].id, new_id);
        assert_eq!(list.items()[2].id, b);
    }

    #[test]
    fn insert_after_last() {
        let (mut list, _, _, c) = sample_list();
        let new_id = Uuid::new_v4();
        assert!(list.insert_after(c, make_todo(new_id, "new")));
        assert_eq!(list.items()[3].id, new_id);
        assert_eq!(list.len(), 4);
    }

    #[test]
    fn insert_after_nested() {
        let a = Uuid::new_v4();
        let child = Uuid::new_v4();
        let mut parent = make_todo(a, "parent");
        parent.subtasks.push(make_todo(child, "child"));
        let mut list = TodoList::new(vec![parent]);

        let new_id = Uuid::new_v4();
        assert!(list.insert_after(child, make_todo(new_id, "new")));
        assert_eq!(list.items()[0].subtasks.len(), 2);
        assert_eq!(list.items()[0].subtasks[1].id, new_id);
    }

    #[test]
    fn nest_inside_target() {
        let (mut list, _, b, _) = sample_list();
        let new_id = Uuid::new_v4();
        assert!(list.nest_inside(b, make_todo(new_id, "nested")));
        let target = list.find_mut(b).unwrap();
        assert_eq!(target.subtasks.len(), 1);
        assert_eq!(target.subtasks[0].id, new_id);
    }

    #[test]
    fn drag_reorder_sequence() {
        // Simulate dragging C before A: remove(C) -> insert_before(A, C)
        let (mut list, a, _b, c) = sample_list();
        let removed = list.remove(c).unwrap();
        assert!(list.insert_before(a, removed));
        assert_eq!(list.items()[0].title, "C");
        assert_eq!(list.items()[1].title, "A");
        assert_eq!(list.items()[2].title, "B");
    }

    #[test]
    fn drag_to_end() {
        // Simulate dragging A to end: remove(A) -> push(A)
        let (mut list, a, _, _) = sample_list();
        let removed = list.remove(a).unwrap();
        list.push(removed);
        assert_eq!(list.items()[0].title, "B");
        assert_eq!(list.items()[1].title, "C");
        assert_eq!(list.items()[2].title, "A");
    }
}
