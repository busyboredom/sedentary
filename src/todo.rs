use cosmic::widget;
use uuid::Uuid;

use crate::config::{RecurrenceRule, TodoData};

/// Which part of a task row the cursor is hovering over during a drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropZone {
    Above,
    /// Middle region — nest inside this task as a subtask.
    Inside,
    Below,
}

pub(crate) struct Todo {
    pub(crate) id: Uuid,
    pub(crate) title: String,
    pub(crate) show_notes: bool,
    pub(crate) notes: widget::text_editor::Content,
    pub(crate) subtasks: Vec<Todo>,
    pub(crate) complete: bool,
    pub(crate) deadline: Option<jiff::Timestamp>,
    pub(crate) recurrence: Option<RecurrenceRule>,
    pub(crate) recurrence_int_d: Option<String>,
    pub(crate) recurrence_int_h: Option<String>,
    pub(crate) recurrence_int_m: Option<String>,
    pub(crate) deadline_time_input: Option<String>,
    pub(crate) collapsed: bool,
}

impl Todo {
    /// Forces the deadline to align with the next occurrence of the configured recurrence logic.
    pub(crate) fn apply_recurrence_to_deadline(&mut self) {
        if let Some(rule) = &self.recurrence {
            let now = jiff::Timestamp::now();

            // User specifically requested that for Intervals, we always reset to now + interval.
            if let crate::config::RecurrenceRule::Interval(_) = rule
                && let Some(next_ts) = rule.next_occurrence(now)
            {
                self.deadline = Some(next_ts);
                let zdt = next_ts.to_zoned(jiff::tz::TimeZone::system());
                self.deadline_time_input = Some(format!(
                    "{:02}:{:02}",
                    zdt.time().hour(),
                    zdt.time().minute()
                ));
                return;
            }

            let next_ts_opt = rule.first_occurrence_at_or_after(now);

            // If current deadline is valid AND there isn't a *sooner* valid occurrence,
            // we keep the current deadline. This allows adding an earlier day to jump to it.
            if let Some(d) = self.deadline
                && d >= now
                && rule.is_valid_occurrence(d)
            {
                if let Some(next_ts) = next_ts_opt {
                    // Only keep current if the new calculated `next_ts` isn't strictly sooner
                    // (e.g. if we had Friday and just added Tuesday, next_ts is Tuesday, d is Friday).
                    if d <= next_ts {
                        return;
                    }
                } else {
                    return;
                }
            }

            if let Some(next_ts) = next_ts_opt {
                self.deadline = Some(next_ts);
                let zdt = next_ts.to_zoned(jiff::tz::TimeZone::system());
                self.deadline_time_input = Some(format!(
                    "{:02}:{:02}",
                    zdt.time().hour(),
                    zdt.time().minute()
                ));
            } else if self.deadline.is_none() {
                self.deadline = Some(now);
            }
        }
    }
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
            deadline: self.deadline,
            recurrence: self.recurrence.clone(),
            recurrence_int_d: self.recurrence_int_d.clone(),
            recurrence_int_h: self.recurrence_int_h.clone(),
            recurrence_int_m: self.recurrence_int_m.clone(),
            deadline_time_input: self.deadline_time_input.clone(),
            collapsed: self.collapsed,
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
            deadline: None,
            recurrence: None,
            recurrence_int_d: None,
            recurrence_int_h: None,
            recurrence_int_m: None,
            deadline_time_input: None,
            collapsed: false,
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
            deadline: data.deadline,
            recurrence: data.recurrence.clone(),
            recurrence_int_d: match &data.recurrence {
                Some(RecurrenceRule::Interval(span)) => Some(span.get_days().to_string()),
                _ => None,
            },
            recurrence_int_h: match &data.recurrence {
                Some(RecurrenceRule::Interval(span)) => Some(span.get_hours().to_string()),
                _ => None,
            },
            recurrence_int_m: match &data.recurrence {
                Some(RecurrenceRule::Interval(span)) => Some(span.get_minutes().to_string()),
                _ => None,
            },
            deadline_time_input: if let Some(ts) = data.deadline {
                let zdt = ts.to_zoned(jiff::tz::TimeZone::UTC);
                Some(format!(
                    "{:02}:{:02}",
                    zdt.time().hour(),
                    zdt.time().minute()
                ))
            } else {
                None
            },
            collapsed: data.collapsed,
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
            deadline: todo.deadline,
            recurrence: todo.recurrence.clone(),
            collapsed: todo.collapsed,
        }
    }
}

/// A collection of to-do items with tree manipulation operations.
pub(crate) struct TodoList {
    items: Vec<Todo>,
}

