//! UI Automation objects stay on one dedicated COM thread. Only lease ids
//! cross the coordinator's threads; no interface is marked Send unsafely.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern2,
    IUIAutomationTextRange, TextPatternRangeEndpoint_Start, UIA_TextPattern2Id,
};

const WAIT: Duration = Duration::from_millis(350);
const CAP: usize = 256;

#[derive(Debug)]
pub struct FocusLease {
    id: u64,
    usable: AtomicBool,
}

impl PartialEq for FocusLease {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for FocusLease {}
impl Drop for FocusLease {
    fn drop(&mut self) {
        if let Some(Some(tx)) = WORKER.get() {
            let _ = tx.try_send(Request::Release(self.id));
        }
    }
}

enum Request {
    Capture {
        pid: u32,
        due: Instant,
        answer: mpsc::Sender<Option<Arc<FocusLease>>>,
    },
    Restore {
        id: u64,
        pid: u32,
        due: Instant,
        answer: mpsc::Sender<bool>,
    },
    Release(u64),
    InvalidateCaret(u64),
    NativeFocus {
        window: isize,
        child: isize,
        pid: u32,
        due: Instant,
        answer: mpsc::Sender<bool>,
    },
    Refresh {
        id: u64,
        answer: mpsc::Sender<()>,
    },
}
static WORKER: OnceLock<Option<mpsc::SyncSender<Request>>> = OnceLock::new();

fn worker() -> Option<&'static mpsc::SyncSender<Request>> {
    WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel(64);
            let (ready_tx, ready_rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("destination-focus".into())
                .spawn(move || {
                    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                    if !initialized {
                        let _ = ready_tx.send(false);
                        return;
                    }
                    // AttachThreadInput requires a Win32 message queue on both threads.
                    // COM's MTA initialization alone does not guarantee that queue.
                    let mut message = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                    let _ = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                            &mut message,
                            windows::Win32::Foundation::HWND::default(),
                            0,
                            0,
                            windows::Win32::UI::WindowsAndMessaging::PM_NOREMOVE,
                        )
                    };
                    let automation: windows::core::Result<IUIAutomation> =
                        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) };
                    if let Ok(automation) = automation {
                        let _ = ready_tx.send(true);
                        run(automation, rx);
                    } else {
                        let _ = ready_tx.send(false);
                    }
                    unsafe {
                        CoUninitialize();
                    }
                })
                .ok()?;
            ready_rx
                .recv_timeout(WAIT)
                .ok()
                .filter(|ready| *ready)
                .map(|_| tx)
        })
        .as_ref()
}

struct CapturedElement {
    element: IUIAutomationElement,
    caret: Option<IUIAutomationTextRange>,
    lease: std::sync::Weak<FocusLease>,
}

fn caret_of(element: &IUIAutomationElement) -> Option<IUIAutomationTextRange> {
    let pattern: IUIAutomationTextPattern2 =
        unsafe { element.GetCurrentPatternAs(UIA_TextPattern2Id) }.ok()?;
    let mut active = windows::Win32::Foundation::BOOL(0);
    let range = unsafe { pattern.GetCaretRange(&mut active) }.ok()?;
    active.as_bool().then_some(range)
}

