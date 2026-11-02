use super::*;
use std::cell::RefCell;

#[test]
fn poll_result_precedes_the_terminal_only_code() {
    let events = RefCell::new(Vec::new());
    render_poll_then_sas(
        || {
            events.borrow_mut().push("result");
            Ok(())
        },
        Some("012345"),
        |code| {
            assert_eq!(code, "012345");
            events.borrow_mut().push("code");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*events.borrow(), vec!["result", "code"]);
}

#[test]
fn failed_result_render_does_not_present_a_success_code() {
    let displayed = RefCell::new(false);
    let result = render_poll_then_sas(
        || {
            Err(ExitError::new(
                "io_error",
                1,
                "synthetic render failure",
                "retry",
            ))
        },
        Some("012345"),
        |_| {
            *displayed.borrow_mut() = true;
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(!*displayed.borrow());
}

#[test]
fn no_code_still_renders_the_safe_result() {
    let rendered = RefCell::new(false);
    render_poll_then_sas(
        || {
            *rendered.borrow_mut() = true;
            Ok(())
        },
        None,
        |_| panic!("no code should be displayed"),
    )
    .unwrap();
    assert!(*rendered.borrow());
}