impl TodoList {
    pub(crate) fn new(items: Vec<Todo>) -> Self {
        let mut list = Self { items };
        list.sort();
        list
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

    /// Find a Todo by ID (recursive).
    pub(crate) fn find(&self, id: Uuid) -> Option<&Todo> {
        fn find_in(todos: &[Todo], id: Uuid) -> Option<&Todo> {
            for todo in todos {
                if todo.id == id {
                    return Some(todo);
                }
                if let Some(found) = find_in(&todo.subtasks, id) {
                    return Some(found);
                }
            }
            None
        }
        find_in(&self.items, id)
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
        ) -> Result<(), Box<Todo>> {
            if let Some(pos) = todos.iter().position(|t| t.id == target_id) {
                todos.insert(pos + offset, new_todo);
                return Ok(());
            }
            let mut passed = new_todo;
            for todo in todos {
                match insert_in(&mut todo.subtasks, target_id, passed, offset) {
                    Ok(()) => return Ok(()),
                    Err(returned) => passed = *returned,
                }
            }
            Err(Box::new(passed))
        }
        insert_in(&mut self.items, target_id, new_todo, offset).is_ok()
    }

    pub(crate) fn insert_before(&mut self, target_id: Uuid, new_todo: Todo) -> bool {
        self.insert_relative(target_id, new_todo, 0)
    }

    pub(crate) fn insert_after(&mut self, target_id: Uuid, new_todo: Todo) -> bool {
        self.insert_relative(target_id, new_todo, 1)
    }

    /// Sorts all tasks (and subtasks) by deadline. Tasks with deadlines come first,
    /// ordered earliest to latest. Tasks without deadlines are sorted to the end.
    pub(crate) fn sort_by_deadline(&mut self) {
        fn sort_in(todos: &mut [Todo]) {
            for todo in todos.iter_mut() {
                sort_in(&mut todo.subtasks);
            }
            todos.sort_by(|a, b| match (a.deadline, b.deadline) {
                (Some(d1), Some(d2)) => d1.cmp(&d2),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            });
        }
        sort_in(&mut self.items);
    }

    /// Recursively move completed tasks to the bottom of their respective arrays.
    pub(crate) fn pop_completed(&mut self) {
        fn pop_in(todos: &mut Vec<Todo>) {
            for todo in todos.iter_mut() {
                pop_in(&mut todo.subtasks);
            }
            // stable_partition is not in std, so we use retain + extend
            let mut completed = Vec::new();
            let mut i = 0;
            while i < todos.len() {
                if todos[i].complete {
                    completed.push(todos.remove(i));
                } else {
                    i += 1;
                }
            }
            todos.extend(completed);
        }
        pop_in(&mut self.items);
    }

    /// Fully sort the list by deadline, and then ensure completed tasks are at the bottom.
    pub(crate) fn sort(&mut self) {
        self.sort_by_deadline();
        self.pop_completed();
    }