fn run(automation: IUIAutomation, rx: mpsc::Receiver<Request>) {
    let mut elements: HashMap<u64, CapturedElement> = HashMap::new();
    let mut next = 0u64;
    while let Ok(request) = rx.recv() {
        // A full queue may drop a Release notification. Weak ownership still
        // lets this COM thread reclaim every abandoned element on its next turn.
        elements.retain(|_, captured| captured.lease.strong_count() > 0);
        match request {
            Request::Capture { pid, due, answer } => {
                let element = if Instant::now() < due && elements.len() < CAP {
                    unsafe { automation.GetFocusedElement() }
                        .ok()
                        .filter(|element| {
                            unsafe { element.CurrentProcessId() }.ok() == Some(pid as i32)
                        })
                } else {
                    None
                };
                if let Some(element) = element {
                    next += 1;
                    let caret = caret_of(&element);
                    let lease = Arc::new(FocusLease {
                        id: next,
                        usable: AtomicBool::new(true),
                    });
                    tracing::debug!(
                        native_caret_bookmark = caret.is_some(),
                        "destination accessibility capture"
                    );
                    elements.insert(
                        next,
                        CapturedElement {
                            element,
                            caret,
                            lease: Arc::downgrade(&lease),
                        },
                    );
                    if answer.send(Some(lease)).is_err() {
                        elements.remove(&next);
                    }
                } else {
                    let _ = answer.send(None);
                }
            }
            Request::Restore {
                id,
                pid,
                due,
                answer,
            } => {
                let restored = elements.get(&id).is_some_and(|captured| {
                    let element = &captured.element;
                    if Instant::now() >= due
                        || unsafe { element.CurrentProcessId() }.ok() != Some(pid as i32)
                    {
                        return false;
                    }
                    let already =
                        unsafe { automation.GetFocusedElement() }
                            .ok()
                            .is_some_and(|focused| {
                                unsafe { automation.CompareElements(element, &focused) }
                                    .is_ok_and(|same| same.as_bool())
                            });
                    if !already && unsafe { element.SetFocus() }.is_err() {
                        return false;
                    }
                    if Instant::now() >= due {
                        return false;
                    }
                    if let Some(caret) = &captured.caret {
                        if unsafe { caret.Select() }.is_err()
                            || !caret_of(element).is_some_and(|current| {
                                unsafe {
                                    current.CompareEndpoints(
                                        TextPatternRangeEndpoint_Start,
                                        caret,
                                        TextPatternRangeEndpoint_Start,
                                    )
                                }
                                .is_ok_and(|distance| distance == 0)
                            })
                        {
                            return false;
                        }
                    }
                    unsafe { automation.GetFocusedElement() }
                        .ok()
                        .is_some_and(|focused| {
                            unsafe { automation.CompareElements(element, &focused) }
                                .is_ok_and(|same| same.as_bool())
                        })
                });
                let _ = answer.send(restored);
            }
            Request::Release(id) => {
                elements.remove(&id);
            }
            Request::InvalidateCaret(id) => {
                if let Some(captured) = elements.get_mut(&id) {
                    captured.caret = None;
                }
            }
            Request::NativeFocus {
                window,
                child,
                pid,
                due,
                answer,
            } => {
                let restored = Instant::now() < due && restore_native_now(window, child, pid);
                let _ = answer.send(restored && Instant::now() < due);
            }
            Request::Refresh { id, answer } => {
                if let Some(captured) = elements.get_mut(&id) {
                    // SendInput only reports queued events. Do not save a stale
                    // caret as the next chunk's position before the editor moves.
                    if let Some(previous) = captured.caret.take() {
                        let deadline = Instant::now() + Duration::from_millis(150);
                        loop {
                            if let Some(current) = caret_of(&captured.element) {
                                if unsafe {
                                    current.CompareEndpoints(
                                        TextPatternRangeEndpoint_Start,
                                        &previous,
                                        TextPatternRangeEndpoint_Start,
                                    )
                                }
                                .is_ok_and(|distance| distance != 0)
                                {
                                    captured.caret = Some(current);
                                    break;
                                }
                            }
                            if Instant::now() >= deadline {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        // No observed movement => abandon the bookmark; never
                        // restore the pre-insertion position on the next chunk.
                    }
                }
                let _ = answer.send(());
            }
        }
    }
}

pub fn capture(pid: u32) -> Option<Arc<FocusLease>> {
    let (tx, rx) = mpsc::channel();
    worker()?
        .try_send(Request::Capture {
            pid,
            due: Instant::now() + WAIT,
            answer: tx,
        })
        .ok()?;
    rx.recv_timeout(WAIT).ok().flatten()
}

pub fn restore(lease: &FocusLease, pid: u32) -> bool {
    if !lease.usable.load(Ordering::Relaxed) {
        return false;
    }
    let (tx, rx) = mpsc::channel();
    let Some(worker) = worker() else {
        return false;
    };
    if worker
        .try_send(Request::Restore {
            id: lease.id,
            pid,
            due: Instant::now() + WAIT,
            answer: tx,
        })
        .is_err()
    {
        return false;
    }
    rx.recv_timeout(WAIT).unwrap_or(false)
}

pub fn inserted(lease: &FocusLease) {
    let (tx, rx) = mpsc::channel();
    if let Some(worker) = worker() {
        if worker
            .try_send(Request::Refresh {
                id: lease.id,
                answer: tx,
            })
            .is_ok()
        {
            if rx.recv_timeout(WAIT).is_err() {
                lease.usable.store(false, Ordering::Relaxed);
            }
        } else {
            lease.usable.store(false, Ordering::Relaxed);
        }
    }
}

pub fn invalidate_caret(lease: &FocusLease) {
    if let Some(worker) = worker() {
        if worker.try_send(Request::InvalidateCaret(lease.id)).is_err() {
            lease.usable.store(false, Ordering::Relaxed);
        }
    }
}

fn restore_native_now(window: isize, child: isize, pid: u32) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsChild, IsWindow};
    let raw = HWND(window as *mut _);
    let child_raw = HWND(child as *mut _);
    if !unsafe { IsWindow(raw) }.as_bool()
        || !unsafe { IsWindow(child_raw) }.as_bool()
        || super::target::window_pid(window) != Some(pid)
        || super::target::window_pid(child) != Some(pid)
        || (child != window && !unsafe { IsChild(raw, child_raw) }.as_bool())
    {
        return false;
    }
    let current = unsafe { GetCurrentThreadId() };
    let target = unsafe { GetWindowThreadProcessId(raw, None) };
    let attached =
        current != target && unsafe { AttachThreadInput(current, target, true) }.as_bool();
    if current != target && !attached {
        return false;
    }
    let _ = unsafe { SetFocus(child_raw) };
    if attached {
        let _ = unsafe { AttachThreadInput(current, target, false) };
    }
    super::target::focused_child(window) == Some(child)
}

pub fn restore_native(window: isize, child: isize, pid: u32) -> bool {
    let (tx, rx) = mpsc::channel();
    let Some(worker) = worker() else {
        return false;
    };
    if worker
        .try_send(Request::NativeFocus {
            window,
            child,
            pid,
            due: Instant::now() + WAIT,
            answer: tx,
        })
        .is_err()
    {
        return false;
    }
    rx.recv_timeout(WAIT).unwrap_or(false)
}
