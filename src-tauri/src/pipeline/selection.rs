//! Read the focused app's selection by copying it, then put the clipboard back.
//!
//! A sentinel is written first so "nothing selected" is distinct from "the
//! selection is the same text that was already on the clipboard".

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedSelection {
    pub text: Option<String>,
    pub note: String,
}

pub fn capture(
    mut read: impl FnMut() -> Option<String>,
    mut write: impl FnMut(&str) -> Result<(), String>,
    mut copy: impl FnMut() -> Result<(), String>,
) -> CapturedSelection {
    let previous = read();
    let sentinel = format!(
        "\u{2060}newtype-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    );
    if let Err(err) = write(&sentinel) {
        return CapturedSelection {
            text: None,
            note: format!("Could not stage the clipboard to read a selection: {err}"),
        };
    }
    let copied = copy();
    let current = read();
    let restored = match &previous {
        Some(text) => write(text),
        None => write(""),
    };
    if let Err(err) = copied {
        let extra = if restored.is_err() {
            " The previous clipboard could not be restored."
        } else {
            ""
        };
        return CapturedSelection {
            text: None,
            note: format!("Could not copy a selection: {err}.{extra}"),
        };
    }
    let note = match restored {
        Ok(()) => String::new(),
        Err(err) => format!("The previous clipboard could not be restored: {err}"),
    };
    let text = match current {
        Some(value) if !value.is_empty() && value != sentinel => Some(value),
        _ => None,
    };
    CapturedSelection { text, note }
}

pub fn capture_focused() -> CapturedSelection {
    capture(super::insert::clipboard_get, super::insert::clipboard_set, || {
        std::thread::sleep(std::time::Duration::from_millis(40));
        super::insert::copy_shortcut()?;
        std::thread::sleep(std::time::Duration::from_millis(120));
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    fn board(initial: Option<&str>) -> Rc<RefCell<Option<String>>> {
        Rc::new(RefCell::new(initial.map(str::to_string)))
    }

    #[test]
    fn a_changed_clipboard_is_the_selection_and_the_previous_text_comes_back() {
        let board = board(Some("previous secret"));
        let writes = Rc::new(RefCell::new(Vec::new()));
        let read = Rc::clone(&board);
        let write_board = Rc::clone(&board);
        let write_log = Rc::clone(&writes);
        let copy_board = Rc::clone(&board);
        let captured = capture(
            move || read.borrow().clone(),
            move |text| {
                write_log.borrow_mut().push(text.to_string());
                *write_board.borrow_mut() = Some(text.to_string());
                Ok(())
            },
            move || {
                *copy_board.borrow_mut() = Some("selected paragraph".into());
                Ok(())
            },
        );
        assert_eq!(captured.text.as_deref(), Some("selected paragraph"));
        assert!(captured.note.is_empty());
        assert_eq!(board.borrow().as_deref(), Some("previous secret"));
        assert!(writes.borrow().len() >= 2);
    }

    #[test]
    fn an_unchanged_sentinel_means_nothing_was_selected() {
        let board = board(Some("already there"));
        let read = Rc::clone(&board);
        let write_board = Rc::clone(&board);
        let captured = capture(
            move || read.borrow().clone(),
            move |text| {
                *write_board.borrow_mut() = Some(text.to_string());
                Ok(())
            },
            || Ok(()),
        );
        assert_eq!(captured.text, None);
        assert_eq!(board.borrow().as_deref(), Some("already there"));
    }

    #[test]
    fn a_failed_copy_still_restores_the_previous_clipboard() {
        let board = board(None);
        let read = Rc::clone(&board);
        let write_board = Rc::clone(&board);
        let captured = capture(
            move || read.borrow().clone(),
            move |text| {
                *write_board.borrow_mut() = Some(text.to_string());
                Ok(())
            },
            || Err("accessibility is off".into()),
        );
        assert_eq!(captured.text, None);
        assert!(captured.note.contains("accessibility"));
        assert_eq!(board.borrow().as_deref(), Some(""));
    }

    #[test]
    fn an_empty_previous_clipboard_is_cleared_after_the_sentinel() {
        let board = board(None);
        let read = Rc::clone(&board);
        let write_board = Rc::clone(&board);
        let copy_board = Rc::clone(&board);
        let captured = capture(
            move || read.borrow().clone(),
            move |text| {
                *write_board.borrow_mut() = Some(text.to_string());
                Ok(())
            },
            move || {
                *copy_board.borrow_mut() = Some("kept selection".into());
                Ok(())
            },
        );
        assert_eq!(captured.text.as_deref(), Some("kept selection"));
        assert_eq!(board.borrow().as_deref(), Some(""));
    }
}