    /// Returns a reference to the incomplete task with the earliest (or most
    /// overdue) deadline, searching recursively through subtasks.
    pub(crate) fn next_due(&self) -> Option<&Todo> {
        fn earliest<'a>(todos: &'a [Todo], best: Option<&'a Todo>) -> Option<&'a Todo> {
            let mut best = best;
            for todo in todos {
                if !todo.complete
                    && let Some(dl) = todo.deadline
                    && best.is_none_or(|b| dl < b.deadline.unwrap())
                {
                    best = Some(todo);
                }
                best = earliest(&todo.subtasks, best);
            }
            best
        }
        earliest(&self.items, None)
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

    /// Check completed recurring tasks. If `now` is past the halfway point to their next deadline,
    /// uncheck them and advance the deadline. Returns true if any task was changed.
    pub(crate) fn tick_recurrences(&mut self, now: jiff::Timestamp) -> bool {
        fn tick_in(todos: &mut [Todo], now: jiff::Timestamp) -> bool {
            let mut changed = false;
            for todo in todos.iter_mut() {
                if tick_in(&mut todo.subtasks, now) {
                    changed = true;
                }

                if todo.complete
                    && let (Some(mut deadline), Some(rule)) = (todo.deadline, &todo.recurrence)
                {
                    let mut advanced = false;
                    while let Some(next_deadline) = rule.next_occurrence(deadline) {
                        if next_deadline <= deadline {
                            break;
                        }
                        let total_diff = next_deadline.duration_since(deadline);
                        let trigger_point = deadline
                            .checked_add((total_diff / 4) * 3)
                            .unwrap_or(deadline);

                        if now >= trigger_point {
                            deadline = next_deadline;
                            todo.deadline = Some(deadline);
                            advanced = true;
                            changed = true;
                        } else {
                            break;
                        }
                    }

                    if advanced {
                        todo.complete = false;
                        // Sync time input if we advanced
                        let zdt = deadline.to_zoned(jiff::tz::TimeZone::system());
                        todo.deadline_time_input = Some(format!(
                            "{:02}:{:02}",
                            zdt.time().hour(),
                            zdt.time().minute()
                        ));
                    }
                }
            }
            changed
        }
        tick_in(&mut self.items, now)
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
        let (mut list, a, _b, _c) = sample_list();
        let removed = list.remove(a).unwrap();
        list.push(removed);
        assert_eq!(list.items()[0].title, "B");
        assert_eq!(list.items()[2].title, "A");
    }

    #[test]
    fn pop_completed_moves_to_bottom() {
        let (mut list, a, _b, _c) = sample_list();
        // Complete A
        list.find_mut(a).unwrap().complete = true;
        list.pop_completed();
        // A should be moved to the end
        assert_eq!(list.items()[0].title, "B");
        assert_eq!(list.items()[1].title, "C");
        assert_eq!(list.items()[2].title, "A");
    }

    #[test]
    fn tick_recurrences_updates_deadline() {
        let mut todo = make_todo(Uuid::new_v4(), "Recurring");
        todo.complete = true;
        // set deadline 2 days ago
        let past_deadline = jiff::Timestamp::now()
            .checked_sub(jiff::Span::new().hours(48))
            .unwrap();
        todo.deadline = Some(past_deadline);
        // set recurrence: interval 1 day
        todo.recurrence = Some(crate::config::RecurrenceRule::Interval(
            jiff::Span::new().hours(24),
        ));

        let mut list = TodoList::new(vec![todo]);

        let changed = list.tick_recurrences(jiff::Timestamp::now());
        assert!(changed);

        let updated = list.items().first().unwrap();
        assert!(!updated.complete, "Task should be unchecked");
        assert!(
            updated.deadline.unwrap() > past_deadline,
            "Deadline should be advanced"
        );
    }

    #[test]
    fn overdue_move_to_future() {
        let mut todo = Todo::default();
        let now = jiff::Timestamp::now();
        let yesterday = now.checked_sub(jiff::Span::new().hours(24)).unwrap();
        todo.deadline = Some(yesterday);

        // Set recurrence to 1 day interval
        todo.recurrence = Some(crate::config::RecurrenceRule::Interval(
            jiff::Span::new().days(1),
        ));

        // Before fix, this would have returned early and kept it yesterday
        todo.apply_recurrence_to_deadline();

        // Check that deadline is now (or at least >= now)
        assert!(todo.deadline.unwrap() >= now);
    }

    #[test]
    fn interval_update_resets_deadline() {
        let mut todo = Todo::default();
        let now = jiff::Timestamp::now();
        let tomorrow = now.checked_add(jiff::Span::new().hours(24)).unwrap();
        todo.deadline = Some(tomorrow);

        // Set recurrence to 1 hour interval
        todo.recurrence = Some(crate::config::RecurrenceRule::Interval(
            jiff::Span::new().hours(1),
        ));
        todo.apply_recurrence_to_deadline();

        // Check that deadline is now reset to roughly now + 1 hour (much sooner than tomorrow)
        assert!(todo.deadline.unwrap() < tomorrow);
    }

    #[test]
    fn sort_by_deadline_orders_correctly() {
        let now = jiff::Timestamp::now();
        let tomorrow = now.checked_add(jiff::Span::new().hours(24)).unwrap();
        let yesterday = now.checked_sub(jiff::Span::new().hours(24)).unwrap();

        let mut a = make_todo(Uuid::new_v4(), "A");
        a.deadline = None;

        let mut b = make_todo(Uuid::new_v4(), "B");
        b.deadline = Some(now);

        let mut c = make_todo(Uuid::new_v4(), "C");
        c.deadline = Some(tomorrow);

        let mut d = make_todo(Uuid::new_v4(), "D");
        d.deadline = Some(yesterday);

        let list = TodoList::new(vec![a, b, c, d]);
        // list automatically sorts on init now

        assert_eq!(list.items()[0].title, "D"); // Yesterday
        assert_eq!(list.items()[1].title, "B"); // Now
        assert_eq!(list.items()[2].title, "C"); // Tomorrow
        assert_eq!(list.items()[3].title, "A"); // None
    }

    #[test]
    fn tick_recurrences_3_4_point() {
        let mut todo = make_todo(Uuid::new_v4(), "Recurring 3/4");
        todo.complete = true;

        let now = jiff::Timestamp::now();
        let past_deadline = now.checked_sub(jiff::Span::new().hours(100)).unwrap();
        todo.deadline = Some(past_deadline);
        // Interval is 100 hours. The next deadline is `now`.
        // The previous deadline was 100 hours ago.
        // 3/4 of 100 hours is 75 hours.
        // So it should trigger if `now` >= `past_deadline + 75 hours`.
        // Since `now` is exactly `past_deadline + 100 hours`, it should trigger.
        todo.recurrence = Some(crate::config::RecurrenceRule::Interval(
            jiff::Span::new().hours(100),
        ));

        let mut list = TodoList::new(vec![todo]);
        let changed = list.tick_recurrences(now);
        assert!(changed);
        assert!(!list.items()[0].complete);

        // Test just before 3/4 point
        let mut todo2 = make_todo(Uuid::new_v4(), "Recurring Not Yet");
        todo2.complete = true;
        // Deadline was 100 hours ago relative to the target 'next' deadline.
        // Let's set it to exactly 74 hours past the deadline.
        let deadline2 = now.checked_sub(jiff::Span::new().hours(74)).unwrap();
        todo2.deadline = Some(deadline2);
        todo2.recurrence = Some(crate::config::RecurrenceRule::Interval(
            jiff::Span::new().hours(100),
        ));

        let mut list2 = TodoList::new(vec![todo2]);
        let changed2 = list2.tick_recurrences(now);
        assert!(!changed2);
        assert!(list2.items()[0].complete);
    }
}
