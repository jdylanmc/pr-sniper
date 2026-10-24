//! Host panic capture at review boundaries: a host defect keeps its message and
//! location as typed evidence instead of vanishing with an unwound task.
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    cell::RefCell,
    future::Future,
    panic::{self, AssertUnwindSafe},
    sync::Once,
    task::Poll,
};

/// Bytes of panic message kept; the location is the reliable part.
const MESSAGE_BYTES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPanic {
    pub message: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
}

thread_local! {
    static LAST: RefCell<Option<HostPanic>> = const { RefCell::new(None) };
}

static HOOK: Once = Once::new();

fn message(payload: &(dyn Any + Send)) -> String {
    let text = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic payload");
    let mut end = text.len().min(MESSAGE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Chains a hook that remembers the panicking thread's last panic location.
/// The previous hook still runs, so stderr reporting is unchanged.
pub fn install() {
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let location = info.location();
            let captured = HostPanic {
                message: message(info.payload()),
                file: location.map_or_else(|| "unknown".into(), |l| l.file().to_owned()),
                line: location.map_or(0, |l| l.line()),
                column: location.map_or(0, |l| l.column()),
            };
            let _ = LAST.try_with(|last| {
                if let Ok(mut last) = last.try_borrow_mut() {
                    *last = Some(captured);
                }
            });
            previous(info);
        }));
    });
}

fn forget() {
    let _ = LAST.try_with(|last| {
        if let Ok(mut last) = last.try_borrow_mut() {
            *last = None;
        }
    });
}

fn caught(payload: Box<dyn Any + Send>) -> HostPanic {
    LAST.try_with(|last| last.try_borrow_mut().ok().and_then(|mut last| last.take()))
        .ok()
        .flatten()
        .unwrap_or_else(|| HostPanic {
            message: message(payload.as_ref()),
            file: "unknown".into(),
            line: 0,
            column: 0,
        })
}

/// Runs `work`, returning a panic it raised as typed evidence.
pub fn catch<R>(work: impl FnOnce() -> R) -> Result<R, HostPanic> {
    install();
    forget();
    panic::catch_unwind(AssertUnwindSafe(work)).map_err(caught)
}

/// Polls `future` to completion, returning a panic raised while polling it.
/// The future is never polled again after it panicked.
pub async fn catch_future<F: Future>(future: F) -> Result<F::Output, HostPanic> {
    install();
    let mut future = std::pin::pin!(future);
    std::future::poll_fn(|cx| {
        forget();
        match panic::catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(cx))) {
            Ok(Poll::Ready(output)) => Poll::Ready(Ok(output)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(payload) => Poll::Ready(Err(caught(payload))),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caught_panic_keeps_its_message_and_location() {
        let line = line!() + 1;
        let caught = catch::<()>(|| panic!("index {} out of range", 4)).unwrap_err();
        assert_eq!(caught.message, "index 4 out of range");
        assert_eq!(caught.file, file!());
        assert_eq!(caught.line, line);
        assert_eq!(catch(|| 7), Ok(7));
    }

    #[test]
    fn a_long_message_is_bounded_on_a_char_boundary() {
        let caught = catch::<()>(|| panic!("{}", "é".repeat(400))).unwrap_err();
        assert!(caught.message.len() <= MESSAGE_BYTES && caught.message.starts_with('é'));
    }

    #[tokio::test]
    async fn a_panicking_future_is_caught_with_its_location() {
        let line = line!() + 4;
        let caught = catch_future(async {
            tokio::task::yield_now().await;
            let empty: Vec<u8> = Vec::new();
            empty[0]
        })
        .await
        .unwrap_err();
        assert_eq!(caught.file, file!());
        assert_eq!(caught.line, line);
        assert!(caught.message.contains("index out of bounds"));
        assert_eq!(catch_future(async { 3 }).await, Ok(3));
    }
}
